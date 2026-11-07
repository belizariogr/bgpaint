#!/usr/bin/env sh
set -eu

# Removes what script/install.sh installed. Your settings (theme, recent files, custom
# colors) stay unless you ask to delete them, and that question is only asked on a
# terminal.

main() {
    config="${XDG_CONFIG_HOME:-$HOME/.config}/bgpaint"
    data="${XDG_DATA_HOME:-$HOME/.local/share}/bgpaint"

    rm -rf "$HOME/.local/bgpaint.app"
    rm -f "$HOME/.local/bin/bgpaint"
    rm -f "$HOME/.local/share/applications/io.github.belizariogr.bgpaint.desktop"
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$HOME/.local/share/applications" >/dev/null 2>&1 || true
    fi
    echo "BGPaint has been uninstalled."

    if [ -d "$config" ] || [ -d "$data" ]; then
        response=""
        if [ -t 0 ]; then
            printf "Also delete your BGPaint settings (%s) and temporary files (%s)? [y/N] " "$config" "$data"
            read -r response
        fi
        case "$response" in
            [yY] | [yY][eE][sS])
                rm -rf "$config" "$data"
                echo "Deleted $config and $data"
                ;;
            *)
                echo "Your settings are still in $config"
                ;;
        esac
    fi
}

main "$@"
