//! Vector objects written as XML elements (inside a project's `stack.xml`) and read back.
//!
//! Every object is one `<bgpaint:object>` element: its geometry and paint are attributes, and a
//! text box's text is the element's content. Numbers are written in Rust's shortest exact form,
//! so a saved object reads back identical.

use crate::brush::Texture;
use crate::color::Color;
use crate::geom::{Point, RectF};
use crate::raster;
use crate::shapes::{ShapeKind, ShapePaint, ShapeStyle};
use crate::text::{TextAlign, TextStyle};
use crate::vector::{ShapeGeom, VectorShape};
use std::fmt::Write;

/// Escapes text for an attribute value or element content (line breaks survive both).
/// Characters XML cannot hold at all (other control characters, U+FFFE, U+FFFF) are left out:
/// one of them would make the whole project unreadable.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\r' => out.push_str("&#13;"),
            '\n' => out.push_str("&#10;"),
            '\t' => out.push_str("&#9;"),
            '\0'..='\u{1F}' | '\u{FFFE}' | '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

fn color(c: Color) -> String {
    format!("#{:02X}{:02X}{:02X}{:02X}", c.r, c.g, c.b, c.a)
}

/// The name of a shape in files. Part of the file format: never change one.
fn kind_name(k: ShapeKind) -> &'static str {
    match k {
        ShapeKind::Line => "line",
        ShapeKind::Curve => "curve",
        ShapeKind::CurvedArrow => "curved-arrow",
        ShapeKind::Oval => "oval",
        ShapeKind::Rectangle => "rectangle",
        ShapeKind::RoundedRectangle => "rounded-rectangle",
        ShapeKind::Polygon => "polygon",
        ShapeKind::Triangle => "triangle",
        ShapeKind::RightTriangle => "right-triangle",
        ShapeKind::Diamond => "diamond",
        ShapeKind::Pentagon => "pentagon",
        ShapeKind::Hexagon => "hexagon",
        ShapeKind::RightArrow => "right-arrow",
        ShapeKind::LeftArrow => "left-arrow",
        ShapeKind::UpArrow => "up-arrow",
        ShapeKind::DownArrow => "down-arrow",
        ShapeKind::Star4 => "star4",
        ShapeKind::Star5 => "star5",
        ShapeKind::Star6 => "star6",
        ShapeKind::RoundedRectCallout => "rounded-rect-callout",
        ShapeKind::OvalCallout => "oval-callout",
        ShapeKind::CloudCallout => "cloud-callout",
        ShapeKind::Heart => "heart",
        ShapeKind::Lightning => "lightning",
        ShapeKind::Moon => "moon",
    }
}

fn parse_kind(s: &str) -> Option<ShapeKind> {
    ShapeKind::ALL.into_iter().find(|k| kind_name(*k) == s)
}

/// Every texture, for reading names back: a texture added to [`Texture`] goes here too, or the
/// files that use it cannot be read (see [`texture_name`]).
const TEXTURES: [Texture; 6] =
    [Texture::Solid, Texture::Crayon, Texture::Marker, Texture::Oil, Texture::NaturalPencil, Texture::Watercolor];

/// The name of a texture in files. Part of the file format: never change one.
fn texture_name(t: Texture) -> &'static str {
    match t {
        Texture::Solid => "plain",
        Texture::Crayon => "crayon",
        Texture::Marker => "marker",
        Texture::Oil => "oil",
        Texture::NaturalPencil => "natural-pencil",
        Texture::Watercolor => "watercolor",
    }
}

fn style_name(s: ShapeStyle) -> &'static str {
    match s {
        ShapeStyle::None => "none",
        ShapeStyle::Solid => "solid",
        ShapeStyle::Textured(t) => texture_name(t),
    }
}

fn parse_style(s: &str) -> Option<ShapeStyle> {
    [ShapeStyle::None, ShapeStyle::Solid].into_iter().chain(TEXTURES.map(ShapeStyle::Textured)).find(|style| style_name(*style) == s)
}

fn align_name(a: TextAlign) -> &'static str {
    match a {
        TextAlign::Left => "left",
        TextAlign::Center => "center",
        TextAlign::Right => "right",
    }
}

fn parse_align(s: &str) -> Option<TextAlign> {
    [TextAlign::Left, TextAlign::Center, TextAlign::Right].into_iter().find(|a| align_name(*a) == s)
}

fn points(pts: impl IntoIterator<Item = (f32, f32)>) -> String {
    pts.into_iter().map(|(x, y)| format!("{x},{y}")).collect::<Vec<_>>().join(" ")
}

fn rect(r: &RectF) -> String {
    format!("{} {} {} {}", r.min.x, r.min.y, r.max.x, r.max.y)
}

/// Whether [`read_object`] can read `o` back once written: every number is finite and point
/// lists are long enough. Anything else draws nothing, and writing it would make the whole
/// project unreadable, so it is left out.
pub fn writable(o: &VectorShape) -> bool {
    let p = &o.paint;
    let fin = |v: &[f32]| v.iter().all(|v| v.is_finite());
    let pt = |p: &Point| p.x.is_finite() && p.y.is_finite();
    let rect_ok = |r: &RectF| pt(&r.min) && pt(&r.max);
    fin(&[p.width, p.opacity])
        && match &o.geom {
            ShapeGeom::Box { rect, angle, points, .. } => {
                rect_ok(rect)
                    && angle.is_finite()
                    && points.as_ref().is_none_or(|pts| pts.len() >= 3 && pts.iter().all(|&(u, v)| u.is_finite() && v.is_finite()))
            }
            ShapeGeom::Line { a, b } => pt(a) && pt(b),
            ShapeGeom::Curve { a, b, c1, c2 } => [a, b, c1, c2].into_iter().all(pt),
            ShapeGeom::Arrow { points } => points.len() >= 2 && points.iter().all(pt),
            ShapeGeom::Text { rect, angle, style, .. } => rect_ok(rect) && fin(&[*angle, style.size_pt]),
        }
}

/// The element of one object, indented by `indent`.
pub fn write_object(out: &mut String, o: &VectorShape, indent: &str) {
    let p = &o.paint;
    let mut attrs = String::new();
    let mut content = None;
    match &o.geom {
        ShapeGeom::Box { kind, rect: r, angle, points: pts } => {
            let _ = write!(attrs, " type=\"box\" kind=\"{}\" rect=\"{}\" angle=\"{angle}\"", kind_name(*kind), rect(r));
            if let Some(pts) = pts {
                let _ = write!(attrs, " points=\"{}\"", points(pts.iter().copied()));
            }
        }
        ShapeGeom::Line { a, b } => {
            let _ = write!(attrs, " type=\"line\" points=\"{}\"", points([(a.x, a.y), (b.x, b.y)]));
        }
        ShapeGeom::Curve { a, b, c1, c2 } => {
            let _ = write!(attrs, " type=\"curve\" points=\"{}\"", points([a, c1, c2, b].map(|p| (p.x, p.y))));
        }
        ShapeGeom::Arrow { points: pts } => {
            let _ = write!(attrs, " type=\"arrow\" points=\"{}\"", points(pts.iter().map(|p| (p.x, p.y))));
        }
        ShapeGeom::Text { rect: r, angle, text, style, auto_size } => {
            let _ = write!(
                attrs,
                " type=\"text\" rect=\"{}\" angle=\"{angle}\" auto-size=\"{auto_size}\" font=\"{}\" size=\"{}\" bold=\"{}\" italic=\"{}\" underline=\"{}\" strikethrough=\"{}\" align=\"{}\"",
                rect(r),
                escape(&style.family),
                style.size_pt,
                style.bold,
                style.italic,
                style.underline,
                style.strikethrough,
                align_name(style.align),
            );
            content = Some(escape(text));
        }
    }
    let _ = write!(
        attrs,
        " outline=\"{}\" fill=\"{}\" width=\"{}\" outline-color=\"{}\" fill-color=\"{}\" opacity=\"{}\"",
        style_name(p.outline),
        style_name(p.fill),
        p.width,
        color(p.outline_color),
        color(p.fill_color),
        p.opacity,
    );
    match content {
        Some(text) => {
            let _ = writeln!(out, "{indent}<bgpaint:object{attrs}>{text}</bgpaint:object>");
        }
        None => {
            let _ = writeln!(out, "{indent}<bgpaint:object{attrs}/>");
        }
    }
}

fn parse_points(s: &str) -> Option<Vec<(f32, f32)>> {
    s.split_whitespace()
        .map(|pair| {
            let (x, y) = pair.split_once(',')?;
            let (x, y): (f32, f32) = (x.parse().ok()?, y.parse().ok()?);
            (x.is_finite() && y.is_finite()).then_some((x, y))
        })
        .collect()
}

fn parse_rect(s: &str) -> Option<RectF> {
    let v: Vec<f32> = s.split_whitespace().map(str::parse).collect::<Result<_, _>>().ok()?;
    let [x0, y0, x1, y1] = v[..] else { return None };
    if !v.iter().all(|v| v.is_finite()) {
        return None;
    }
    Some(RectF { min: Point::new(x0, y0), max: Point::new(x1, y1) })
}

/// Reads an object element; `None` when it is damaged or of an unknown kind. `canvas` is the
/// document size the object is drawn for.
pub fn read_object(node: roxmltree::Node, canvas: (u32, u32)) -> Option<VectorShape> {
    let a = |name: &str| node.attribute(name);
    let num = |name: &str| a(name).and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite());
    let flag = |name: &str| a(name) == Some("true");
    let pts = || parse_points(a("points")?);
    let paint = ShapePaint {
        outline: parse_style(a("outline")?)?,
        fill: parse_style(a("fill")?)?,
        width: num("width")?,
        outline_color: Color::parse_hex(a("outline-color")?)?,
        fill_color: Color::parse_hex(a("fill-color")?)?,
        opacity: num("opacity")?.clamp(0.0, 1.0),
    };
    let point = |(x, y): (f32, f32)| Point::new(x, y);
    let geom = match a("type")? {
        "box" => ShapeGeom::Box {
            kind: parse_kind(a("kind")?)?,
            rect: parse_rect(a("rect")?)?,
            angle: num("angle").unwrap_or(0.0),
            points: match a("points") {
                Some(_) => Some(pts().filter(|p| p.len() >= 3)?),
                None => None,
            },
        },
        "line" => {
            let p = pts()?;
            let [a, b] = p[..] else { return None };
            ShapeGeom::Line { a: point(a), b: point(b) }
        }
        "curve" => {
            let p = pts()?;
            let [a, c1, c2, b] = p[..] else { return None };
            ShapeGeom::Curve { a: point(a), b: point(b), c1: point(c1), c2: point(c2) }
        }
        "arrow" => {
            let p = pts().filter(|p| p.len() >= 2)?;
            ShapeGeom::Arrow { points: p.into_iter().map(point).collect() }
        }
        "text" => {
            let base = TextStyle {
                family: a("font")?.to_string(),
                // An absurd font size from a damaged file is brought within the largest image
                // rather than refused (text::render bounds the box it draws into).
                size_pt: num("size")?.clamp(1.0, raster::MAX_SIDE as f32 * 72.0 / 96.0),
                bold: flag("bold"),
                italic: flag("italic"),
                underline: flag("underline"),
                strikethrough: flag("strikethrough"),
                align: parse_align(a("align")?)?,
                // Replaced by the paint's below.
                color: paint.outline_color,
                background: None,
                opacity: paint.opacity,
            };
            ShapeGeom::Text {
                rect: parse_rect(a("rect")?)?,
                angle: num("angle").unwrap_or(0.0),
                text: node.text().unwrap_or("").to_string(),
                // The colors and opacity always come from the paint (see ShapePaint::text_style).
                style: paint.text_style(&base),
                auto_size: flag("auto-size"),
            }
        }
        _ => return None,
    };
    Some(VectorShape::new(geom, paint, canvas))
}

/// Whether `namespace` is BGPaint's (elements and attributes other editors ignore).
pub fn is_bgpaint(namespace: Option<&str>) -> bool {
    namespace.is_some_and(|ns| ns.contains("bgpaint"))
}

/// Whether `node` is a BGPaint object element.
pub fn is_object(node: &roxmltree::Node) -> bool {
    node.is_element() && node.tag_name().name() == "object" && is_bgpaint(node.tag_name().namespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_offered_reads_back() {
        // A texture missing from TEXTURES would make every project that uses it unreadable.
        for s in ShapeStyle::OPTIONS.into_iter().chain(TEXTURES.map(ShapeStyle::Textured)) {
            assert_eq!(parse_style(style_name(s)), Some(s), "{s:?}");
        }
    }
}
