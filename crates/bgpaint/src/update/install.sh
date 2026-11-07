#!/bin/sh
# Installs a downloaded BGPaint update. Embedded in the app and run by src/update/mod.rs,
# as root (pkexec, sudo or the macOS administrator prompt) when the install needs it.
#
#   install.sh bundle   <tarball>  <bgpaint.app folder>   (script/install.sh, in ~/.local)
#   install.sh prefix   <tarball>  <prefix>               (script/install-linux-system)
#   install.sh appimage <AppImage> <AppImage to replace>
#   install.sh deb      <package.deb>
#   install.sh macapp   <dmg>      <BGPaint.app>
#
# The new files are put next to the old ones first and swapped in with a rename, so a
# failure leaves the working install alone.

set -eu
# Tells the app that authorization succeeded and this script ran (see `elevate`).
echo "bgpaint-update: start"

mode=$1
package=$2
target=${3:-}
umask 022

fail() {
    echo "$1" >&2
    exit 3
}

# Unpacks the release tarball (bgpaint.app/{bin,share}) into $1.
unpack() {
    tar -xzf "$package" -C "$1"
    [ -x "$1/bgpaint.app/bin/bgpaint" ] || fail "Invalid package: bin/bgpaint is missing."
}

case "$mode" in
bundle)
    stage=$(mktemp -d "$(dirname "$target")/.bgpaint-update.XXXXXX")
    trap 'rm -rf "$stage"' EXIT
    unpack "$stage"
    mv "$target" "$stage/old"
    if ! mv "$stage/bgpaint.app" "$target"; then
        mv "$stage/old" "$target"
        fail "Could not replace $target."
    fi
    ;;

prefix)
    stage=$(mktemp -d)
    trap 'rm -rf "$stage"' EXIT
    unpack "$stage"
    app=$stage/bgpaint.app
    name=io.github.belizariogr.bgpaint.desktop
    # Absolute paths, as script/install-linux-system writes them.
    sed -i -e "s|^Exec=bgpaint|Exec=$target/bin/bgpaint|" \
        -e "s|^Icon=bgpaint|Icon=$target/share/icons/hicolor/512x512/apps/bgpaint.png|" \
        "$app/share/applications/$name"
    # A rename, not a copy over the running binary.
    install -Dm755 "$app/bin/bgpaint" "$target/bin/.bgpaint.new"
    mv -f "$target/bin/.bgpaint.new" "$target/bin/bgpaint"
    install -Dm644 "$app/share/applications/$name" "$target/share/applications/$name"
    for dir in "$app"/share/icons/hicolor/*/apps; do
        for file in "$dir"/*; do
            install -Dm644 "$file" "$target/${dir#"$app"/}/$(basename "$file")"
        done
    done
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -f "$target/share/icons/hicolor" >/dev/null 2>&1 || true
    fi
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$target/share/applications" >/dev/null 2>&1 || true
    fi
    ;;

appimage)
    new="$(dirname "$target")/.bgpaint-update.$$.AppImage"
    trap 'rm -f "$new"' EXIT
    cp "$package" "$new"
    chmod 755 "$new"
    mv -f "$new" "$target"
    ;;

deb)
    export DEBIAN_FRONTEND=noninteractive
    if command -v apt-get >/dev/null 2>&1; then
        apt-get install -y "$package"
    else
        dpkg -i "$package"
    fi
    ;;

macapp)
    parent=$(dirname "$target")
    mnt=$(mktemp -d "${TMPDIR:-/tmp}/bgpaint-dmg.XXXXXX")
    new="$parent/.bgpaint-update.$$.app"
    old="$parent/.bgpaint-old.$$.app"
    cleanup() {
        hdiutil detach -quiet "$mnt" 2>/dev/null || hdiutil detach -force -quiet "$mnt" 2>/dev/null || true
        rmdir "$mnt" 2>/dev/null || true
        rm -rf "$new" "$old"
    }
    trap cleanup EXIT
    hdiutil attach -nobrowse -readonly -noautoopen -mountpoint "$mnt" "$package" >/dev/null
    src=""
    for app in "$mnt"/*.app; do
        if [ -d "$app" ]; then
            src=$app
            break
        fi
    done
    [ -n "$src" ] || fail "The disk image has no app."
    ditto "$src" "$new"
    # A signed copy only accepts an update signed by the same developer.
    team() { codesign -dv "$1" 2>&1 | sed -n 's/^TeamIdentifier=//p'; }
    current=$(team "$target")
    if [ -n "$current" ] && [ "$current" != "not set" ]; then
        codesign --verify --deep --strict "$new" || fail "The new version's signature is not valid."
        [ "$(team "$new")" = "$current" ] || fail "The new version is signed by a different developer."
    fi
    mv "$target" "$old"
    if ! mv "$new" "$target"; then
        mv "$old" "$target"
        fail "Could not replace $target."
    fi
    ;;

*)
    fail "Unknown install type: $mode"
    ;;
esac
