#!/usr/bin/env sh
set -eu

# Installs BGPaint from its GitHub releases, with the package your distro's
# package manager installs when there is one:
#
#   Arch, Manjaro, EndeavourOS… (pacman)   .pkg.tar.zst   system-wide (sudo)
#   Debian, Ubuntu, Mint… (apt)            .deb           system-wide (sudo)
#   Fedora, RHEL, openSUSE… (dnf, zypper)  .rpm           system-wide (sudo)
#   anything else                          tarball        ~/.local, no root
#
# Environment:
#   BGPAINT_VERSION      release tag, e.g. v0.1.0 (default: latest)
#   BGPAINT_INSTALL      auto (default), pacman, deb, rpm or local (~/.local tarball)
#   BGPAINT_BUNDLE_PATH  install this local tarball into ~/.local instead of downloading

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
    # apt reads the .deb as its own unprivileged user.
    chmod 755 "$temp"

    version="${BGPAINT_VERSION:-latest}"
    bundle="$temp/bgpaint-linux-$arch.tar.gz"
    if [ -n "${BGPAINT_BUNDLE_PATH:-}" ]; then
        cp "$BGPAINT_BUNDLE_PATH" "$bundle"
        install_local "$bundle"
        return
    fi

    find_fetcher
    method="${BGPAINT_INSTALL:-auto}"
    case "$method" in
        auto) method="$(package_manager)" ;;
        pacman | deb | rpm | local) ;;
        *)
            echo "Unknown BGPAINT_INSTALL: $method (use auto, pacman, deb, rpm or local)"
            exit 1
            ;;
    esac

    if [ "$method" != local ]; then
        if [ "$(id -u)" -eq 0 ]; then
            sudo=""
        elif command -v sudo >/dev/null 2>&1; then
            sudo="sudo"
        elif command -v doas >/dev/null 2>&1; then
            sudo="doas"
        else
            echo "Neither sudo nor doas found: installing into ~/.local instead of a $method package."
            method=local
        fi
    fi

    assets="$(release_assets)"
    if [ "$method" != local ]; then
        url="$(echo "$assets" | grep -E "$(package_pattern "$method")" | head -n 1 || true)"
        if [ -n "$url" ]; then
            install_package "$method" "$url"
            return
        fi
        echo "No $method package for $arch in release $version: installing into ~/.local instead."
    fi

    url="$(echo "$assets" | grep -E "/bgpaint-linux-$arch\.tar\.gz\$" | head -n 1 || true)"
    if [ -z "$url" ]; then
        if [ "$version" = latest ]; then
            url="https://github.com/$REPO/releases/latest/download/bgpaint-linux-$arch.tar.gz"
        else
            url="https://github.com/$REPO/releases/download/$version/bgpaint-linux-$arch.tar.gz"
        fi
    fi
    download "$url" "$bundle"
    install_local "$bundle"
}

# The package format this system's package manager installs; local when none.
package_manager() {
    ids=""
    if [ -r /etc/os-release ]; then
        ids="$(. /etc/os-release && echo " ${ID:-} ${ID_LIKE:-} ")"
    fi
    case "$ids" in
        *" arch "*) has pacman && echo pacman && return ;;
        *" debian "* | *" ubuntu "*) has apt-get && echo deb && return ;;
        *fedora* | *rhel* | *centos* | *suse* | *mageia*) rpm_installer >/dev/null && echo rpm && return ;;
    esac
    # Unknown ID: go by the tools present.
    if has pacman; then
        echo pacman
    elif has apt-get; then
        echo deb
    elif rpm_installer >/dev/null; then
        echo rpm
    else
        echo local
    fi
}

has() {
    command -v "$1" >/dev/null 2>&1
}

rpm_installer() {
    for tool in dnf zypper yum; do
        if has "$tool"; then
            echo "$tool"
            return 0
        fi
    done
    return 1
}

# The release asset of each format, for this CPU.
package_pattern() {
    case "$1" in
        pacman) echo "/bgpaint-[^/]*-$arch\.pkg\.tar\.zst\$" ;;
        rpm) echo "/bgpaint-[^/]*\.$arch\.rpm\$" ;;
        deb)
            case "$arch" in
                x86_64) echo "/bgpaint_[^/]*_amd64\.deb\$" ;;
                *) echo "/bgpaint_[^/]*_arm64\.deb\$" ;;
            esac
            ;;
    esac
}

# The download URLs of the release's files, one per line (none if GitHub's API
# cannot be reached; the tarball has a fixed URL anyway).
release_assets() {
    if [ "$version" = latest ]; then
        api="https://api.github.com/repos/$REPO/releases/latest"
    else
        api="https://api.github.com/repos/$REPO/releases/tags/$version"
    fi
    $fetch_quiet "$api" 2>/dev/null \
        | grep -o '"browser_download_url": *"[^"]*"' \
        | sed 's/.*"\(https:[^"]*\)"$/\1/' || true
}

install_package() {
    file="$temp/${2##*/}"
    download "$2" "$file"
    echo "Installing ${file##*/} (your password may be asked for)"
    case "$1" in
        pacman) $sudo pacman -U --noconfirm "$file" ;;
        deb) $sudo apt-get install -y "$file" ;;
        rpm)
            case "$(rpm_installer)" in
                zypper) $sudo zypper --non-interactive install --allow-unsigned-rpm "$file" ;;
                *) $sudo "$(rpm_installer)" install -y "$file" ;;
            esac
            ;;
    esac

    # A copy installed earlier into ~/.local would shadow the package (PATH and
    # launcher entry); remove it. Settings in ~/.config/bgpaint are kept.
    if [ -d "$HOME/.local/bgpaint.app" ]; then
        rm -rf "$HOME/.local/bgpaint.app"
        rm -f "$HOME/.local/bin/bgpaint" "$HOME/.local/share/applications/$DESKTOP"
        echo "Removed the previous BGPaint copy in ~/.local."
    fi
    echo "BGPaint has been installed. Run it from your app launcher or with 'bgpaint'"
}

find_fetcher() {
    if has curl; then
        fetch="curl -fL -o"
        fetch_quiet="curl -fsSL"
    elif has wget; then
        fetch="wget -O"
        fetch_quiet="wget -qO-"
    else
        echo "Could not find 'curl' or 'wget' in your path"
        exit 1
    fi
}

download() {
    echo "Downloading $1"
    if ! $fetch "$2" "$1"; then
        echo "Could not download $1"
        echo "There may be no release for $arch yet. To build from source: script/install-linux"
        exit 1
    fi
}

install_local() {
    install_bundle "$1"

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
