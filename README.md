<p align="center">
  <img src="crates/bgpaint/assets/icon.svg" alt="BGPaint icon" width="128" height="128">
</p>

<h1 align="center">BGPaint</h1>

A fast, native clone of **Windows 11 Paint** for **Linux** and **macOS**, written in Rust.

> Work in progress — see the roadmap below.

- Same layout as Windows 11 Paint (toolbar groups, menus, layers panel, status bar), light and dark themes
- Starts in tens of milliseconds: GPU rendering through OpenGL, fonts and icons embedded, no system scans
- `#![forbid(unsafe_code)]` everywhere
- Opens PNG, JPEG, GIF, BMP, ICO, TIFF, WebP, SVG/SVGZ, TGA, PNM, QOI, HDR, OpenEXR, DDS, farbfeld and OpenRaster
- Saves PNG, JPEG, BMP, GIF, WebP, TIFF (flattened, transparency preserved) and layered OpenRaster projects

## Building

```sh
cargo build --release
./target/release/bgpaint [images…]
```

On Linux you need the usual windowing headers (`libxkbcommon`, `wayland`, `libGL`).

## Install

On Linux, paste this into a terminal:

```sh
curl -fsSL https://github.com/belizariogr/bgpaint/releases/latest/download/install.sh | sh
```

It finds your distro's package manager and installs the matching package from the
latest release, system-wide (asks for your password through `sudo`):

| Distro | Package |
|---|---|
| Arch, Manjaro, EndeavourOS… | `.pkg.tar.zst` (pacman) |
| Debian, Ubuntu, Mint, Pop!_OS… | `.deb` (apt) |
| Fedora, RHEL, openSUSE… | `.rpm` (dnf, zypper) |
| anything else, or no `sudo` | the binary, into `~/.local` (no root) |

`BGPAINT_INSTALL=local` keeps it in `~/.local` on any distro, and `BGPAINT_VERSION=v0.6.0`
picks a release, e.g. `curl -fsSL …/install.sh | BGPAINT_INSTALL=local sh`. Without `curl`,
use `wget -qO- <same URL> | sh`. On macOS, download the `.dmg` from the
[releases page](https://github.com/belizariogr/bgpaint/releases/latest).

## Build packages

Releases are built by GitHub Actions (`.github/workflows/release.yml`): run the
**Release** workflow by hand (Actions ▸ Release ▸ Run workflow) and it bumps the
version in `Cargo.toml` (patch by default; minor/major, or an exact version, on
request), builds every package, creates the `v*` tag and publishes the GitHub
Release. Pushing a `v*` tag yourself makes a draft release instead.

| OS | Packages |
|---|---|
| macOS (Apple silicon, Intel) | `BGPaint.app` inside a `.dmg` (min. macOS 10.15.7) |
| Linux x86_64 | `.deb`, `.rpm`, `.AppImage`, pacman `.pkg.tar.zst` (plus its `.tar.gz` + `PKGBUILD`), and the `bgpaint-linux-x86_64.tar.gz` that `install.sh` unpacks |

cargo-packager has no `.rpm` and writes only a `PKGBUILD` for pacman, so two more
scripts make those: `script/bundle-pacman <dir with PKGBUILD and tarball> <out dir>`
(needs `makepkg`) and `script/bundle-rpm <bgpaint-linux-x86_64.tar.gz> <out dir>`
(needs `rpmbuild`).

On Linux, these scripts install without a package manager. Only the
system-wide one needs root:

```sh
script/install-linux                    # build this checkout, into ~/.local
script/install-linux-system             # build this checkout, into /usr (asks for sudo)
script/uninstall.sh                     # remove the ~/.local install; asks before deleting settings
script/install-linux-system --uninstall
```

All of them add a launcher entry (with the icon and the image types BGPaint opens) and the
`bgpaint` command. `script/test-install` checks the scripts in a throwaway home.

To build packages locally, use [cargo-packager](https://github.com/crabnebula-dev/cargo-packager)
(configured in `[package.metadata.packager]` of `crates/bgpaint/Cargo.toml`):

```sh
cargo install cargo-packager --locked
cd crates/bgpaint
cargo packager --release                     # this OS's default formats
cargo packager --release --formats deb       # just one format
```

Packages are written to `target/release/` (or `target/<triple>/release/` with
`--target`). The release `.dmg`, with the installer window art, comes from
`script/bundle-macos-dmg target/release/BGPaint.app out.dmg`, which needs
`pip install dmgbuild` (`script/test-macos-dmg` checks its layout). On rolling
distros whose libraries use `.relr.dyn` (e.g. Arch), the AppImage step needs
`NO_STRIP=1`; release AppImages are built on Ubuntu 22.04 for wider glibc compatibility.

The icon is drawn in `crates/bgpaint/assets/icon.svg`; after editing it, run
`script/generate-app-icons` (PNGs, `.icns`, `.ico`) and
`script/generate-macos-dmg-assets` (DMG background and volume icon).

## Updates

At startup BGPaint asks GitHub for the latest release and, when it is newer, downloads the
package that matches how it was installed, checks it against the SHA-256 digest GitHub
publishes, and installs it; the new version runs after a restart (Settings ▸ Updates turns the
check off or runs it by hand, as does BGPaint ▸ Check for Updates on macOS):

| Installed with | Updated from | Password |
|---|---|---|
| `install.sh` (`~/.local/bgpaint.app`) | `bgpaint-linux-<arch>.tar.gz` | no, silent |
| `script/install-linux-system` (`/usr`, `/usr/local`) | the same tarball | root (polkit, or `sudo`) |
| `.deb` | the `.deb` (apt) | root (polkit, or `sudo`) |
| `.rpm` | the `.rpm` (dnf, zypper, yum) | root (polkit, or `sudo`) |
| pacman `.pkg.tar.zst` | the `.pkg.tar.zst` (pacman) | root (polkit, or `sudo`) |
| AppImage | the `.AppImage` | only if its folder is not writable |
| macOS `.dmg` | the `.dmg` | only if the app's folder is not writable |

Root is requested through the desktop's polkit agent (`pkexec`); without one, BGPaint asks
for the password itself and runs `sudo`. Copies owned by a package with another name (an AUR
build, say) or built from source only report the new version. Builds with debug assertions,
or `BGPAINT_NO_UPDATE=1`, skip the startup check. The installer steps are in `crates/bgpaint/src/update/install.sh`.

## License

MIT. Bundled fonts and icons are listed in [THIRD_PARTY.md](THIRD_PARTY.md).
BGPaint is an independent project and is not affiliated with Microsoft.
