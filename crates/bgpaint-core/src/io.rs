//! Opening and saving images.
//!
//! Every raster format supported by the `image` crate can be opened (PNG, JPEG, GIF, BMP, ICO,
//! TIFF, WebP, TGA, PNM, QOI, HDR, OpenEXR, DDS, farbfeld), plus SVG/SVGZ through resvg and layered
//! OpenRaster projects. Images are saved flattened; transparency is preserved whenever the format
//! supports it.

use crate::document::Document;
use crate::raster::{self, TooLarge};
use crate::{color::Color, ora};
use std::io::Cursor;
use std::path::Path;
use tiny_skia::Pixmap;

#[derive(Debug)]
pub enum IoError {
    Io(std::io::Error),
    Decode(String),
    Encode(String),
    TooLarge(TooLarge),
    Unsupported,
}

impl std::fmt::Display for IoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IoError::Io(e) => write!(f, "{e}"),
            IoError::Decode(e) => write!(f, "could not read the image: {e}"),
            IoError::Encode(e) => write!(f, "could not write the image: {e}"),
            IoError::TooLarge(e) => write!(f, "{e}"),
            IoError::Unsupported => write!(f, "unsupported file format"),
        }
    }
}

impl std::error::Error for IoError {}

impl From<std::io::Error> for IoError {
    fn from(e: std::io::Error) -> Self {
        IoError::Io(e)
    }
}

impl From<TooLarge> for IoError {
    fn from(e: TooLarge) -> Self {
        IoError::TooLarge(e)
    }
}

impl From<image::ImageError> for IoError {
    fn from(e: image::ImageError) -> Self {
        match e {
            image::ImageError::IoError(e) => IoError::Io(e),
            image::ImageError::Unsupported(_) => IoError::Unsupported,
            image::ImageError::Encoding(e) => IoError::Encode(e.to_string()),
            other => IoError::Decode(other.to_string()),
        }
    }
}

/// Formats that can be written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SaveFormat {
    Png,
    Jpeg,
    Bmp,
    Gif,
    Webp,
    Tiff,
    /// Layered OpenRaster project.
    Ora,
}

impl SaveFormat {
    pub const IMAGE_FORMATS: [SaveFormat; 6] =
        [SaveFormat::Png, SaveFormat::Jpeg, SaveFormat::Bmp, SaveFormat::Gif, SaveFormat::Webp, SaveFormat::Tiff];

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            SaveFormat::Png => &["png"],
            SaveFormat::Jpeg => &["jpg", "jpeg", "jpe", "jfif"],
            SaveFormat::Bmp => &["bmp", "dib"],
            SaveFormat::Gif => &["gif"],
            SaveFormat::Webp => &["webp"],
            SaveFormat::Tiff => &["tif", "tiff"],
            SaveFormat::Ora => &["ora"],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            SaveFormat::Png => "PNG",
            SaveFormat::Jpeg => "JPEG",
            SaveFormat::Bmp => "BMP",
            SaveFormat::Gif => "GIF",
            SaveFormat::Webp => "WEBP",
            SaveFormat::Tiff => "TIFF",
            SaveFormat::Ora => "OpenRaster",
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        [SaveFormat::Png, SaveFormat::Jpeg, SaveFormat::Bmp, SaveFormat::Gif, SaveFormat::Webp, SaveFormat::Tiff, SaveFormat::Ora]
            .into_iter()
            .find(|f| f.extensions().contains(&ext.as_str()))
    }

    /// Whether the format keeps an alpha channel.
    pub fn supports_alpha(self) -> bool {
        !matches!(self, SaveFormat::Jpeg)
    }
}

/// Extensions accepted by the open dialog.
pub const OPEN_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "jpe", "jfif", "gif", "bmp", "dib", "ico", "tif", "tiff", "webp", "svg", "svgz", "tga", "pnm", "pbm", "pgm",
    "ppm", "pam", "qoi", "hdr", "exr", "dds", "ff", "ora",
];

/// The result of opening a file.
pub struct Opened {
    pub document: Document,
    /// Format to use for a plain "Save" (None if the file can't be written back in its format).
    pub save_format: Option<SaveFormat>,
}

/// Opens any supported file.
pub fn open(path: &Path) -> Result<Opened, IoError> {
    let bytes = std::fs::read(path)?;
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    if ext == "ora" || ora::is_ora(&bytes) {
        return Ok(Opened { document: ora::read(&bytes)?, save_format: Some(SaveFormat::Ora) });
    }
    let pixmap = decode(&bytes, &ext)?;
    let save_format = SaveFormat::from_path(path).filter(|f| *f != SaveFormat::Ora);
    Ok(Opened { document: Document::from_image(pixmap), save_format })
}

fn looks_like_svg(bytes: &[u8], ext: &str) -> bool {
    if ext == "svg" || ext == "svgz" {
        return true;
    }
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return false;
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
    head.contains("<svg")
}

/// Decodes an image file in memory. `ext` is a lowercase extension hint (may be empty).
pub fn decode(bytes: &[u8], ext: &str) -> Result<Pixmap, IoError> {
    if looks_like_svg(bytes, ext) {
        return decode_svg(bytes);
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    if reader.format().is_none() {
        if let Some(f) = image::ImageFormat::from_extension(ext) {
            reader.set_format(f);
        }
    }
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(4 * raster::MAX_AREA);
    limits.max_image_width = Some(raster::MAX_SIDE);
    limits.max_image_height = Some(raster::MAX_SIDE);
    reader.limits(limits);
    let img = reader.decode()?.into_rgba8();
    Ok(raster::from_straight_rgba(img.width(), img.height(), img.as_raw())?)
}

/// Rasterizes an SVG at its intrinsic size (scaled down if it exceeds the size limits).
pub fn decode_svg(bytes: &[u8]) -> Result<Pixmap, IoError> {
    let mut db = resvg::usvg::fontdb::Database::new();
    db.load_system_fonts();
    let opt = resvg::usvg::Options { fontdb: std::sync::Arc::new(db), ..Default::default() };
    let tree = resvg::usvg::Tree::from_data(bytes, &opt).map_err(|e| IoError::Decode(e.to_string()))?;
    let size = tree.size();
    let limit = 8192.0f32;
    let scale = (limit / size.width().max(size.height())).min(1.0);
    let w = (size.width() * scale).ceil().max(1.0) as u32;
    let h = (size.height() * scale).ceil().max(1.0) as u32;
    let mut pixmap = raster::try_pixmap(w, h)?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok(pixmap)
}

/// Encodes a flattened image.
pub fn encode(pixmap: &Pixmap, format: SaveFormat) -> Result<Vec<u8>, IoError> {
    use image::ImageEncoder;
    let (w, h) = (pixmap.width(), pixmap.height());
    let rgba = raster::to_straight_rgba(pixmap);
    let opaque = !raster::has_transparency(pixmap);
    let mut out = Vec::new();
    match format {
        SaveFormat::Png => {
            let enc = image::codecs::png::PngEncoder::new(&mut out);
            if opaque {
                enc.write_image(&rgb_only(&rgba), w, h, image::ExtendedColorType::Rgb8)?;
            } else {
                enc.write_image(&rgba, w, h, image::ExtendedColorType::Rgba8)?;
            }
        }
        SaveFormat::Jpeg => {
            let rgb = flatten_on(&rgba, Color::WHITE);
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92).write_image(&rgb, w, h, image::ExtendedColorType::Rgb8)?;
        }
        SaveFormat::Bmp => {
            let enc = image::codecs::bmp::BmpEncoder::new(&mut out);
            if opaque {
                enc.write_image(&rgb_only(&rgba), w, h, image::ExtendedColorType::Rgb8)?;
            } else {
                enc.write_image(&rgba, w, h, image::ExtendedColorType::Rgba8)?;
            }
        }
        SaveFormat::Gif => {
            let mut enc = image::codecs::gif::GifEncoder::new_with_speed(&mut out, 10);
            let img = image::RgbaImage::from_raw(w, h, rgba).ok_or_else(|| IoError::Encode("bad buffer".into()))?;
            enc.encode_frame(image::Frame::new(img))?;
        }
        SaveFormat::Webp => {
            image::codecs::webp::WebPEncoder::new_lossless(&mut out).write_image(&rgba, w, h, image::ExtendedColorType::Rgba8)?;
        }
        SaveFormat::Tiff => {
            image::codecs::tiff::TiffEncoder::new(Cursor::new(&mut out)).write_image(&rgba, w, h, image::ExtendedColorType::Rgba8)?;
        }
        SaveFormat::Ora => return Err(IoError::Unsupported),
    }
    Ok(out)
}

fn rgb_only(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect()
}

fn flatten_on(rgba: &[u8], bg: Color) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|p| {
            let a = u32::from(p[3]);
            let mix = |c: u8, b: u8| ((u32::from(c) * a + u32::from(b) * (255 - a) + 127) / 255) as u8;
            [mix(p[0], bg.r), mix(p[1], bg.g), mix(p[2], bg.b)]
        })
        .collect()
}

/// Writes bytes atomically (temporary file + rename) so a failed save never corrupts the file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), IoError> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

/// Saves the document in `format` (images are flattened).
pub fn save(doc: &Document, path: &Path, format: SaveFormat) -> Result<u64, IoError> {
    let bytes = match format {
        SaveFormat::Ora => ora::write(doc)?,
        f => encode(&doc.flatten(), f)?,
    };
    write_atomic(path, &bytes)?;
    Ok(bytes.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::PixelRect;

    fn sample() -> Pixmap {
        let mut p = raster::pixmap(8, 6);
        raster::fill_region(&mut p, PixelRect::new(0, 0, 4, 6), Color::rgb(200, 30, 40));
        p
    }

    #[test]
    fn roundtrip_formats_keep_transparency() {
        let p = sample();
        for f in [SaveFormat::Png, SaveFormat::Bmp, SaveFormat::Gif, SaveFormat::Webp, SaveFormat::Tiff] {
            let bytes = encode(&p, f).unwrap();
            let back = decode(&bytes, f.extensions()[0]).unwrap();
            assert_eq!((back.width(), back.height()), (8, 6), "{f:?}");
            assert_eq!(back.pixel(6, 3).unwrap().alpha(), 0, "{f:?} should keep transparency");
            let c = raster::get_pixel(&back, 1, 1).unwrap();
            assert!(c.distance(&Color::rgb(200, 30, 40)) <= 8, "{f:?} {c:?}");
        }
    }

    #[test]
    fn jpeg_is_flattened_on_white() {
        let bytes = encode(&sample(), SaveFormat::Jpeg).unwrap();
        let back = decode(&bytes, "jpg").unwrap();
        let c = raster::get_pixel(&back, 7, 5).unwrap();
        assert!(c.distance(&Color::WHITE) <= 6);
    }

    #[test]
    fn svg_renders() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect x="0" y="0" width="10" height="10" fill="#ff0000"/></svg>"##;
        let p = decode(svg, "").unwrap();
        assert_eq!((p.width(), p.height()), (20, 10));
        assert_eq!(raster::get_pixel(&p, 5, 5), Some(Color::rgb(255, 0, 0)));
        assert_eq!(p.pixel(15, 5).unwrap().alpha(), 0);
    }

    #[test]
    fn format_from_path() {
        assert_eq!(SaveFormat::from_path(Path::new("a/b.JPG")), Some(SaveFormat::Jpeg));
        assert_eq!(SaveFormat::from_path(Path::new("x.ora")), Some(SaveFormat::Ora));
        assert_eq!(SaveFormat::from_path(Path::new("x.svg")), None);
    }

    #[test]
    fn open_and_save_files() {
        let dir = std::env::temp_dir().join(format!("bgpaint-io-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let doc = Document::from_image(sample());
        let path = dir.join("t.webp");
        save(&doc, &path, SaveFormat::Webp).unwrap();
        let opened = open(&path).unwrap();
        assert_eq!(opened.save_format, Some(SaveFormat::Webp));
        assert!(!opened.document.background().visible);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
