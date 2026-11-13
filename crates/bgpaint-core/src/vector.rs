//! Vector objects: shapes and text boxes that stay editable on their layer while the document's vector mode is
//! on. Each keeps its geometry and style plus a cached rasterization, drawn above the layer's
//! pixels. Turning vector mode off draws them into the pixels for good.

use crate::geom::{PixelRect, Point, RectF};
use crate::shapes::{self, RasterShape, ShapeKind, ShapePaint};
use crate::text::{self, TextStyle};
use tiny_skia::{Path, Pixmap, Transform};

/// Geometry of a finished shape, in canvas coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum ShapeGeom {
    /// A shape drawn in a box rotated by `angle` degrees around its center. Free polygons keep
    /// their points normalized to the box.
    Box {
        kind: ShapeKind,
        rect: RectF,
        angle: f32,
        points: Option<Vec<(f32, f32)>>,
    },
    Line {
        a: Point,
        b: Point,
    },
    /// Cubic curve from `a` to `b`.
    Curve {
        a: Point,
        b: Point,
        c1: Point,
        c2: Point,
    },
    /// Straight lines through `points` with rounded corners, ending in an arrowhead.
    Arrow {
        points: Vec<Point>,
    },
    /// A text box rotated by `angle` degrees around its center. Its colors come from the object's
    /// paint (see [`ShapePaint::for_text`]). While `auto_size`, the box fits itself to the text;
    /// a manual resize turns that off for good.
    Text {
        rect: RectF,
        angle: f32,
        text: String,
        style: TextStyle,
        auto_size: bool,
    },
}

impl ShapeGeom {
    /// The shape's path and whether it is closed (`None` for text, which is laid out instead).
    /// `width` is the outline width (arrowheads are sized from it).
    pub fn path(&self, width: f32) -> Option<(Path, bool)> {
        match self {
            ShapeGeom::Box { kind, rect, angle, points } => {
                let r = *rect;
                let path = match points {
                    Some(pts) => {
                        let abs: Vec<Point> =
                            pts.iter().map(|&(u, v)| Point::new(r.min.x + u * r.width(), r.min.y + v * r.height())).collect();
                        shapes::polygon_path(&abs, true)?
                    }
                    None => shapes::box_shape_path(*kind, &r)?,
                };
                let c = r.center();
                let path = if *angle != 0.0 { path.transform(Transform::from_rotate_at(*angle, c.x, c.y))? } else { path };
                Some((path, !kind.is_open()))
            }
            ShapeGeom::Line { a, b } => Some((shapes::line_path(*a, *b)?, false)),
            ShapeGeom::Curve { a, b, c1, c2 } => Some((shapes::curve_path(*a, *c1, *c2, *b)?, false)),
            ShapeGeom::Arrow { points } => Some((shapes::arrow_parts(points, width)?.0, false)),
            ShapeGeom::Text { .. } => None,
        }
    }

    /// The filled arrowhead of an arrow drawn `width` thick.
    pub fn head(&self, width: f32) -> Option<Path> {
        match self {
            ShapeGeom::Arrow { points } => shapes::arrow_parts(points, width)?.1,
            _ => None,
        }
    }

    /// Renders the shape with `paint`, clipped to a `canvas`-sized document.
    pub fn rasterize(&self, paint: &ShapePaint, canvas: (u32, u32)) -> Option<RasterShape> {
        if let ShapeGeom::Text { rect, angle, text, style, .. } = self {
            return text::rasterize_box(text, &paint.text_style(style), *rect, *angle, canvas);
        }
        let (path, closed) = self.path(paint.width)?;
        shapes::rasterize_with_head(&path, self.head(paint.width).as_ref(), paint, closed, canvas.0, canvas.1)
    }

    /// The geometry mapped by the affine transform `t` (a whole-image resize, skew, rotation or
    /// flip), staying editable. Points map exactly. A box keeps its kind when it stays a
    /// rectangle (rotated or not); an oval stays an oval; other box shapes become free polygons,
    /// their curves flattened. Text cannot be sheared or mirrored: its box follows the
    /// transform's rotation and scale, and the font scales so the lines keep their breaks.
    pub fn transformed(&self, t: Transform) -> Self {
        let map = |p: &Point| {
            let mut q = [tiny_skia::Point::from_xy(p.x, p.y)];
            t.map_points(&mut q);
            Point::new(q[0].x, q[0].y)
        };
        let lin = |x: f32, y: f32| (t.sx * x + t.kx * y, t.ky * x + t.sy * y);
        match self {
            ShapeGeom::Line { a, b } => ShapeGeom::Line { a: map(a), b: map(b) },
            ShapeGeom::Curve { a, b, c1, c2 } => ShapeGeom::Curve { a: map(a), b: map(b), c1: map(c1), c2: map(c2) },
            ShapeGeom::Arrow { points } => ShapeGeom::Arrow { points: points.iter().map(map).collect() },
            ShapeGeom::Box { kind, rect, angle, points } => {
                let (rad, c) = (angle.to_radians(), map(&rect.center()));
                // Where the box's width and height directions go.
                let u = lin(rad.cos(), rad.sin());
                let v = lin(-rad.sin(), rad.cos());
                let (lu, lv) = (u.0.hypot(u.1), v.0.hypot(v.1));
                let det = u.0 * v.1 - u.1 * v.0;
                // Still a rectangle; a mirrored one only for shapes symmetric about their axes.
                let symmetric = points.is_none()
                    && matches!(
                        kind,
                        ShapeKind::Rectangle
                            | ShapeKind::RoundedRectangle
                            | ShapeKind::Oval
                            | ShapeKind::Diamond
                            | ShapeKind::Hexagon
                            | ShapeKind::Star4
                    );
                if lu > 0.0 && lv > 0.0 && (det > 0.0 || symmetric) && (u.0 * v.0 + u.1 * v.1).abs() <= 1e-4 * lu * lv {
                    let (w, h) = (rect.width() * lu, rect.height() * lv);
                    let rect = RectF::from_xywh(c.x - w / 2.0, c.y - h / 2.0, w, h);
                    return ShapeGeom::Box { kind: *kind, rect, angle: u.1.atan2(u.0).to_degrees(), points: points.clone() };
                }
                if *kind == ShapeKind::Oval && points.is_none() {
                    // The image of an ellipse is an ellipse: its axes are the eigenvectors of
                    // M·Mᵀ, M taking the unit circle to the oval.
                    let (a, b) = (rect.width() / 2.0, rect.height() / 2.0);
                    let m = [[u.0 * a, v.0 * b], [u.1 * a, v.1 * b]];
                    let e00 = m[0][0] * m[0][0] + m[0][1] * m[0][1];
                    let e11 = m[1][0] * m[1][0] + m[1][1] * m[1][1];
                    let e01 = m[0][0] * m[1][0] + m[0][1] * m[1][1];
                    let beta = 0.5 * (2.0 * e01).atan2(e00 - e11);
                    let (mean, diff) = ((e00 + e11) / 2.0, (((e00 - e11) / 2.0).powi(2) + e01 * e01).sqrt());
                    let (rx, ry) = ((mean + diff).max(0.0).sqrt(), (mean - diff).max(0.0).sqrt());
                    let rect = RectF::from_xywh(c.x - rx, c.y - ry, 2.0 * rx, 2.0 * ry);
                    return ShapeGeom::Box { kind: *kind, rect, angle: beta.to_degrees(), points: None };
                }
                if let Some(pts) = self.path(0.0).and_then(|(path, _)| flatten(&path)) {
                    let pts: Vec<Point> = pts.iter().map(map).collect();
                    let (mut min, mut max) = (pts[0], pts[0]);
                    for p in &pts {
                        (min.x, min.y, max.x, max.y) = (min.x.min(p.x), min.y.min(p.y), max.x.max(p.x), max.y.max(p.y));
                    }
                    let (w, h) = ((max.x - min.x).max(1e-3), (max.y - min.y).max(1e-3));
                    let points = pts.iter().map(|p| ((p.x - min.x) / w, (p.y - min.y) / h)).collect();
                    return ShapeGeom::Box {
                        kind: ShapeKind::Polygon,
                        rect: RectF::from_xywh(min.x, min.y, w, h),
                        angle: 0.0,
                        points: Some(points),
                    };
                }
                // Several outlines (a cloud callout): the closest rotated box.
                let (rect, angle, _) = sheared_box(*rect, *angle, c, u, lu, det);
                ShapeGeom::Box { kind: *kind, rect, angle, points: points.clone() }
            }
            ShapeGeom::Text { rect, angle, text, style, auto_size } => {
                let (rad, c) = (angle.to_radians(), map(&rect.center()));
                let u = lin(rad.cos(), rad.sin());
                let v = lin(-rad.sin(), rad.cos());
                let det = u.0 * v.1 - u.1 * v.0;
                let (rect, mut new_angle, scale) = sheared_box(*rect, *angle, c, u, u.0.hypot(u.1), det);
                if det < 0.0 {
                    // Mirrored: the text reads along `u` or against it; keep the turn smallest.
                    let alt = (-v.0).atan2(v.1).to_degrees();
                    let turn = |a: f32| ((a - angle).rem_euclid(360.0)).min((angle - a).rem_euclid(360.0));
                    if turn(alt) < turn(new_angle) {
                        new_angle = alt;
                    }
                }
                let angle = new_angle;
                let mut style = style.clone();
                style.size_pt = (style.size_pt * scale).max(1.0);
                ShapeGeom::Text { rect, angle, text: text.clone(), style, auto_size: *auto_size }
            }
        }
    }

    /// The same geometry moved by (`dx`,`dy`).
    pub fn translated(&self, dx: f32, dy: f32) -> Self {
        let t = |p: &Point| Point::new(p.x + dx, p.y + dy);
        match self {
            ShapeGeom::Box { kind, rect, angle, points } => {
                ShapeGeom::Box { kind: *kind, rect: rect.translate(dx, dy), angle: *angle, points: points.clone() }
            }
            ShapeGeom::Line { a, b } => ShapeGeom::Line { a: t(a), b: t(b) },
            ShapeGeom::Curve { a, b, c1, c2 } => ShapeGeom::Curve { a: t(a), b: t(b), c1: t(c1), c2: t(c2) },
            ShapeGeom::Arrow { points } => ShapeGeom::Arrow { points: points.iter().map(t).collect() },
            ShapeGeom::Text { rect, angle, text, style, auto_size } => ShapeGeom::Text {
                rect: rect.translate(dx, dy),
                angle: *angle,
                text: text.clone(),
                style: style.clone(),
                auto_size: *auto_size,
            },
        }
    }
}

/// A box rotated by `angle` with center mapped to `c`, mapped by a transform that takes its width
/// direction to `u` (of length `lu`) with determinant `det`: the shear is dropped, the width
/// keeps its direction and the height becomes the distance across. Also returns the smaller of
/// the two scales.
fn sheared_box(rect: RectF, angle: f32, c: Point, u: (f32, f32), lu: f32, det: f32) -> (RectF, f32, f32) {
    let (sw, sh) = (lu, if lu > 0.0 { det.abs() / lu } else { 0.0 });
    let (w, h) = (rect.width() * sw, rect.height() * sh);
    let angle = if lu > 0.0 { u.1.atan2(u.0).to_degrees() } else { angle };
    (RectF::from_xywh(c.x - w / 2.0, c.y - h / 2.0, w, h), angle, sw.min(sh))
}

/// The points of a single closed outline, curves flattened (`None` for several outlines).
fn flatten(path: &Path) -> Option<Vec<Point>> {
    use tiny_skia::PathSegment;
    const STEPS: usize = 16;
    let mut pts: Vec<Point> = Vec::new();
    let mut moves = 0;
    for seg in path.segments() {
        let last = pts.last().copied().unwrap_or(Point::new(0.0, 0.0));
        match seg {
            PathSegment::MoveTo(p) => {
                moves += 1;
                if moves > 1 {
                    return None;
                }
                pts.push(Point::new(p.x, p.y));
            }
            PathSegment::LineTo(p) => pts.push(Point::new(p.x, p.y)),
            PathSegment::QuadTo(p1, p) => {
                for i in 1..=STEPS {
                    let t = i as f32 / STEPS as f32;
                    let mt = 1.0 - t;
                    pts.push(Point::new(
                        mt * mt * last.x + 2.0 * mt * t * p1.x + t * t * p.x,
                        mt * mt * last.y + 2.0 * mt * t * p1.y + t * t * p.y,
                    ));
                }
            }
            PathSegment::CubicTo(p1, p2, p) => {
                for i in 1..=STEPS {
                    let t = i as f32 / STEPS as f32;
                    let mt = 1.0 - t;
                    let (a, b, c, d) = (mt * mt * mt, 3.0 * mt * mt * t, 3.0 * mt * t * t, t * t * t);
                    pts.push(Point::new(a * last.x + b * p1.x + c * p2.x + d * p.x, a * last.y + b * p1.y + c * p2.y + d * p.y));
                }
            }
            PathSegment::Close => {}
        }
    }
    (pts.len() >= 3).then_some(pts)
}

/// A shape kept as an editable object on a layer.
#[derive(Clone, Debug)]
pub struct VectorShape {
    pub geom: ShapeGeom,
    pub paint: ShapePaint,
    /// Cached rasterization (clipped to the canvas); its tiles are shared, so document snapshots
    /// and copies stay cheap.
    raster: Option<RasterShape>,
}

impl VectorShape {
    /// A shape rasterized for a `canvas`-sized document.
    pub fn new(geom: ShapeGeom, paint: ShapePaint, canvas: (u32, u32)) -> Self {
        let mut s = Self { geom, paint, raster: None };
        s.rasterize(canvas);
        s
    }

    /// A shape whose rasterization is already known (it must match `geom` and `paint`).
    pub fn with_raster(geom: ShapeGeom, paint: ShapePaint, raster: Option<RasterShape>) -> Self {
        Self { geom, paint, raster }
    }

    pub fn raster(&self) -> Option<&RasterShape> {
        self.raster.as_ref()
    }

    /// Memory held by this shape's raster alone (not shared with another snapshot), for the
    /// history's memory budget.
    pub fn unshared_bytes(&self) -> usize {
        self.raster.as_ref().map_or(0, RasterShape::unshared_bytes)
    }

    /// Canvas area covered by the shape.
    pub fn bounds(&self) -> PixelRect {
        self.raster.as_ref().map_or(PixelRect::EMPTY, |r| r.bounds())
    }

    /// Maps the shape by an affine transform (see [`ShapeGeom::transformed`]) and re-renders it
    /// for a `canvas`-sized document. The outline width scales with the area.
    pub fn transform(&mut self, t: Transform, canvas: (u32, u32)) {
        self.geom = self.geom.transformed(t);
        self.paint.width *= (t.sx * t.sy - t.kx * t.ky).abs().sqrt();
        self.rasterize(canvas);
    }

    /// Re-renders the cached raster (after a canvas size change).
    pub fn rasterize(&mut self, canvas: (u32, u32)) {
        self.raster = self.geom.rasterize(&self.paint, canvas);
    }

    /// Same geometry and style (the raster follows from them).
    pub fn same_as(&self, other: &VectorShape) -> bool {
        self.geom == other.geom && self.paint == other.paint
    }

    /// Whether the visible shape covers `p`, give or take `tol` pixels.
    pub fn hit(&self, p: Point, tol: f32) -> bool {
        // Text is grabbed by its words, not only by the strokes of its letters.
        let tol = match &self.geom {
            ShapeGeom::Text { style, .. } if tol > 0.0 => tol.max(style.size_px() / 2.0),
            _ => tol,
        };
        self.raster().is_some_and(|r| r.hit(p, tol))
    }

    /// Draws the shape onto `out`, whose pixel (0,0) is canvas pixel `origin`.
    pub fn draw_into(&self, out: &mut Pixmap, origin: (i32, i32)) {
        if let Some(r) = self.raster() {
            r.draw_into(out, origin);
        }
    }

    /// Draws the shape over `band`, rows of an image `width` pixels wide covering canvas `area`.
    pub fn over_band(&self, band: &mut [crate::blend::Px], width: usize, area: PixelRect) {
        if let Some(r) = self.raster() {
            r.over_band(band, width, area);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::shapes::ShapeStyle;

    fn paint() -> ShapePaint {
        ShapePaint {
            outline: ShapeStyle::Solid,
            fill: ShapeStyle::None,
            width: 4.0,
            outline_color: Color::BLACK,
            fill_color: Color::WHITE,
            opacity: 1.0,
        }
    }

    #[test]
    fn hit_follows_the_visible_outline() {
        let geom = ShapeGeom::Box { kind: ShapeKind::Rectangle, rect: RectF::from_xywh(10.0, 10.0, 40.0, 30.0), angle: 0.0, points: None };
        let s = VectorShape::new(geom, paint(), (100, 100));
        assert!(s.hit(Point::new(10.0, 25.0), 0.0));
        assert!(s.hit(Point::new(5.0, 25.0), 4.0));
        // No fill: the empty middle is not part of the shape.
        assert!(!s.hit(Point::new(30.0, 25.0), 3.0));
        assert!(!s.hit(Point::new(80.0, 80.0), 3.0));
    }

    #[test]
    fn arrowhead_grows_with_the_line_width_only() {
        let a = Point::new(10.0, 50.0);
        let arrow = |len: f32| ShapeGeom::Arrow { points: vec![a, Point::new(a.x + len, a.y)] };
        let head = |g: &ShapeGeom, w: f32| g.head(w).unwrap().compute_tight_bounds().unwrap();
        let (thin, thick) = (head(&arrow(200.0), 2.0).height(), head(&arrow(200.0), 12.0).height());
        assert!(thick > thin * 2.0, "{thin} -> {thick}");
        // A longer arrow gets the same head.
        assert!((head(&arrow(400.0), 2.0).height() - thin).abs() < 0.01);
        assert!(!arrow(200.0).path(2.0).unwrap().1, "an arrow is an open line");
        // The line stops inside the head, which ends exactly at the tip.
        let line = arrow(200.0).path(12.0).unwrap().0.compute_tight_bounds().unwrap();
        let tip = head(&arrow(200.0), 12.0);
        assert!(line.right() < tip.right() - 12.0 && line.right() > tip.left());
        assert!((tip.right() - 210.0).abs() < 0.01);
        // Drawn with the outline color: the head is solid.
        let s = VectorShape::new(arrow(200.0), paint(), (300, 100));
        assert!(s.hit(Point::new(205.0, 50.0), 0.0));
    }

    #[test]
    fn arrow_passes_through_every_point_wherever_it_goes() {
        let start = Point::new(40.0, 150.0);
        let tip = Point::new(260.0, 150.0);
        // Points close to the line, far out, behind the start, beyond the tip, on both sides.
        for corners in [
            vec![Point::new(150.0, 140.0)],
            vec![Point::new(150.0, 20.0)],
            vec![Point::new(10.0, 40.0)],
            vec![Point::new(290.0, 260.0)],
            vec![Point::new(100.0, 60.0), Point::new(200.0, 250.0)],
            vec![Point::new(100.0, 60.0), Point::new(120.0, 70.0), Point::new(200.0, 250.0)],
        ] {
            check(start, tip, corners);
        }
        // Found in the app: an almost straight first corner followed by a sharp one.
        check(Point::new(71.0, 521.0), Point::new(821.0, 451.0), vec![Point::new(321.0, 221.0), Point::new(481.0, 41.0)]);
        fn check(start: Point, tip: Point, corners: Vec<Point>) {
            let mut p = paint();
            p.width = 1.0;
            let mut points = vec![start];
            points.extend(&corners);
            points.push(tip);
            let s = VectorShape::new(ShapeGeom::Arrow { points: points.clone() }, p, (1000, 1000));
            for q in &corners {
                assert!(s.hit(*q, 1.0), "the line misses {q:?} in {points:?}");
            }
        }
    }

    #[test]
    fn arrow_corners_keep_a_minimum_radius() {
        // A point barely off the line still gets a rounded corner, not a point: the line passes
        // through it and stays away from the hidden vertex.
        let g = ShapeGeom::Arrow { points: vec![Point::new(0.0, 100.0), Point::new(100.0, 40.0), Point::new(200.0, 100.0)] };
        let top = g.path(2.0).unwrap().0.compute_tight_bounds().unwrap().top();
        assert!((top - 40.0).abs() < 0.6, "{top}");
        let down = ShapeGeom::Arrow { points: vec![Point::new(0.0, 0.0), Point::new(50.0, 0.0), Point::new(50.0, 80.0)] };
        let h = down.head(4.0).unwrap().compute_tight_bounds().unwrap();
        assert!((h.bottom() - 80.0).abs() < 0.01);
        assert!(ShapeGeom::Arrow { points: vec![Point::new(3.0, 3.0)] }.path(2.0).is_some());
    }

    #[test]
    fn moon_is_filled_between_its_arcs_only() {
        let rect = RectF::from_xywh(0.0, 0.0, 60.0, 40.0);
        let mut p = paint();
        p.fill = ShapeStyle::Solid;
        let moon = VectorShape::new(ShapeGeom::Box { kind: ShapeKind::Moon, rect, angle: 0.0, points: None }, p, (100, 100));
        assert!(moon.hit(Point::new(6.0, 20.0), 0.0));
        assert!(!moon.hit(Point::new(50.0, 20.0), 0.0), "the bite of the crescent is empty");
    }

    fn text_object(canvas: (u32, u32)) -> Option<VectorShape> {
        let family = text::fonts().families().first().cloned()?;
        let style = TextStyle {
            family,
            size_pt: 24.0,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            align: text::TextAlign::Left,
            color: Color::BLACK,
            background: None,
            opacity: 1.0,
        };
        let geom = ShapeGeom::Text {
            rect: RectF::from_xywh(10.0, 10.0, 180.0, 50.0),
            angle: 0.0,
            text: "HHHH HHHH".to_owned(),
            style: style.clone(),
            auto_size: false,
        };
        Some(VectorShape::new(geom, ShapePaint::for_text(&style), canvas))
    }

    #[test]
    fn text_object_is_drawn_from_its_content_and_follows_its_paint() {
        let Some(mut s) = text_object((200, 100)) else { return }; // No fonts (minimal CI image).
        let dark = |s: &VectorShape| s.raster().unwrap().to_pixmap().pixels().iter().filter(|c| c.alpha() > 128).count();
        let before = dark(&s);
        assert!(before > 50, "the glyphs are drawn");
        assert_eq!(s.bounds().x0, 10);
        // Growing the canvas renders it again, with nothing lost.
        s.rasterize((400, 300));
        assert_eq!(dark(&s), before);
        // A background comes from the paint's fill, and covers the whole box.
        s.paint.fill = shapes::ShapeStyle::Solid;
        s.rasterize((400, 300));
        let image = &s.raster().unwrap().to_pixmap();
        assert!(image.pixels()[..image.width() as usize * 50].iter().all(|c| c.alpha() == 255));
        // The paint's opacity fades it.
        s.paint.opacity = 0.5;
        s.rasterize((400, 300));
        assert!(s.raster().unwrap().to_pixmap().pixels().iter().all(|c| c.alpha() <= 128));
    }

    #[test]
    fn text_object_is_grabbed_by_its_words() {
        let Some(s) = text_object((200, 100)) else { return };
        // In the gap between the two words, on the text's line.
        let gap = (20..120).find(|&x| !s.hit(Point::new(x as f32, 30.0), 0.0) && s.hit(Point::new(x as f32 - 3.0, 30.0), 0.0));
        let gap = Point::new(gap.expect("a gap between letters") as f32 + 1.0, 30.0);
        assert!(!s.hit(gap, 0.0), "exact tests (the Fill tool) see the letters only");
        assert!(s.hit(gap, 1.0), "the pointer grabs the words");
        assert!(!s.hit(Point::new(185.0, 55.0), 1.0), "the empty rest of the box is not part of it");
        let moved = s.geom.translated(5.0, 7.0);
        let ShapeGeom::Text { rect, .. } = moved else { panic!() };
        assert_eq!(rect, RectF::from_xywh(15.0, 17.0, 180.0, 50.0));
    }

    #[test]
    fn translated_moves_every_kind() {
        let line = ShapeGeom::Line { a: Point::new(0.0, 0.0), b: Point::new(5.0, 5.0) };
        assert_eq!(line.translated(2.0, -1.0), ShapeGeom::Line { a: Point::new(2.0, -1.0), b: Point::new(7.0, 4.0) });
        let curve =
            ShapeGeom::Curve { a: Point::new(0.0, 0.0), b: Point::new(9.0, 0.0), c1: Point::new(3.0, 3.0), c2: Point::new(6.0, 3.0) };
        let ShapeGeom::Curve { c2, .. } = curve.translated(1.0, 1.0) else { panic!() };
        assert_eq!(c2, Point::new(7.0, 4.0));
        let poly = ShapeGeom::Box {
            kind: ShapeKind::Polygon,
            rect: RectF::from_xywh(0.0, 0.0, 10.0, 10.0),
            angle: 30.0,
            points: Some(vec![(0.0, 0.0), (1.0, 0.5), (0.0, 1.0)]),
        };
        let ShapeGeom::Box { rect, angle, points, .. } = poly.translated(5.0, 5.0) else { panic!() };
        assert_eq!((rect, angle, points.unwrap().len()), (RectF::from_xywh(5.0, 5.0, 10.0, 10.0), 30.0, 3));
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    type BoxParts = (ShapeKind, RectF, f32, Option<Vec<(f32, f32)>>);

    fn box_of(g: &ShapeGeom) -> BoxParts {
        let ShapeGeom::Box { kind, rect, angle, points } = g else { panic!("not a box: {g:?}") };
        (*kind, *rect, *angle, points.clone())
    }

    #[test]
    fn a_resize_keeps_boxes_as_they_are() {
        let rect = ShapeGeom::Box { kind: ShapeKind::Star5, rect: RectF::from_xywh(10.0, 20.0, 30.0, 40.0), angle: 0.0, points: None };
        let (kind, r, angle, points) = box_of(&rect.transformed(Transform::from_scale(2.0, 0.5)));
        assert_eq!((kind, angle, points), (ShapeKind::Star5, 0.0, None));
        assert!(close(r.min.x, 20.0) && close(r.min.y, 10.0) && close(r.width(), 60.0) && close(r.height(), 20.0));
    }

    #[test]
    fn a_rotation_turns_the_box() {
        let g = ShapeGeom::Box { kind: ShapeKind::Triangle, rect: RectF::from_xywh(0.0, 0.0, 20.0, 10.0), angle: 10.0, points: None };
        // A quarter turn clockwise of a 100×50 image.
        let t = Transform::from_row(0.0, 1.0, -1.0, 0.0, 50.0, 0.0);
        let (kind, r, angle, _) = box_of(&g.transformed(t));
        assert_eq!(kind, ShapeKind::Triangle);
        assert!(close(angle, 100.0), "{angle}");
        assert!(close(r.center().x, 45.0) && close(r.center().y, 10.0) && close(r.width(), 20.0) && close(r.height(), 10.0));
    }

    #[test]
    fn a_skewed_rectangle_becomes_a_parallelogram() {
        let g = ShapeGeom::Box { kind: ShapeKind::Rectangle, rect: RectF::from_xywh(0.0, 0.0, 10.0, 10.0), angle: 0.0, points: None };
        let (kind, r, angle, points) = box_of(&g.transformed(Transform::from_row(1.0, 0.0, 1.0, 1.0, 0.0, 0.0)));
        assert_eq!((kind, angle), (ShapeKind::Polygon, 0.0));
        assert!(close(r.width(), 20.0) && close(r.height(), 10.0));
        let abs: Vec<(f32, f32)> = points.unwrap().iter().map(|&(u, v)| (u * r.width() + r.min.x, v * r.height() + r.min.y)).collect();
        for corner in [(0.0, 0.0), (10.0, 0.0), (20.0, 10.0), (10.0, 10.0)] {
            assert!(abs.iter().any(|p| close(p.0, corner.0) && close(p.1, corner.1)), "{corner:?} missing from {abs:?}");
        }
    }

    #[test]
    fn a_skewed_oval_stays_an_oval() {
        let g = ShapeGeom::Box { kind: ShapeKind::Oval, rect: RectF::from_xywh(-10.0, -10.0, 20.0, 20.0), angle: 0.0, points: None };
        let t = Transform::from_row(1.0, 0.0, 1.0, 1.0, 0.0, 0.0);
        let (kind, r, angle, _) = box_of(&g.transformed(t));
        assert_eq!(kind, ShapeKind::Oval);
        // Every point of the skewed circle lies on the new ellipse.
        let (a, b, rad) = (r.width() / 2.0, r.height() / 2.0, angle.to_radians());
        for i in 0..12 {
            let th = i as f32 * std::f32::consts::TAU / 12.0;
            let mut p = [tiny_skia::Point::from_xy(10.0 * th.cos(), 10.0 * th.sin())];
            t.map_points(&mut p);
            let (x, y) = (p[0].x * rad.cos() + p[0].y * rad.sin(), -p[0].x * rad.sin() + p[0].y * rad.cos());
            assert!(((x / a).powi(2) + (y / b).powi(2) - 1.0).abs() < 1e-3);
        }
    }

    #[test]
    fn mirrored_text_stays_readable() {
        let g = ShapeGeom::Text {
            rect: RectF::from_xywh(0.0, 0.0, 40.0, 10.0),
            angle: 0.0,
            text: "a".into(),
            style: TextStyle {
                family: String::new(),
                size_pt: 20.0,
                bold: false,
                italic: false,
                underline: false,
                strikethrough: false,
                align: text::TextAlign::Left,
                color: Color::BLACK,
                background: None,
                opacity: 1.0,
            },
            auto_size: false,
        };
        for horizontal in [true, false] {
            let t = crate::raster::flip_transform(100, 100, horizontal);
            let ShapeGeom::Text { rect, angle, style, .. } = g.transformed(t) else { panic!() };
            assert!(close(angle.rem_euclid(360.0), 0.0), "{angle}");
            assert!(close(rect.width(), 40.0) && close(rect.height(), 10.0) && close(style.size_pt, 20.0));
        }
        let ShapeGeom::Text { style, .. } = g.transformed(Transform::from_scale(2.0, 0.5)) else { panic!() };
        assert!(close(style.size_pt, 10.0));
    }
}
