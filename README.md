# BGPaint

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
./target/release/bgpaint [image]
```

On Linux you need the usual windowing headers (`libxkbcommon`, `wayland`, `libGL`).

## License

MIT. Bundled fonts and icons are listed in [THIRD_PARTY.md](THIRD_PARTY.md).
BGPaint is an independent project and is not affiliated with Microsoft.
