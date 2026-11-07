# dmgbuild settings for the BGPaint installer image, run by
# script/bundle-macos-dmg. dmgbuild writes the Finder layout (.DS_Store) itself,
# without Finder or AppleScript, so it works on headless CI runners.
# `defines` comes from dmgbuild's -D flags: app (path to BGPaint.app) and
# assets (this directory; dmgbuild runs the file without `__file__`).
import os.path

_here = defines["assets"]  # noqa: F821 (injected by dmgbuild)
_app = defines["app"]  # noqa: F821 (injected by dmgbuild)
_app_name = os.path.basename(_app)

format = "UDZO"
compression_level = 9
filesystem = "HFS+"

files = [_app]
symlinks = {"Applications": "/Applications"}
# No hide_extensions: SetFile would put a FinderInfo xattr on the signed
# bundle, which `codesign --strict` rejects. Finder hides ".app" anyway.

icon = os.path.join(_here, "volume-icon.icns")
# background@2x.png next to it is merged into a HiDPI TIFF by dmgbuild.
background = os.path.join(_here, "background.png")

# Same window as BGDesk's DMG: 800x480 (the 800x400 art plus the title bar),
# icon view without toolbar, sidebar or status bar, 128px icons, 16pt labels.
window_rect = ((200, 120), (800, 480))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
arrange_by = None
icon_size = 128
text_size = 16
icon_locations = {
    _app_name: (200, 190),
    "Applications": (600, 190),
    # Off-window, as in BGDesk's layout, for Finders that show hidden files.
    ".background.tiff": (2000, 2000),
    ".VolumeIcon.icns": (2000, 2000),
    ".DS_Store": (2000, 2000),
    ".fseventsd": (2000, 2000),
    ".Trashes": (2000, 2000),
}
