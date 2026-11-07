//! OpenRaster (`.ora`) projects: a zip with `stack.xml` and one PNG per layer.
//! The background is stored as a regular bottom layer tagged with `bgpaint:background`, so other
//! editors (GIMP, Krita, MyPaint) show it as a normal layer.

use crate::color::Color;
use crate::document::{Background, Document};
use crate::io::{IoError, SaveFormat, decode, encode};
use crate::raster;
use std::io::{Cursor, Read, Write};

const MIMETYPE: &str = "image/openraster";

pub fn is_ora(bytes: &[u8]) -> bool {
    // A zip whose first entry is an uncompressed "mimetype" file.
    bytes.len() > 54 && bytes.starts_with(b"PK\x03\x04") && bytes[30..38] == *b"mimetype" && bytes[38..54] == *b"image/openraster"
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn zip_err(e: zip::result::ZipError) -> IoError {
    IoError::Encode(e.to_string())
}

pub fn write(doc: &Document) -> Result<Vec<u8>, IoError> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("mimetype", stored).map_err(zip_err)?;
    zip.write_all(MIMETYPE.as_bytes())?;

    let mut stack = String::new();
    for (i, layer) in doc.layers().iter().enumerate().rev() {
        let src = format!("data/layer{i}.png");
        zip.start_file(src.as_str(), stored).map_err(zip_err)?;
        // Vector objects are saved drawn into their layer: the format only stores images.
        zip.write_all(&encode(&doc.layer_image(i), SaveFormat::Png)?)?;
        stack.push_str(&format!(
            "  <layer name=\"{}\" src=\"{src}\" x=\"0\" y=\"0\" opacity=\"1.0\" visibility=\"{}\"{}/>\n",
            escape(&layer.name),
            if layer.visible { "visible" } else { "hidden" },
            if i == doc.active_index() { " selected=\"true\"" } else { "" },
        ));
    }
    let bg = doc.background();
    let mut bg_pixels = raster::pixmap(doc.width(), doc.height());
    bg_pixels.fill(bg.color.to_skia());
    zip.start_file("data/background.png", stored).map_err(zip_err)?;
    zip.write_all(&encode(&bg_pixels, SaveFormat::Png)?)?;
    stack.push_str(&format!(
        "  <layer name=\"Background\" src=\"data/background.png\" x=\"0\" y=\"0\" opacity=\"1.0\" visibility=\"{}\" bgpaint:background=\"{}\"/>\n",
        if bg.visible { "visible" } else { "hidden" },
        bg.color.to_hex_string(),
    ));
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<image version=\"0.0.3\" w=\"{}\" h=\"{}\" xres=\"96\" yres=\"96\" xmlns:bgpaint=\"https://github.com/belizariogr/bgpaint\">\n <stack>\n{stack} </stack>\n</image>\n",
        doc.width(),
        doc.height()
    );
    zip.start_file("stack.xml", deflated).map_err(zip_err)?;
    zip.write_all(xml.as_bytes())?;

    let merged = doc.flatten();
    zip.start_file("mergedimage.png", stored).map_err(zip_err)?;
    zip.write_all(&encode(&merged, SaveFormat::Png)?)?;
    let (tw, th) = fit(merged.width(), merged.height(), 256);
    zip.start_file("Thumbnails/thumbnail.png", stored).map_err(zip_err)?;
    zip.write_all(&encode(&raster::resize(&merged, tw, th), SaveFormat::Png)?)?;
    Ok(zip.finish().map_err(zip_err)?.into_inner())
}

fn fit(w: u32, h: u32, max: u32) -> (u32, u32) {
    let s = (max as f32 / w.max(h) as f32).min(1.0);
    (((w as f32 * s).round() as u32).max(1), ((h as f32 * s).round() as u32).max(1))
}

pub fn read(bytes: &[u8]) -> Result<Document, IoError> {
    let bad = |e: &dyn std::fmt::Display| IoError::Decode(e.to_string());
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| bad(&e))?;
    let mut xml = String::new();
    zip.by_name("stack.xml").map_err(|e| bad(&e))?.read_to_string(&mut xml)?;
    let tree = roxmltree::Document::parse(&xml).map_err(|e| bad(&e))?;
    let image = tree.root_element();
    let attr_u32 = |n: roxmltree::Node, a: &str| n.attribute(a).and_then(|v| v.trim().parse::<u32>().ok());
    let width = attr_u32(image, "w").ok_or_else(|| IoError::Decode("missing image width".into()))?;
    let height = attr_u32(image, "h").ok_or_else(|| IoError::Decode("missing image height".into()))?;
    raster::try_pixmap(width, height)?;

    let mut layers = Vec::new();
    let mut background = None;
    // Flatten nested stacks; document order is top to bottom.
    for node in image.descendants().filter(|n| n.has_tag_name("layer")) {
        let Some(src) = node.attribute("src") else { continue };
        let visible = node.attribute("visibility") != Some("hidden");
        let bg_attr = node.attributes().find(|a| a.name() == "background" && a.namespace().is_some_and(|ns| ns.contains("bgpaint")));
        if let Some(bg) = bg_attr {
            background = Some(Background { color: Color::parse_hex(bg.value()).unwrap_or(Color::WHITE), visible });
            continue;
        }
        let mut data = Vec::new();
        zip.by_name(src).map_err(|e| bad(&e))?.read_to_end(&mut data)?;
        let pixels = decode(&data, "png")?;
        let x = node.attribute("x").and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
        let y = node.attribute("y").and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
        let opacity = node.attribute("opacity").and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
        let mut placed = raster::pixmap(width, height);
        raster::draw_over(&mut placed, &pixels, x, y, opacity.clamp(0.0, 1.0));
        let name = node.attribute("name").unwrap_or("").to_string();
        layers.push((name, placed, visible));
    }
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
    Ok(Document::from_layers(width, height, layers, background))
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
        let bytes = write(&doc).unwrap();
        assert!(is_ora(&bytes));
        let back = read(&bytes).unwrap();
        assert_eq!((back.width(), back.height()), (16, 8));
        assert_eq!(back.layers().len(), 2);
        assert_eq!(back.layers()[1].name, "Top & <more>");
        assert!(!back.layers()[1].visible);
        assert_eq!(back.background(), Background { color: Color::rgb(10, 20, 30), visible: false });
        assert_eq!(raster::get_pixel(&back.layers()[0].pixels, 1, 1), Some(Color::rgb(1, 2, 3)));
        assert_eq!(raster::get_pixel(&back.layers()[1].pixels, 9, 1), Some(Color::rgb(9, 9, 9)));
    }
}
