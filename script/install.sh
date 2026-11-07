#!/usr/bin/env sh
set -eu

# Downloads a tarball from the BGPaint GitHub releases and unpacks it into
# ~/.local/. Environment:
#   BGPAINT_VERSION      release tag, e.g. v0.1.0 (default: latest)
#   BGPAINT_BUNDLE_PATH  install this local tarball instead of downloading

REPO="belizariogr/bgpaint"
DESKTOP="io.github.belizariogr.bgpaint.desktop"

main() {
    platform="$(uname -s)"
    arch="$(uname -m)"
    if [ "$platform" != "Linux" ]; then
        echo "This script installs BGPaint on Linux. Unsupported platform: $platform"
        echo "Other systems: https://github.com/$REPO/releases"
        exit 1
    fi
    case "$arch" in
        x86_64 | aarch64) ;;
        arm64) arch="aarch64" ;;
        *)
            echo "Unsupported architecture: $arch"
            exit 1
            ;;
    esac

    # Use TMPDIR if available (for environments with non-standard temp directories)
    temp="$(mktemp -d "${TMPDIR:-/tmp}/bgpaint-XXXXXX")"
    trap 'rm -rf "$temp"' EXIT

    bundle="$temp/bgpaint-linux-$arch.tar.gz"
    if [ -n "${BGPAINT_BUNDLE_PATH:-}" ]; then
        cp "$BGPAINT_BUNDLE_PATH" "$bundle"
    else
        download "$bundle"
    fi

    install_bundle "$bundle"

    if [ "$(command -v bgpaint)" = "$HOME/.local/bin/bgpaint" ]; then
        echo "BGPaint has been installed. Run with 'bgpaint'"
    else
        echo "BGPaint has been installed. To run it from your terminal, add ~/.local/bin to your PATH"
        echo "Run:"
        case "${SHELL:-}" in
            *zsh)
                echo "   echo 'export PATH=\$HOME/.local/bin:\$PATH' >> ~/.zshrc"
                echo "   source ~/.zshrc"
                ;;
            *fish)
                echo "   fish_add_path -U $HOME/.local/bin"
                ;;
            *)
                echo "   echo 'export PATH=\$HOME/.local/bin:\$PATH' >> ~/.bashrc"
                echo "   source ~/.bashrc"
                ;;
        esac
        echo "To run BGPaint now, '~/.local/bin/bgpaint'"
    fi
}

download() {
    version="${BGPAINT_VERSION:-latest}"
    if [ "$version" = "latest" ]; then
        url="https://github.com/$REPO/releases/latest/download/bgpaint-linux-$arch.tar.gz"
    else
        url="https://github.com/$REPO/releases/download/$version/bgpaint-linux-$arch.tar.gz"
    fi
    echo "Downloading BGPaint version: $version"
    if command -v curl >/dev/null 2>&1; then
        fetch="curl -fL -o"
    elif command -v wget >/dev/null 2>&1; then
        fetch="wget -O"
    else
        echo "Could not find 'curl' or 'wget' in your path"
        exit 1
    fi
    if ! $fetch "$1" "$url"; then
        echo "Could not download $url"
        echo "There may be no release for $arch yet. To build from source: script/install-linux"
        exit 1
    fi
}

install_bundle() {
    # Unpack aside first: a bad archive must not cost a working install.
    tar -xzf "$1" -C "$temp"
    if [ ! -x "$temp/bgpaint.app/bin/bgpaint" ]; then
        echo "Invalid BGPaint bundle: bin/bgpaint is missing"
        exit 1
    fi

    app="$HOME/.local/bgpaint.app"
    mkdir -p "$HOME/.local/bin" "$HOME/.local/share/applications"
    rm -rf "$app"
    mv "$temp/bgpaint.app" "$app"

    if command -v ldd >/dev/null 2>&1; then
        missing="$(ldd "$app/bin/bgpaint" 2>/dev/null | sed -n 's/^[[:space:]]*\(.*\) => not found$/\1/p')"
        if [ -n "$missing" ]; then
            echo "Warning: your system is missing libraries that BGPaint needs:"
            echo "$missing" | sed 's/^/    /'
            echo "Install them with your package manager, or BGPaint will fail to start."
        fi
    fi

    ln -sf "$app/bin/bgpaint" "$HOME/.local/bin/bgpaint"

    # Absolute paths: ~/.local/bin is often not in the PATH of desktop sessions.
    sed -e "s|^Exec=bgpaint|Exec=$app/bin/bgpaint|" \
        -e "s|^Icon=bgpaint|Icon=$app/share/icons/hicolor/512x512/apps/bgpaint.png|" \
        "$app/share/applications/$DESKTOP" \
        > "$HOME/.local/share/applications/$DESKTOP"
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$HOME/.local/share/applications" >/dev/null 2>&1 || true
    fi
}

main "$@"
