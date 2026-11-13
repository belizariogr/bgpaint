//! OpenRaster (`.ora`) projects: a zip with `stack.xml` and one PNG per layer.
//! The background is stored as a regular bottom layer tagged with `bgpaint:background`, so other
//! editors (GIMP, Krita, MyPaint) show it as a normal layer.
//!
//! The editable variant ([`write_editable`], embedded in full PNGs) keeps vector objects as
//! `<bgpaint:object>` elements inside their `<layer>`, the layer images holding only the pixels
//! below them, and records the vector mode (when there are objects) on
//! `<image bgpaint:vector="true">`.

use crate::color::Color;
use crate::document::{Background, Document};
use crate::io::{IoError, PNG_LEVEL, decode};
use crate::png;
use crate::raster;
use crate::vector_xml::{self, escape};
use rayon::prelude::*;
use std::io::{Cursor, Read, Write};

const MIMETYPE: &str = "image/openraster";

pub fn is_ora(bytes: &[u8]) -> bool {
    // A zip whose first entry is an uncompressed "mimetype" file.
    bytes.len() > 54 && bytes.starts_with(b"PK\x03\x04") && bytes[30..38] == *b"mimetype" && bytes[38..54] == *b"image/openraster"
}

fn zip_err(e: zip::result::ZipError) -> IoError {
    IoError::Encode(e.to_string())
}

/// An OpenRaster project with vector objects drawn into their layers (what other editors read).
pub fn write(doc: &Document) -> Result<Vec<u8>, IoError> {
    write_with(doc, false)
}

/// A project that keeps vector objects editable and the vector mode, without the merged image
/// and thumbnail (it is embedded in a PNG that already is that image).
pub fn write_editable(doc: &Document) -> Result<Vec<u8>, IoError> {
    write_with(doc, true)
}

fn write_with(doc: &Document, editable: bool) -> Result<Vec<u8>, IoError> {
    // Every image is encoded at once, on all cores; the zip is written afterwards.
    enum Part {
        Layer(usize),
        Background,
        Merged,
    }
    let mut parts: Vec<Part> = (0..doc.layers().len()).map(Part::Layer).collect();
    parts.push(Part::Background);
    if !editable {
        parts.push(Part::Merged);
    }
    let bg = doc.background();
    let encoded: Vec<Vec<Vec<u8>>> = parts
        .par_iter()
        .map(|part| match part {
            // Vector objects are saved drawn into their layer (the format only stores images),
            // unless they are kept as objects.
            Part::Layer(i) if editable => vec![png::encode(&doc.layers()[*i].pixels, PNG_LEVEL)],
            Part::Layer(i) => vec![png::encode(&doc.layer_image(*i), PNG_LEVEL)],
            // Only the color is read back from an editable project (see `read`): one pixel will do.
            Part::Background if editable => vec![png::encode_solid(1, 1, bg.color.premultiplied(), PNG_LEVEL)],
            Part::Background => vec![png::encode_solid(doc.width(), doc.height(), bg.color.premultiplied(), PNG_LEVEL)],
            Part::Merged => {
                let merged = doc.flatten();
                let (tw, th) = fit(merged.width(), merged.height(), 256);
                let thumb = raster::thumbnail(&merged, tw, th);
                vec![png::encode(&merged, PNG_LEVEL), png::encode(&thumb, PNG_LEVEL)]
            }
        })
        .collect();

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("mimetype", stored).map_err(zip_err)?;
    zip.write_all(MIMETYPE.as_bytes())?;

    let mut stack = String::new();
    for (i, layer) in doc.layers().iter().enumerate().rev() {
        let src = format!("data/layer{i}.png");
        zip.start_file(src.as_str(), stored).map_err(zip_err)?;
        zip.write_all(&encoded[i][0])?;
        stack.push_str(&format!(
            "  <layer name=\"{}\" src=\"{src}\" x=\"0\" y=\"0\" opacity=\"1.0\" visibility=\"{}\"{}",
            escape(&layer.name),
            if layer.visible { "visible" } else { "hidden" },
            if i == doc.active_index() { " selected=\"true\"" } else { "" },
        ));
        if editable && layer.objects.iter().any(vector_xml::writable) {
            stack.push_str(">\n");
            for o in layer.objects.iter().filter(|o| vector_xml::writable(o)) {
                vector_xml::write_object(&mut stack, o, "   ");
            }
            stack.push_str("  </layer>\n");
        } else {
            stack.push_str("/>\n");
        }
    }
    let n = doc.layers().len();
    zip.start_file("data/background.png", stored).map_err(zip_err)?;
    zip.write_all(&encoded[n][0])?;
    stack.push_str(&format!(
        "  <layer name=\"Background\" src=\"data/background.png\" x=\"0\" y=\"0\" opacity=\"1.0\" visibility=\"{}\" bgpaint:background=\"{}\"/>\n",
        if bg.visible { "visible" } else { "hidden" },
        bg.color.to_hex_string(),
    ));
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<image version=\"0.0.3\" w=\"{}\" h=\"{}\" xres=\"96\" yres=\"96\" xmlns:bgpaint=\"https://github.com/belizariogr/bgpaint\"{}>\n <stack>\n{stack} </stack>\n</image>\n",
        doc.width(),
        doc.height(),
        // Only a document with objects opens in vector mode: on its own the mode is a
        // remembered setting, not part of the file.
        if editable && doc.vector_mode() && doc.layers().iter().any(|l| l.objects.iter().any(vector_xml::writable)) {
            " bgpaint:vector=\"true\""
        } else {
            ""
        },
    );
    zip.start_file("stack.xml", deflated).map_err(zip_err)?;
    zip.write_all(xml.as_bytes())?;

    if !editable {
        zip.start_file("mergedimage.png", stored).map_err(zip_err)?;
        zip.write_all(&encoded[n + 1][0])?;
        zip.start_file("Thumbnails/thumbnail.png", stored).map_err(zip_err)?;
        zip.write_all(&encoded[n + 1][1])?;
    }
    Ok(zip.finish().map_err(zip_err)?.into_inner())
}

fn fit(w: u32, h: u32, max: u32) -> (u32, u32) {
    let s = (max as f32 / w.max(h) as f32).min(1.0);
    (((w as f32 * s).round() as u32).max(1), ((h as f32 * s).round() as u32).max(1))
}

/// Largest `stack.xml` read (vector objects included).
const MAX_XML: u64 = 256 << 20;
/// Largest layer image read: more than any image within the size limits takes as a PNG.
const MAX_LAYER_FILE: u64 = 5 * raster::MAX_AREA;

/// A zip entry whose declared size is within `limit`, read no further than that (a damaged or
/// crafted archive could inflate far beyond what it declares).
fn read_limited<R: Read>(entry: zip::read::ZipFile<'_, R>, limit: u64) -> Result<impl Read, IoError> {
    if entry.size() > limit {
        return Err(IoError::Decode("project entry too large".into()));
    }
    Ok(entry.take(limit))
}

pub fn read(bytes: &[u8]) -> Result<Document, IoError> {
    let bad = |e: &dyn std::fmt::Display| IoError::Decode(e.to_string());
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| bad(&e))?;
    let mut xml = String::new();
    read_limited(zip.by_name("stack.xml").map_err(|e| bad(&e))?, MAX_XML)?.read_to_string(&mut xml)?;
    let tree = roxmltree::Document::parse(&xml).map_err(|e| bad(&e))?;
    let image = tree.root_element();
    let attr_u32 = |n: roxmltree::Node, a: &str| n.attribute(a).and_then(|v| v.trim().parse::<u32>().ok());
    let width = attr_u32(image, "w").ok_or_else(|| IoError::Decode("missing image width".into()))?;
    let height = attr_u32(image, "h").ok_or_else(|| IoError::Decode("missing image height".into()))?;
    raster::try_pixmap(width, height)?;

    struct Entry {
        name: String,
        visible: bool,
        x: i32,
        y: i32,
        opacity: f32,
        data: Vec<u8>,
    }
    let mut entries = Vec::new();
    let mut objects = Vec::new();
    // The active layer, counted from the top.
    let mut selected = None;
    let mut background = None;
    // Flatten nested stacks; document order is top to bottom.
    for node in image.descendants().filter(|n| n.has_tag_name("layer")) {
        let Some(src) = node.attribute("src") else { continue };
        let visible = node.attribute("visibility") != Some("hidden");
        let bg_attr = node.attributes().find(|a| a.name() == "background" && vector_xml::is_bgpaint(a.namespace()));
        if let Some(bg) = bg_attr {
            background = Some(Background { color: Color::parse_hex(bg.value()).unwrap_or(Color::WHITE), visible });
            continue;
        }
        let mut data = Vec::new();
        read_limited(zip.by_name(src).map_err(|e| bad(&e))?, MAX_LAYER_FILE)?.read_to_end(&mut data)?;
        if node.attribute("selected") == Some("true") {
            selected = Some(entries.len());
        }
        entries.push(Entry {
            name: node.attribute("name").unwrap_or("").to_string(),
            visible,
            x: node.attribute("x").and_then(|v| v.parse::<i32>().ok()).unwrap_or(0),
            y: node.attribute("y").and_then(|v| v.parse::<i32>().ok()).unwrap_or(0),
            opacity: node.attribute("opacity").and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0).clamp(0.0, 1.0),
            data,
        });
        objects.push(node.children().filter(vector_xml::is_object).collect::<Vec<_>>());
    }
    // Bottom first, like the layers.
    objects.reverse();
    // Decoded on all cores at once.
    let mut layers = entries
        .into_par_iter()
        .map(|e| {
            let pixels = decode(&e.data, "png")?;
            // A layer that already covers the canvas as it is (the usual case) is used directly.
            let placed = if (e.x, e.y, pixels.width(), pixels.height()) == (0, 0, width, height) && e.opacity == 1.0 {
                pixels
            } else {
                let mut placed = raster::pixmap(width, height);
                raster::draw_over(&mut placed, &pixels, e.x, e.y, e.opacity);
                placed
            };
            Ok((e.name, placed, e.visible))
        })
        .collect::<Result<Vec<_>, IoError>>()?;
    layers.reverse();
    let background = background.unwrap_or_else(|| {
        // Foreign file: show a white background only if the image is fully opaque.
        let mut probe = raster::pixmap(width, height);
        for (_, p, v) in &layers {
            if *v {
                raster::draw_over(&mut probe, p, 0, 0, 1.0);
            }
        }
        Background { color: Color::WHITE, visible: !raster::has_transparency(&probe) }
    });
    let mut doc = Document::from_layers(width, height, layers, background);
    if let Some(top) = selected {
        doc.set_active(doc.layers().len() - 1 - top);
    }
    let vector = image.attributes().any(|a| a.name() == "vector" && vector_xml::is_bgpaint(a.namespace()) && a.value() == "true");
    // Rendered on all cores. An object that cannot be read (damaged, or from a newer version)
    // fails the whole project: the layer images hold only the pixels below the objects, so
    // dropping one would lose what it shows (a full PNG then opens as its image).
    let shapes = objects
        .par_iter()
        .map(|nodes| {
            nodes
                .par_iter()
                .map(|node| {
                    vector_xml::read_object(*node, (width, height)).ok_or_else(|| IoError::Decode("unreadable vector object".into()))
                })
                .collect::<Result<Vec<_>, IoError>>()
        })
        .collect::<Result<Vec<_>, IoError>>()?;
    if vector || shapes.iter().any(|s| !s.is_empty()) {
        doc.set_vector_mode(true);
        for (i, shapes) in shapes.into_iter().enumerate() {
            let Some(id) = doc.layers().get(i).map(|l| l.id) else { break };
            for (n, o) in shapes.into_iter().enumerate() {
                doc.insert_object(id, n, o);
            }
        }
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::PixelRect;

    #[test]
    fn roundtrip() {
        let mut doc = Document::new(16, 8).unwrap();
        doc.edit_active(doc.bounds(), |p| raster::fill_region(p, PixelRect::new(0, 0, 4, 4), Color::rgb(1, 2, 3)));
        doc.add_layer();
        doc.rename_layer(1, "Top & <more>".into());
        doc.edit_active(doc.bounds(), |p| raster::fill_region(p, PixelRect::new(8, 0, 12, 4), Color::rgb(9, 9, 9)));
        doc.set_layer_visible(1, false);
        doc.set_background(Background { color: Color::rgb(10, 20, 30), visible: false });
        doc.set_active(0);
        let bytes = write(&doc).unwrap();
        assert!(is_ora(&bytes));
        let back = read(&bytes).unwrap();
        assert_eq!((back.width(), back.height()), (16, 8));
        assert_eq!(back.layers().len(), 2);
        assert_eq!(back.active_index(), 0);
        assert_eq!(back.layers()[1].name, "Top & <more>");
        assert!(!back.layers()[1].visible);
        assert_eq!(back.background(), Background { color: Color::rgb(10, 20, 30), visible: false });
        assert_eq!(raster::get_pixel(&back.layers()[0].pixels, 1, 1), Some(Color::rgb(1, 2, 3)));
        assert_eq!(raster::get_pixel(&back.layers()[1].pixels, 9, 1), Some(Color::rgb(9, 9, 9)));
    }

    #[test]
    fn an_unreadable_object_fails_the_whole_project() {
        let project = |kind: &str| {
            let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            zip.start_file("stack.xml", stored).unwrap();
            let object = format!(
                "<bgpaint:object type=\"box\" kind=\"{kind}\" rect=\"0 0 2 2\" outline=\"solid\" fill=\"none\" width=\"1\" outline-color=\"#000000FF\" fill-color=\"#FFFFFFFF\" opacity=\"1\"/>"
            );
            let xml = format!(
                "<image w=\"4\" h=\"4\" xmlns:bgpaint=\"https://github.com/belizariogr/bgpaint\"><stack><layer src=\"l.png\">{object}</layer></stack></image>"
            );
            zip.write_all(xml.as_bytes()).unwrap();
            zip.start_file("l.png", stored).unwrap();
            zip.write_all(&png::encode(&raster::pixmap(4, 4), PNG_LEVEL)).unwrap();
            zip.finish().unwrap().into_inner()
        };
        let doc = read(&project("rectangle")).unwrap();
        assert!(doc.vector_mode());
        assert_eq!(doc.layers()[0].objects.len(), 1);
        // A kind from a newer version: the layers alone would lose what the object shows.
        assert!(read(&project("hexadecagon")).is_err());
    }
}
