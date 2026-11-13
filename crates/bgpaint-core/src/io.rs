//! Opening and saving images.
//!
//! Every raster format supported by the `image` crate can be opened (PNG, JPEG, GIF, BMP, ICO,
//! TIFF, WebP, TGA, PNM, QOI, HDR, OpenEXR, DDS, farbfeld), plus SVG/SVGZ through resvg and layered
//! OpenRaster projects. Images are saved flattened; transparency is preserved whenever the format
//! supports it.

use crate::document::Document;
use crate::raster::{self, TooLarge};
use crate::{color::Color, ora};
use rayon::prelude::*;
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
    /// Full PNG: the image as every program shows it, plus the editable layers and vector
    /// objects when there is more to keep than the image (see [`keeps_project`]).
    Png,
    /// Optimized PNG: only the image, objects drawn in.
    PngOptimized,
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
            SaveFormat::Png | SaveFormat::PngOptimized => &["png"],
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
            SaveFormat::Png | SaveFormat::PngOptimized => "PNG",
            SaveFormat::Jpeg => "JPEG",
            SaveFormat::Bmp => "BMP",
            SaveFormat::Gif => "GIF",
            SaveFormat::Webp => "WEBP",
            SaveFormat::Tiff => "TIFF",
            SaveFormat::Ora => "OpenRaster",
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        Self::from_path_among(
            path,
            &[SaveFormat::Png, SaveFormat::Jpeg, SaveFormat::Bmp, SaveFormat::Gif, SaveFormat::Webp, SaveFormat::Tiff, SaveFormat::Ora],
        )
    }

    /// The first of `formats` that uses the extension of `path` (case does not matter).
    pub fn from_path_among(path: &Path, formats: &[SaveFormat]) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        formats.iter().copied().find(|f| f.extensions().contains(&ext.as_str()))
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
    let mut save_format = SaveFormat::from_path(path).filter(|f| *f != SaveFormat::Ora);
    // A full PNG opens with its layers and editable objects, unless the image was changed
    // elsewhere (resized, cropped...) by a program that kept the project that no longer matches.
    if crate::png::has_project(&bytes) {
        if let Some(project) = crate::png::embedded_project(&bytes)
            && let Ok(document) = ora::read(&project)
            && crate::png::dimensions(&bytes) == Some((document.width(), document.height()))
        {
            return Ok(Opened { document, save_format });
        }
        // A project that cannot be read (damaged, from a newer version, or kept by a program
        // that changed the image) opens as its image, but a plain "Save" asks where to write
        // instead of replacing the file and the project it holds.
        save_format = None;
    }
    let pixmap = decode(&bytes, &ext)?;
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
    // PNGs written by BGPaint decode on every core.
    if let Some(p) = crate::png::decode(bytes) {
        return Ok(p);
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    if reader.format().is_none()
        && let Some(f) = image::ImageFormat::from_extension(ext)
    {
        reader.set_format(f);
    }
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(4 * raster::MAX_AREA);
    limits.max_image_width = Some(raster::MAX_SIDE);
    limits.max_image_height = Some(raster::MAX_SIDE);
    reader.limits(limits);
    let img = match reader.decode()? {
        // The common opaque case, widened on every core.
        image::DynamicImage::ImageRgb8(rgb) => {
            let (w, h) = (rgb.width(), rgb.height());
            let src = rgb.into_raw();
            let mut out = vec![255u8; w as usize * h as usize * 4];
            out.as_chunks_mut::<4>().0.par_chunks_mut(1 << 14).zip(src.as_chunks::<3>().0.par_chunks(1 << 14)).for_each(|(o, s)| {
                for (o, s) in o.iter_mut().zip(s) {
                    o[..3].copy_from_slice(s);
                }
            });
            image::RgbaImage::from_raw(w, h, out).expect("buffer sized for the image")
        }
        other => other.into_rgba8(),
    };
    let (w, h) = (img.width(), img.height());
    Ok(raster::from_straight_rgba_vec(w, h, img.into_raw())?)
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

/// Deflate level of saved PNGs (and of the layers in projects).
pub const PNG_LEVEL: u32 = 6;

/// Encodes a flattened image.
pub fn encode(pixmap: &Pixmap, format: SaveFormat) -> Result<Vec<u8>, IoError> {
    use image::ImageEncoder;
    if matches!(format, SaveFormat::Png | SaveFormat::PngOptimized) {
        return Ok(crate::png::encode(pixmap, PNG_LEVEL));
    }
    let (w, h) = (pixmap.width(), pixmap.height());
    let rgba = raster::to_straight_rgba(pixmap);
    let opaque = !raster::has_transparency(pixmap);
    let mut out = Vec::new();
    match format {
        SaveFormat::Png | SaveFormat::PngOptimized => unreachable!("handled above"),
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
    let (px, _) = rgba.as_chunks::<4>();
    let mut out = vec![0u8; px.len() * 3];
    out.par_chunks_mut(3 << 14).zip(px.par_chunks(1 << 14)).for_each(|(o, p)| {
        for (o, p) in o.as_chunks_mut::<3>().0.iter_mut().zip(p) {
            o.copy_from_slice(&p[..3]);
        }
    });
    out
}

fn flatten_on(rgba: &[u8], bg: Color) -> Vec<u8> {
    let (px, _) = rgba.as_chunks::<4>();
    let mut out = vec![0u8; px.len() * 3];
    out.par_chunks_mut(3 << 14).zip(px.par_chunks(1 << 14)).for_each(|(o, p)| {
        for (o, p) in o.as_chunks_mut::<3>().0.iter_mut().zip(p) {
            let a = u32::from(p[3]);
            let mix = |c: u8, b: u8| ((u32::from(c) * a + u32::from(b) * (255 - a) + 127) / 255) as u8;
            o.copy_from_slice(&[mix(p[0], bg.r), mix(p[1], bg.g), mix(p[2], bg.b)]);
        }
    });
    out
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

/// Whether a full PNG of `doc` embeds its project: it has vector objects, more than one layer,
/// a hidden layer or a background color other than white (the image alone would lose them).
/// Vector mode alone does not count: it is a remembered setting, on for most documents of
/// someone who uses it, and an embedded copy of every layer would double their plain images.
pub fn keeps_project(doc: &Document) -> bool {
    let bg = doc.background();
    doc.layers().len() > 1 || doc.layers().iter().any(|l| !l.visible || !l.objects.is_empty()) || bg.color != Color::WHITE
}

/// A full PNG of `doc` (see [`SaveFormat::Png`]).
pub fn encode_full_png(doc: &Document) -> Result<Vec<u8>, IoError> {
    // The image and the project are encoded at the same time.
    let (mut png, project) =
        rayon::join(|| crate::png::encode(&doc.flatten(), PNG_LEVEL), || keeps_project(doc).then(|| ora::write_editable(doc)).transpose());
    if let Some(project) = project? {
        crate::png::embed_project(&mut png, &project);
    }
    Ok(png)
}

/// Saves the document in `format` (images are flattened).
pub fn save(doc: &Document, path: &Path, format: SaveFormat) -> Result<u64, IoError> {
    let bytes = match format {
        SaveFormat::Ora => ora::write(doc)?,
        SaveFormat::Png => encode_full_png(doc)?,
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

    fn objects() -> Vec<crate::vector::VectorShape> {
        use crate::geom::{Point, RectF};
        use crate::shapes::{ShapeKind, ShapePaint, ShapeStyle};
        use crate::text::{TextAlign, TextStyle};
        use crate::vector::{ShapeGeom, VectorShape};
        let paint = ShapePaint {
            outline: ShapeStyle::Textured(crate::brush::Texture::Crayon),
            fill: ShapeStyle::Solid,
            width: 3.5,
            outline_color: Color::rgba(1, 2, 3, 200),
            fill_color: Color::rgb(255, 0, 0),
            opacity: 0.75,
        };
        let style = TextStyle {
            family: "Sans & \"Serif\"".into(),
            size_pt: 12.5,
            bold: true,
            italic: false,
            underline: true,
            strikethrough: false,
            align: TextAlign::Center,
            color: Color::rgb(0, 0, 255),
            background: Some(Color::rgb(9, 9, 9)),
            opacity: 1.0,
        };
        let text_paint = ShapePaint::for_text(&style);
        let canvas = (40, 30);
        vec![
            VectorShape::new(
                ShapeGeom::Box { kind: ShapeKind::Star5, rect: RectF::from_xywh(2.25, 3.0, 10.0, 8.0), angle: 12.5, points: None },
                paint,
                canvas,
            ),
            VectorShape::new(
                ShapeGeom::Box {
                    kind: ShapeKind::Polygon,
                    rect: RectF::from_xywh(5.0, 5.0, 10.0, 10.0),
                    angle: 0.0,
                    points: Some(vec![(0.0, 0.0), (1.0, 0.5), (0.1, 1.0)]),
                },
                paint,
                canvas,
            ),
            VectorShape::new(ShapeGeom::Line { a: Point::new(0.5, 1.0), b: Point::new(30.0, 20.0) }, paint, canvas),
            VectorShape::new(
                ShapeGeom::Curve {
                    a: Point::new(1.0, 1.0),
                    b: Point::new(30.0, 2.0),
                    c1: Point::new(5.0, 20.0),
                    c2: Point::new(20.0, 25.0),
                },
                paint,
                canvas,
            ),
            VectorShape::new(
                ShapeGeom::Arrow { points: vec![Point::new(1.0, 1.0), Point::new(10.0, 1.0), Point::new(10.0, 9.0)] },
                paint,
                canvas,
            ),
            VectorShape::new(
                ShapeGeom::Text {
                    rect: RectF::from_xywh(4.0, 4.0, 30.0, 20.0),
                    angle: 0.0,
                    text: "a < b & c\r\n  two\tlines ".into(),
                    style: text_paint.text_style(&style),
                    auto_size: false,
                },
                text_paint,
                canvas,
            ),
        ]
    }

    #[test]
    fn full_png_keeps_layers_and_objects() {
        let dir = std::env::temp_dir().join(format!("bgpaint-full-png-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut doc = Document::new(40, 30).unwrap();
        doc.set_vector_mode(true);
        doc.edit_active(doc.bounds(), |p| raster::fill_region(p, PixelRect::new(0, 0, 4, 4), Color::rgb(0, 200, 0)));
        for o in objects() {
            doc.add_object(o);
        }
        doc.add_layer();
        doc.rename_layer(1, "Top".into());
        doc.set_layer_visible(1, false);
        let path = dir.join("v.png");
        save(&doc, &path, SaveFormat::Png).unwrap();

        // Any program sees the image as it looks.
        let bytes = std::fs::read(&path).unwrap();
        let other = image::load_from_memory(&bytes).unwrap().into_rgba8();
        let flat = doc.flatten();
        assert_eq!((other.width(), other.height()), (40, 30));
        assert_eq!(other.as_raw(), &raster::to_straight_rgba(&flat));
        assert_eq!(decode(&bytes, "png").unwrap().data(), flat.data());

        let opened = open(&path).unwrap();
        assert_eq!(opened.save_format, Some(SaveFormat::Png));
        let back = opened.document;
        assert!(back.vector_mode());
        assert_eq!(back.layers().len(), 2);
        assert_eq!(back.layers()[1].name, "Top");
        assert!(!back.layers()[1].visible);
        assert_eq!(raster::get_pixel(&back.layers()[0].pixels, 1, 1), Some(Color::rgb(0, 200, 0)));
        let (a, b) = (&doc.layers()[0].objects, &back.layers()[0].objects);
        assert_eq!(a.len(), b.len());
        for (a, b) in a.iter().zip(b) {
            assert_eq!(a.geom, b.geom);
            assert_eq!(a.paint, b.paint);
        }
        assert_eq!(back.flatten().data(), flat.data());

        // The optimized PNG is only the image.
        save(&doc, &path, SaveFormat::PngOptimized).unwrap();
        let opened = open(&path).unwrap();
        assert_eq!(opened.document.layers().len(), 1);
        assert!(opened.document.layers()[0].objects.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_plain_image_saves_as_a_plain_png() {
        let doc = Document::from_image(sample());
        assert!(!keeps_project(&doc));
        let png = encode_full_png(&doc).unwrap();
        assert!(crate::png::embedded_project(&png).is_none());
    }

    #[test]
    fn text_with_control_characters_keeps_the_project_readable() {
        let dir = std::env::temp_dir().join(format!("bgpaint-control-chars-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut doc = Document::new(40, 30).unwrap();
        doc.set_vector_mode(true);
        let mut text = objects().pop().unwrap();
        if let crate::vector::ShapeGeom::Text { text: t, .. } = &mut text.geom {
            *t = "soft\u{0B}break\u{0C}page\u{1B}".into();
        }
        doc.add_object(text);
        doc.add_layer();
        doc.rename_layer(1, "odd\u{0}name".into());
        let path = dir.join("c.png");
        save(&doc, &path, SaveFormat::Png).unwrap();
        let back = open(&path).unwrap().document;
        assert_eq!(back.layers().len(), 2);
        assert_eq!(back.layers()[1].name, "oddname");
        assert_eq!(back.layers()[0].objects.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_project_that_no_longer_matches_the_image_is_ignored() {
        let dir = std::env::temp_dir().join(format!("bgpaint-stale-project-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut doc = Document::new(40, 30).unwrap();
        doc.add_layer();
        let project = ora::write_editable(&doc).unwrap();
        // Another program resized the image but kept the chunk.
        let mut png = crate::png::encode(&sample(), PNG_LEVEL);
        crate::png::embed_project(&mut png, &project);
        let path = dir.join("s.png");
        std::fs::write(&path, &png).unwrap();
        let opened = open(&path).unwrap();
        // A plain "Save" must not silently replace the file and the project it still holds.
        assert_eq!(opened.save_format, None);
        let back = opened.document;
        assert_eq!((back.width(), back.height()), (8, 6));
        assert_eq!(back.layers().len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_damaged_project_is_not_replaced_by_a_plain_save() {
        let dir = std::env::temp_dir().join(format!("bgpaint-damaged-project-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut doc = Document::new(40, 30).unwrap();
        doc.add_layer();
        let mut png = encode_full_png(&doc).unwrap();
        // One byte of the project (the chunk after the fingerprint) goes bad.
        let second = png.windows(4).enumerate().filter(|(_, w)| *w == b"bgPJ").nth(1).unwrap().0;
        png[second + 4] ^= 0xFF;
        let path = dir.join("d.png");
        std::fs::write(&path, &png).unwrap();
        let opened = open(&path).unwrap();
        assert_eq!(opened.save_format, None);
        assert_eq!(opened.document.layers().len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
