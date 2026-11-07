//! Geometry and rasterization of Paint's shapes.

use crate::brush::Texture;
use crate::color::Color;
use crate::geom::{PixelRect, Point, RectF};
use crate::raster;
use tiny_skia::{FillRule, LineCap, LineJoin, Mask, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShapeKind {
    Line,
    Curve,
    Oval,
    Rectangle,
    RoundedRectangle,
    Polygon,
    Triangle,
    RightTriangle,
    Diamond,
    Pentagon,
    Hexagon,
    RightArrow,
    LeftArrow,
    UpArrow,
    DownArrow,
    Star4,
    Star5,
    Star6,
    RoundedRectCallout,
    OvalCallout,
    CloudCallout,
    Heart,
    Lightning,
}

impl ShapeKind {
    /// All shapes in the order of Paint's gallery.
    pub const ALL: [ShapeKind; 23] = [
        ShapeKind::Line,
        ShapeKind::Curve,
        ShapeKind::Oval,
        ShapeKind::Rectangle,
        ShapeKind::RoundedRectangle,
        ShapeKind::Polygon,
        ShapeKind::Triangle,
        ShapeKind::RightTriangle,
        ShapeKind::Diamond,
        ShapeKind::Pentagon,
        ShapeKind::Hexagon,
        ShapeKind::RightArrow,
        ShapeKind::LeftArrow,
        ShapeKind::UpArrow,
        ShapeKind::DownArrow,
        ShapeKind::Star4,
        ShapeKind::Star5,
        ShapeKind::Star6,
        ShapeKind::RoundedRectCallout,
        ShapeKind::OvalCallout,
        ShapeKind::CloudCallout,
        ShapeKind::Heart,
        ShapeKind::Lightning,
    ];

    /// Shapes defined by points rather than a bounding box.
    pub fn is_open(self) -> bool {
        matches!(self, ShapeKind::Line | ShapeKind::Curve)
    }

    pub fn is_point_based(self) -> bool {
        matches!(self, ShapeKind::Line | ShapeKind::Curve | ShapeKind::Polygon)
    }
}

/// Outline or fill appearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShapeStyle {
    None,
    Solid,
    Textured(Texture),
}

impl ShapeStyle {
    /// Options of the outline and fill menus, in Paint's order.
    pub const OPTIONS: [ShapeStyle; 7] = [
        ShapeStyle::None,
        ShapeStyle::Solid,
        ShapeStyle::Textured(Texture::Crayon),
        ShapeStyle::Textured(Texture::Marker),
        ShapeStyle::Textured(Texture::Oil),
        ShapeStyle::Textured(Texture::NaturalPencil),
        ShapeStyle::Textured(Texture::Watercolor),
    ];
}

/// Everything needed to draw a shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapePaint {
    pub outline: ShapeStyle,
    pub fill: ShapeStyle,
    pub width: f32,
    pub outline_color: Color,
    pub fill_color: Color,
}

fn poly(points: &[(f32, f32)], r: &RectF) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for (i, &(u, v)) in points.iter().enumerate() {
        let (x, y) = (r.min.x + u * r.width(), r.min.y + v * r.height());
        if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
    }
    pb.close();
    pb.finish()
}

/// Regular star/polygon points normalized to the unit box.
fn radial(n: usize, inner: Option<f32>) -> Vec<(f32, f32)> {
    let count = if inner.is_some() { n * 2 } else { n };
    let mut pts: Vec<(f32, f32)> = (0..count)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / count as f32;
            let rad = match inner {
                Some(k) if i % 2 == 1 => k,
                _ => 1.0,
            };
            (a.cos() * rad, a.sin() * rad)
        })
        .collect();
    normalize(&mut pts);
    pts
}

fn normalize(pts: &mut [(f32, f32)]) {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &(x, y) in pts.iter() {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    let (w, h) = ((x1 - x0).max(1e-6), (y1 - y0).max(1e-6));
    for p in pts.iter_mut() {
        *p = ((p.0 - x0) / w, (p.1 - y0) / h);
    }
}

fn rounded_rect(pb: &mut PathBuilder, x: f32, y: f32, w: f32, h: f32, r: f32, tail: Option<[(f32, f32); 3]>) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    if let Some([a, tip, b]) = tail {
        pb.line_to(a.0, a.1);
        pb.line_to(tip.0, tip.1);
        pb.line_to(b.0, b.1);
    }
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
}

fn ellipse_points(cx: f32, cy: f32, rx: f32, ry: f32, n: usize) -> Vec<(f32, f32)> {
    (0..n)
        .map(|i| {
            let a = i as f32 * std::f32::consts::TAU / n as f32;
            (cx + rx * a.cos(), cy + ry * a.sin())
        })
        .collect()
}

/// Builds the path of a box shape inside `r`.
pub fn box_shape_path(kind: ShapeKind, r: &RectF) -> Option<Path> {
    let (x, y, w, h) = (r.min.x, r.min.y, r.width().max(1.0), r.height().max(1.0));
    let r = RectF::from_xywh(x, y, w, h);
    match kind {
        ShapeKind::Line | ShapeKind::Curve | ShapeKind::Polygon => None,
        ShapeKind::Oval => PathBuilder::from_oval(tiny_skia::Rect::from_xywh(x, y, w, h)?),
        ShapeKind::Rectangle => Some(PathBuilder::from_rect(tiny_skia::Rect::from_xywh(x, y, w, h)?)),
        ShapeKind::RoundedRectangle => {
            let mut pb = PathBuilder::new();
            rounded_rect(&mut pb, x, y, w, h, w.min(h) / 6.0, None);
            pb.finish()
        }
        ShapeKind::Triangle => poly(&[(0.5, 0.0), (1.0, 1.0), (0.0, 1.0)], &r),
        ShapeKind::RightTriangle => poly(&[(0.0, 0.0), (1.0, 1.0), (0.0, 1.0)], &r),
        ShapeKind::Diamond => poly(&[(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)], &r),
        ShapeKind::Pentagon => poly(&radial(5, None), &r),
        ShapeKind::Hexagon => poly(&[(0.25, 0.0), (0.75, 0.0), (1.0, 0.5), (0.75, 1.0), (0.25, 1.0), (0.0, 0.5)], &r),
        ShapeKind::RightArrow | ShapeKind::LeftArrow | ShapeKind::UpArrow | ShapeKind::DownArrow => {
            let horizontal = matches!(kind, ShapeKind::RightArrow | ShapeKind::LeftArrow);
            let (len, thick) = if horizontal { (w, h) } else { (h, w) };
            let head = (thick * 0.5).min(len) / len;
            let base = [(0.0, 0.25), (1.0 - head, 0.25), (1.0 - head, 0.0), (1.0, 0.5), (1.0 - head, 1.0), (1.0 - head, 0.75), (0.0, 0.75)];
            let pts: Vec<(f32, f32)> = base
                .iter()
                .map(|&(u, v)| match kind {
                    ShapeKind::RightArrow => (u, v),
                    ShapeKind::LeftArrow => (1.0 - u, v),
                    ShapeKind::DownArrow => (v, u),
                    _ => (v, 1.0 - u),
                })
                .collect();
            poly(&pts, &r)
        }
        ShapeKind::Star4 => poly(&radial(4, Some(0.38)), &r),
        ShapeKind::Star5 => poly(&radial(5, Some(0.382)), &r),
        ShapeKind::Star6 => poly(&radial(6, Some(0.577)), &r),
        ShapeKind::RoundedRectCallout => {
            let body = h * 0.78;
            let mut pb = PathBuilder::new();
            let tail = [(x + w * 0.42, y + body), (x + w * 0.18, y + h), (x + w * 0.24, y + body)];
            rounded_rect(&mut pb, x, y, w, body, w.min(body) / 6.0, Some(tail));
            pb.finish()
        }
        ShapeKind::OvalCallout => {
            let body = h * 0.8;
            let (cx, cy, rx, ry) = (x + w / 2.0, y + body / 2.0, w / 2.0, body / 2.0);
            let n = 96;
            let pts = ellipse_points(cx, cy, rx, ry, n);
            let mut pb = PathBuilder::new();
            // Tail between roughly 105° and 125° (bottom-left, y grows downwards).
            let (a0, a1) = (n * 105 / 360, n * 125 / 360);
            for (i, p) in pts.iter().enumerate() {
                if i == 0 {
                    pb.move_to(p.0, p.1);
                } else if i == a0 {
                    pb.line_to(p.0, p.1);
                    pb.line_to(x + w * 0.15, y + h);
                } else if i > a0 && i < a1 {
                    continue;
                } else {
                    pb.line_to(p.0, p.1);
                }
            }
            pb.close();
            pb.finish()
        }
        ShapeKind::CloudCallout => {
            let body = h * 0.8;
            // Scalloped outline: the radius oscillates with |sin|, giving round bumps outwards and
            // sharp valleys inwards.
            let bumps = 11.0;
            let mut pts: Vec<(f32, f32)> = (0..240)
                .map(|i| {
                    let t = i as f32 * std::f32::consts::TAU / 240.0;
                    let f = 0.8 + 0.2 * (bumps * t / 2.0).sin().abs();
                    (t.cos() * f, t.sin() * f)
                })
                .collect();
            normalize(&mut pts);
            let mut pb = PathBuilder::new();
            for (i, &(u, v)) in pts.iter().enumerate() {
                let (px, py) = (x + u * w, y + v * body);
                if i == 0 { pb.move_to(px, py) } else { pb.line_to(px, py) }
            }
            pb.close();
            // Thought bubbles towards the bottom-left corner.
            for (k, s) in [(0.0f32, 0.07f32), (1.0, 0.05), (2.0, 0.035)] {
                let rad = w.min(h) * s;
                let bx = (x + w * (0.27 - k * 0.08)).max(x + rad);
                let by = (y + body + (h - body) * (0.2 + k * 0.3)).min(y + h - rad);
                if let Some(rect) = tiny_skia::Rect::from_xywh(bx - rad, by - rad * 0.8, rad * 2.0, rad * 1.6) {
                    pb.push_oval(rect);
                }
            }
            pb.finish()
        }
        ShapeKind::Heart => {
            let mut pts: Vec<(f32, f32)> = (0..96)
                .map(|i| {
                    let t = i as f32 * std::f32::consts::TAU / 96.0;
                    let px = 16.0 * t.sin().powi(3);
                    let py = 13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos();
                    (px, -py)
                })
                .collect();
            normalize(&mut pts);
            poly(&pts, &r)
        }
        ShapeKind::Lightning => {
            const P: [(f32, f32); 11] = [
                (8472.0, 0.0),
                (12573.0, 7602.0),
                (11103.0, 8442.0),
                (15308.0, 13219.0),
                (14067.0, 13887.0),
                (21600.0, 21600.0),
                (10287.0, 14787.0),
                (11745.0, 13982.0),
                (5767.0, 9418.0),
                (7304.0, 8690.0),
                (0.0, 3810.0),
            ];
            let pts: Vec<(f32, f32)> = P.iter().map(|&(u, v)| (u / 21600.0, v / 21600.0)).collect();
            poly(&pts, &r)
        }
    }
}

/// Path of a straight line.
pub fn line_path(a: Point, b: Point) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(a.x, a.y);
    pb.line_to(b.x + if a == b { 0.01 } else { 0.0 }, b.y);
    pb.finish()
}

/// Path of a cubic curve between `a` and `b` with control points `c1`, `c2`.
pub fn curve_path(a: Point, c1: Point, c2: Point, b: Point) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(a.x, a.y);
    pb.cubic_to(c1.x, c1.y, c2.x, c2.y, b.x + if a == b { 0.01 } else { 0.0 }, b.y);
    pb.finish()
}

/// Path of a free polygon.
pub fn polygon_path(points: &[Point], closed: bool) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let (first, rest) = points.split_first()?;
    pb.move_to(first.x, first.y);
    for p in rest {
        pb.line_to(p.x, p.y);
    }
    if rest.is_empty() {
        pb.line_to(first.x + 0.01, first.y);
    }
    if closed && points.len() > 2 {
        pb.close();
    }
    pb.finish()
}

/// A rasterized shape ready to be composited at (`x`,`y`).
#[derive(Clone, Debug)]
pub struct RasterShape {
    pub pixmap: Pixmap,
    pub x: i32,
    pub y: i32,
}

impl RasterShape {
    pub fn bounds(&self) -> PixelRect {
        PixelRect::from_xywh(self.x, self.y, self.pixmap.width(), self.pixmap.height())
    }
}

/// Rasterizes `path` (already in canvas coordinates) clipped to the canvas.
pub fn rasterize(path: &Path, paint: &ShapePaint, closed: bool, canvas_w: u32, canvas_h: u32) -> Option<RasterShape> {
    let b = path.bounds();
    let pad = paint.width + 2.0;
    let area = PixelRect::enclosing(b.left() - pad, b.top() - pad, b.right() + pad, b.bottom() + pad).clamp_to(canvas_w, canvas_h);
    if area.is_empty() {
        return None;
    }
    let mut out = raster::pixmap(area.width(), area.height());
    let to_local = Transform::from_translate(-area.x0 as f32, -area.y0 as f32);

    if closed && paint.fill != ShapeStyle::None {
        draw_styled(&mut out, path, None, paint.fill, paint.fill_color, to_local, area);
    }
    if paint.outline != ShapeStyle::None || !closed {
        let style = if paint.outline == ShapeStyle::None { ShapeStyle::Solid } else { paint.outline };
        let stroke = Stroke {
            width: paint.width.max(1.0),
            line_cap: if closed { LineCap::Butt } else { LineCap::Round },
            line_join: if matches!(style, ShapeStyle::Solid) { LineJoin::Miter } else { LineJoin::Round },
            ..Stroke::default()
        };
        draw_styled(&mut out, path, Some(&stroke), style, paint.outline_color, to_local, area);
    }
    Some(RasterShape { pixmap: out, x: area.x0, y: area.y0 })
}

fn draw_styled(out: &mut Pixmap, path: &Path, stroke: Option<&Stroke>, style: ShapeStyle, color: Color, ts: Transform, area: PixelRect) {
    match style {
        ShapeStyle::None => {}
        ShapeStyle::Solid => {
            let mut p = Paint::default();
            p.set_color(color.to_skia());
            p.anti_alias = true;
            match stroke {
                Some(s) => out.stroke_path(path, &p, s, ts, None),
                None => out.fill_path(path, &p, FillRule::Winding, ts, None),
            }
        }
        ShapeStyle::Textured(texture) => {
            let Some(mut mask) = Mask::new(out.width(), out.height()) else { return };
            match stroke {
                Some(s) => {
                    if let Some(outline) = path.stroke(s, 1.0) {
                        mask.fill_path(&outline, FillRule::Winding, true, ts);
                    }
                }
                None => mask.fill_path(path, FillRule::Winding, true, ts),
            }
            let w = out.width() as usize;
            let pm = color.premultiplied();
            let md = mask.data().to_vec();
            let data = out.data_mut();
            for (i, &m) in md.iter().enumerate() {
                if m == 0 {
                    continue;
                }
                let (lx, ly) = ((i % w) as i32, (i / w) as i32);
                let f = texture.factor(lx + area.x0, ly + area.y0);
                let k = (f32::from(m) * f).round() as u16;
                let px = &mut data[i * 4..i * 4 + 4];
                let sa = crate::color::mul_div255(u16::from(pm[3]), k);
                let inv = 255 - u16::from(sa);
                for c in 0..3 {
                    px[c] = crate::color::mul_div255(u16::from(pm[c]), k).saturating_add(crate::color::mul_div255(u16::from(px[c]), inv));
                }
                px[3] = sa.saturating_add(crate::color::mul_div255(u16::from(px[3]), inv));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_box_shape_has_a_path_inside_its_box() {
        let r = RectF::from_xywh(10.0, 20.0, 100.0, 60.0);
        for kind in ShapeKind::ALL {
            if kind.is_point_based() {
                assert!(box_shape_path(kind, &r).is_none());
                continue;
            }
            let p = box_shape_path(kind, &r).unwrap_or_else(|| panic!("{kind:?}"));
            let b = p.bounds();
            assert!(b.left() >= 9.0 && b.top() >= 19.0 && b.right() <= 111.0 && b.bottom() <= 81.0, "{kind:?} {b:?}");
            assert!(b.width() > 50.0 && b.height() > 30.0, "{kind:?} {b:?}");
        }
    }

    #[test]
    fn rasterize_fill_and_outline() {
        let paint = ShapePaint {
            outline: ShapeStyle::Solid,
            fill: ShapeStyle::Solid,
            width: 3.0,
            outline_color: Color::BLACK,
            fill_color: Color::rgb(255, 0, 0),
        };
        let path = box_shape_path(ShapeKind::Rectangle, &RectF::from_xywh(10.0, 10.0, 20.0, 20.0)).unwrap();
        let s = rasterize(&path, &paint, true, 100, 100).unwrap();
        let at = |x: i32, y: i32| raster::get_pixel(&s.pixmap, x - s.x, y - s.y).unwrap();
        assert_eq!(at(20, 20), Color::rgb(255, 0, 0));
        assert_eq!(at(10, 20), Color::BLACK);
        assert_eq!(at(20, 6).a, 0);
    }

    #[test]
    fn textured_styles_render() {
        for style in ShapeStyle::OPTIONS {
            let paint = ShapePaint { outline: style, fill: style, width: 8.0, outline_color: Color::BLACK, fill_color: Color::BLACK };
            let path = box_shape_path(ShapeKind::Oval, &RectF::from_xywh(0.0, 0.0, 50.0, 50.0)).unwrap();
            let s = rasterize(&path, &paint, true, 64, 64).unwrap();
            let opaque = s.pixmap.pixels().iter().filter(|p| p.alpha() > 0).count();
            if style == ShapeStyle::None {
                assert_eq!(opaque, 0);
            } else {
                assert!(opaque > 100, "{style:?}");
            }
        }
    }

    #[test]
    fn open_paths() {
        assert!(line_path(Point::new(0.0, 0.0), Point::new(0.0, 0.0)).is_some());
        assert!(curve_path(Point::new(0.0, 0.0), Point::new(5.0, 9.0), Point::new(9.0, 1.0), Point::new(10.0, 10.0)).is_some());
        assert!(polygon_path(&[Point::new(1.0, 1.0)], true).is_some());
        assert!(polygon_path(&[], true).is_none());
    }
}
