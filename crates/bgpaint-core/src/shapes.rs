//! Geometry and rasterization of Paint's shapes.

use crate::brush::Texture;
use crate::color::Color;
use crate::geom::{PixelRect, Point, RectF};
use crate::raster;
use crate::text::TextStyle;
use tiny_skia::{FillRule, LineCap, LineJoin, Mask, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShapeKind {
    Line,
    Curve,
    /// A line with rounded corners ending in an arrowhead.
    CurvedArrow,
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
    /// A crescent (half) moon.
    Moon,
}

impl ShapeKind {
    /// All shapes in the order of Paint's gallery.
    pub const ALL: [ShapeKind; 25] = [
        ShapeKind::Line,
        ShapeKind::Curve,
        ShapeKind::CurvedArrow,
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
        ShapeKind::Moon,
    ];

    /// Shapes drawn as an open line: no fill, round line ends.
    pub fn is_open(self) -> bool {
        matches!(self, ShapeKind::Line | ShapeKind::Curve | ShapeKind::CurvedArrow)
    }

    pub fn is_point_based(self) -> bool {
        matches!(self, ShapeKind::Line | ShapeKind::Curve | ShapeKind::CurvedArrow | ShapeKind::Polygon)
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
    /// Opacity of the whole shape (outline and fill together), 0..=1.
    pub opacity: f32,
}

impl ShapePaint {
    /// The paint of a text object: the outline color is the text's, the fill its background.
    pub fn for_text(style: &TextStyle) -> Self {
        Self {
            outline: ShapeStyle::Solid,
            fill: if style.background.is_some() { ShapeStyle::Solid } else { ShapeStyle::None },
            width: 1.0,
            outline_color: style.color,
            fill_color: style.background.unwrap_or(Color::WHITE),
            opacity: style.opacity,
        }
    }

    /// `style` with the colors and opacity of this paint (see [`ShapePaint::for_text`]).
    pub fn text_style(&self, style: &TextStyle) -> TextStyle {
        TextStyle {
            color: self.outline_color,
            background: (self.fill != ShapeStyle::None).then_some(self.fill_color),
            opacity: self.opacity,
            ..style.clone()
        }
    }
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
        ShapeKind::Line | ShapeKind::Curve | ShapeKind::CurvedArrow | ShapeKind::Polygon => None,
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
        ShapeKind::Moon => {
            // The part of a circle left uncovered by a second, offset circle.
            let (d, r2) = (0.55f32, 0.85f32);
            let ix = (1.0 - r2 * r2 + d * d) / (2.0 * d);
            let iy = (1.0 - ix * ix).sqrt();
            let outer = iy.atan2(ix);
            let inner = iy.atan2(ix - d);
            let n = 48;
            let mut pts: Vec<(f32, f32)> = (0..=n)
                .map(|i| {
                    // Outer circle, the long way round through its left side.
                    let a = outer + i as f32 / n as f32 * (std::f32::consts::TAU - 2.0 * outer);
                    (a.cos(), a.sin())
                })
                .collect();
            pts.extend((1..n).map(|i| {
                // Back along the inner circle's left side.
                let a = -inner - i as f32 / n as f32 * (std::f32::consts::TAU - 2.0 * inner);
                (d + r2 * a.cos(), r2 * a.sin())
            }));
            normalize(&mut pts);
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

/// How round an arrow's corners are: the radius is this share of how far the corner sticks out
/// from the straight line between its neighbors (never less than [`arrow_min_radius`]). Small
/// bends stay tight; pulling a corner further out rounds it more, though never into an arch.
const ARROW_CORNER: f32 = 0.5;

/// The smallest corner radius of an arrow drawn `width` thick: a corner never turns into a point.
pub fn arrow_min_radius(width: f32) -> f32 {
    4.0 + width.max(1.0)
}

/// An arrow through `points`: straight lines with rounded corners, ending at the last point in
/// a head. The line passes through every point; each inner point sits in the middle of its
/// corner's arc. Returns the line to stroke, which stops inside the head so its round end never
/// shows past it, and the head, a triangle filled with the outline style. The head only depends
/// on the outline `width`: a thicker line gets a bigger head, a longer arrow does not.
pub fn arrow_parts(points: &[Point], width: f32) -> Option<(Path, Option<Path>)> {
    let mut pts: Vec<Point> = Vec::with_capacity(points.len());
    for &p in points {
        if pts.last().is_none_or(|q: &Point| q.distance(p) > 0.5) {
            pts.push(p);
        }
    }
    let first = *points.first()?;
    if pts.len() < 2 {
        return Some((line_path(first, first)?, None));
    }
    let w = width.max(1.0);
    let (len, half) = arrow_head_size(w);
    // The line stops where the head is wide enough to cover its round end.
    let cut = (1.7 * w).max(0.5 * len);
    let n = pts.len();
    let corners = arrow_corners(&pts, w, cut);
    let mut pb = PathBuilder::new();
    pb.move_to(pts[0].x, pts[0].y);
    // Where the last straight piece starts.
    let mut last_start = pts[0];
    for c in corners.iter().flatten() {
        let ArrowCorner { vertex: p, u1, u2, reach, turn } = *c;
        if reach <= 1e-3 {
            pb.line_to(p.x, p.y);
            last_start = p;
            continue;
        }
        // A cubic close to the circular arc between the two tangent points.
        let radius = reach / (turn / 2.0).tan();
        let handle = 4.0 / 3.0 * (turn / 4.0).tan() * radius;
        let t1 = Point::new(p.x - u1.x * reach, p.y - u1.y * reach);
        let t2 = Point::new(p.x + u2.x * reach, p.y + u2.y * reach);
        pb.line_to(t1.x, t1.y);
        pb.cubic_to(t1.x + u1.x * handle, t1.y + u1.y * handle, t2.x - u2.x * handle, t2.y - u2.y * handle, t2.x, t2.y);
        last_start = t2;
    }
    let tip = pts[n - 1];
    let from = corners.iter().rev().flatten().next().map_or(pts[0], |c| c.vertex);
    let dir = unit(from, tip);
    let end = if last_start.distance(tip) > cut { Point::new(tip.x - dir.x * cut, tip.y - dir.y * cut) } else { last_start };
    pb.line_to(end.x + if end == pts[0] { 0.01 } else { 0.0 }, end.y);
    let back = Point::new(tip.x - dir.x * len, tip.y - dir.y * len);
    let mut head = PathBuilder::new();
    head.move_to(tip.x, tip.y);
    head.line_to(back.x - dir.y * half, back.y + dir.x * half);
    head.line_to(back.x + dir.y * half, back.y - dir.x * half);
    head.close();
    Some((pb.finish()?, head.finish()))
}

/// A rounded corner: the lines meet at `vertex` (coming in along `u1`, leaving along `u2`, turning
/// by `turn` radians) and the arc touches them `reach` before and after it.
#[derive(Clone, Copy)]
struct ArrowCorner {
    vertex: Point,
    u1: Point,
    u2: Point,
    reach: f32,
    turn: f32,
}

fn unit(from: Point, to: Point) -> Point {
    let d = from.distance(to).max(1e-6);
    Point::new((to.x - from.x) / d, (to.y - from.y) / d)
}

/// The corners of an arrow through `pts` (`None` at both ends). Each inner point is where the
/// middle of its arc must pass, so the hidden vertex where the straight lines meet sits further
/// out; the vertices depend on each other, so they are found by a few rounds of refinement.
fn arrow_corners(pts: &[Point], w: f32, cut: f32) -> Vec<Option<ArrowCorner>> {
    let n = pts.len();
    let min_radius = arrow_min_radius(w);
    let mut vertex = pts.to_vec();
    let corner = |vertex: &[Point], i: usize| -> ArrowCorner {
        let (a, p, b) = (vertex[i - 1], vertex[i], vertex[i + 1]);
        let (u1, u2) = (unit(a, p), unit(p, b));
        let turn = (u1.x * u2.x + u1.y * u2.y).clamp(-1.0, 1.0).acos();
        if turn < 0.01 {
            return ArrowCorner { vertex: p, u1, u2, reach: 0.0, turn };
        }
        // How far the point sticks out from the line between its neighbors.
        let q = pts[i];
        let ab = a.distance(b);
        let out = if ab < 1e-3 { a.distance(q) } else { ((b.x - a.x) * (a.y - q.y) - (a.x - q.x) * (b.y - a.y)).abs() / ab };
        let radius = (ARROW_CORNER * out).max(min_radius);
        let (seg_in, seg_out) = (a.distance(p), p.distance(b));
        // Each straight piece is shared with the neighbor corner; the last one keeps room for
        // the head.
        let room_in = if i == 1 { seg_in } else { seg_in / 2.0 };
        let room_out = if i == n - 2 { (seg_out - cut).max(seg_out / 2.0) } else { seg_out / 2.0 };
        let reach = (radius * (turn / 2.0).tan()).min(room_in).min(room_out).max(0.0);
        ArrowCorner { vertex: p, u1, u2, reach, turn }
    };
    // Converges quickly for gentle corners and more slowly for hairpins; stop once the
    // vertices have settled (cheap: a few points, a few hundred rounds at most).
    for _ in 0..400 {
        let mut moved = 0.0f32;
        for i in 1..n - 1 {
            let c = corner(&vertex, i);
            // The arc's middle lies `reach * tan(turn / 4)` inside the vertex, along the
            // bisector; put the vertex that far outside the point.
            let outward = Point::new(c.u1.x - c.u2.x, c.u1.y - c.u2.y);
            let m = (outward.x * outward.x + outward.y * outward.y).sqrt();
            let e = if c.reach > 0.0 && m > 1e-6 { c.reach * (c.turn / 4.0).tan() / m } else { 0.0 };
            let next = Point::new(pts[i].x + outward.x * e, pts[i].y + outward.y * e);
            moved = moved.max(next.distance(vertex[i]));
            vertex[i] = next;
        }
        if moved < 0.005 {
            break;
        }
    }
    (0..n).map(|i| (i > 0 && i < n - 1).then(|| corner(&vertex, i))).collect()
}

/// Length and half-width of an arrowhead for an outline `width` pixels thick.
pub fn arrow_head_size(width: f32) -> (f32, f32) {
    let len = 2.5 * width.max(1.0) + 7.0;
    (len, len * 0.45)
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
    /// Part of the shape fell outside the canvas and is missing from `pixmap`.
    pub clipped: bool,
}

impl RasterShape {
    pub fn bounds(&self) -> PixelRect {
        PixelRect::from_xywh(self.x, self.y, self.pixmap.width(), self.pixmap.height())
    }

    /// Whether a visible pixel of the shape lies within `tol` pixels of `p` (canvas coords).
    pub fn hit(&self, p: Point, tol: f32) -> bool {
        let reach = tol.max(0.0).ceil() as i32;
        let (cx, cy) = p.floor_i();
        let (w, h) = (self.pixmap.width() as i32, self.pixmap.height() as i32);
        let data = self.pixmap.data();
        for y in (cy - reach - self.y).max(0)..=(cy + reach - self.y).min(h - 1) {
            for x in (cx - reach - self.x).max(0)..=(cx + reach - self.x).min(w - 1) {
                if data[(y as usize * w as usize + x as usize) * 4 + 3] > 24 {
                    return true;
                }
            }
        }
        false
    }
}

/// Multiplies a whole image by `opacity`.
pub fn fade(image: &mut Pixmap, opacity: f32) {
    let opacity = (opacity.clamp(0.0, 1.0) * 255.0).round() as u16;
    if opacity < 255 {
        for c in image.data_mut() {
            *c = crate::color::mul_div255(u16::from(*c), opacity);
        }
    }
}

/// Draws `pixels` stretched into `rect` rotated by `angle` degrees around `pivot`, clipped to the
/// canvas.
pub fn render_box(pixels: &Pixmap, rect: RectF, pivot: Point, angle: f32, canvas: (u32, u32)) -> Option<RasterShape> {
    let r = rect;
    let (sx, sy) = (r.width() / pixels.width() as f32, r.height() / pixels.height() as f32);
    let exact = angle == 0.0 && (sx - 1.0).abs() < 1e-4 && (sy - 1.0).abs() < 1e-4 && r.min.x.fract() == 0.0 && r.min.y.fract() == 0.0;
    let c = pivot;
    let whole = if exact {
        PixelRect::from_xywh(r.min.x as i32, r.min.y as i32, pixels.width(), pixels.height())
    } else {
        let (sin, cos) = angle.to_radians().sin_cos();
        let mut corners = [(r.min.x, r.min.y), (r.max.x, r.min.y), (r.max.x, r.max.y), (r.min.x, r.max.y)];
        for (x, y) in &mut corners {
            let (dx, dy) = (*x - c.x, *y - c.y);
            (*x, *y) = (c.x + dx * cos - dy * sin, c.y + dx * sin + dy * cos);
        }
        let xs = corners.iter().map(|p| p.0);
        let ys = corners.iter().map(|p| p.1);
        PixelRect::enclosing(
            xs.clone().fold(f32::MAX, f32::min),
            ys.clone().fold(f32::MAX, f32::min),
            xs.fold(f32::MIN, f32::max),
            ys.fold(f32::MIN, f32::max),
        )
    };
    let bounds = whole.clamp_to(canvas.0, canvas.1);
    if bounds.is_empty() {
        return None;
    }
    let mut out = raster::pixmap(bounds.width(), bounds.height());
    if exact {
        raster::paste_region(&mut out, pixels, r.min.x as i32 - bounds.x0, r.min.y as i32 - bounds.y0);
    } else {
        let t = Transform::from_scale(sx, sy)
            .post_translate(r.min.x, r.min.y)
            .post_concat(Transform::from_rotate_at(angle, c.x, c.y))
            .post_translate(-bounds.x0 as f32, -bounds.y0 as f32);
        raster::draw_transformed(&mut out, pixels, t, true);
    }
    Some(RasterShape { pixmap: out, x: bounds.x0, y: bounds.y0, clipped: bounds != whole })
}

/// Rasterizes `path` (already in canvas coordinates) clipped to the canvas.
pub fn rasterize(path: &Path, paint: &ShapePaint, closed: bool, canvas_w: u32, canvas_h: u32) -> Option<RasterShape> {
    rasterize_with_head(path, None, paint, closed, canvas_w, canvas_h)
}

/// Like [`rasterize`], plus an arrowhead `head` filled with the outline's style and color.
pub fn rasterize_with_head(
    path: &Path,
    head: Option<&Path>,
    paint: &ShapePaint,
    closed: bool,
    canvas_w: u32,
    canvas_h: u32,
) -> Option<RasterShape> {
    let outline = (paint.outline != ShapeStyle::None || !closed).then(|| {
        let style = if paint.outline == ShapeStyle::None { ShapeStyle::Solid } else { paint.outline };
        let stroke = Stroke {
            width: paint.width.max(1.0),
            line_cap: if closed { LineCap::Butt } else { LineCap::Round },
            line_join: if matches!(style, ShapeStyle::Solid) { LineJoin::Miter } else { LineJoin::Round },
            ..Stroke::default()
        };
        (style, stroke)
    });
    // The outline's own extent: mitered corners reach up to `miter_limit * width / 2` past a
    // vertex, far more than the width itself on sharp corners of thick outlines.
    let b = outline.as_ref().and_then(|(_, s)| path.stroke(s, 1.0)).map_or(path.bounds(), |o| o.bounds());
    let b = head
        .and_then(|h| {
            let hb = h.bounds();
            tiny_skia::Rect::from_ltrb(
                b.left().min(hb.left()),
                b.top().min(hb.top()),
                b.right().max(hb.right()),
                b.bottom().max(hb.bottom()),
            )
        })
        .unwrap_or(b);
    let pad = 2.0;
    let whole = PixelRect::enclosing(b.left() - pad, b.top() - pad, b.right() + pad, b.bottom() + pad);
    let area = whole.clamp_to(canvas_w, canvas_h);
    if area.is_empty() {
        return None;
    }
    let mut out = raster::pixmap(area.width(), area.height());
    let to_local = Transform::from_translate(-area.x0 as f32, -area.y0 as f32);
    if closed && paint.fill != ShapeStyle::None {
        draw_styled(&mut out, path, None, paint.fill, paint.fill_color, to_local, area);
    }
    if let Some((style, stroke)) = &outline {
        draw_styled(&mut out, path, Some(stroke), *style, paint.outline_color, to_local, area);
        if let Some(head) = head {
            draw_styled(&mut out, head, None, *style, paint.outline_color, to_local, area);
        }
    }
    // Faded as one piece, so the outline over the fill does not get darker.
    fade(&mut out, paint.opacity);
    Some(RasterShape { pixmap: out, x: area.x0, y: area.y0, clipped: area != whole })
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
            // The drawn outline, not the control points of its curves.
            let b = p.compute_tight_bounds().unwrap();
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
            opacity: 1.0,
        };
        let path = box_shape_path(ShapeKind::Rectangle, &RectF::from_xywh(10.0, 10.0, 20.0, 20.0)).unwrap();
        let s = rasterize(&path, &paint, true, 100, 100).unwrap();
        let at = |x: i32, y: i32| raster::get_pixel(&s.pixmap, x - s.x, y - s.y).unwrap();
        assert_eq!(at(20, 20), Color::rgb(255, 0, 0));
        assert_eq!(at(10, 20), Color::BLACK);
        assert_eq!(at(20, 6).a, 0);
    }

    #[test]
    fn opacity_fades_the_shape_as_one_piece() {
        let paint = ShapePaint {
            outline: ShapeStyle::Solid,
            fill: ShapeStyle::Solid,
            width: 3.0,
            outline_color: Color::BLACK,
            fill_color: Color::rgb(255, 0, 0),
            opacity: 0.5,
        };
        let path = box_shape_path(ShapeKind::Rectangle, &RectF::from_xywh(10.0, 10.0, 20.0, 20.0)).unwrap();
        let s = rasterize(&path, &paint, true, 100, 100).unwrap();
        let at = |x: i32, y: i32| raster::get_pixel(&s.pixmap, x - s.x, y - s.y).unwrap();
        assert_eq!(at(20, 20), Color::rgba(255, 0, 0, 128));
        assert_eq!(at(10, 20), Color::rgba(0, 0, 0, 128));
        // Where the outline covers the fill, it is not faded twice.
        assert!(s.pixmap.pixels().iter().all(|p| p.alpha() <= 128));
    }

    #[test]
    fn thick_outline_keeps_sharp_mitered_corners() {
        // A sharp triangle (miter ratio ~3.5, under the limit of 4): its top corner's miter
        // reaches ~70 px above the vertex, well past `width + 2`.
        let paint = ShapePaint {
            outline: ShapeStyle::Solid,
            fill: ShapeStyle::None,
            width: 40.0,
            outline_color: Color::BLACK,
            fill_color: Color::BLACK,
            opacity: 1.0,
        };
        let mut pb = PathBuilder::new();
        pb.move_to(200.0, 150.0);
        pb.line_to(260.0, 350.0);
        pb.line_to(140.0, 350.0);
        pb.close();
        let path = pb.finish().unwrap();
        let s = rasterize(&path, &paint, true, 400, 400).unwrap();
        // Same outline drawn straight onto a canvas-sized pixmap.
        let mut full = raster::pixmap(400, 400);
        let mut p = Paint::default();
        p.set_color(Color::BLACK.to_skia());
        p.anti_alias = true;
        let stroke = Stroke { width: 40.0, line_cap: LineCap::Butt, line_join: LineJoin::Miter, ..Stroke::default() };
        full.stroke_path(&path, &p, &stroke, Transform::identity(), None);
        let ink = |p: &Pixmap| p.pixels().iter().map(|c| u32::from(c.alpha())).sum::<u32>();
        assert!(s.y < 150 - 60, "miter tip clipped: area starts at y = {}", s.y);
        assert_eq!(ink(&s.pixmap), ink(&full));
    }

    #[test]
    fn textured_styles_render() {
        for style in ShapeStyle::OPTIONS {
            let paint =
                ShapePaint { outline: style, fill: style, width: 8.0, outline_color: Color::BLACK, fill_color: Color::BLACK, opacity: 1.0 };
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
