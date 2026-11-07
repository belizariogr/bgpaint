//! Vector objects: shapes that stay editable on their layer while the document's vector mode is
//! on. Each keeps its geometry and style plus a cached rasterization, drawn above the layer's
//! pixels. Turning vector mode off draws them into the pixels for good.

use crate::geom::{PixelRect, Point, RectF};
use crate::shapes::{self, RasterShape, ShapeKind, ShapePaint};
use std::sync::Arc;
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
}

impl ShapeGeom {
    /// The shape's path and whether it is closed. `width` is the outline width (arrowheads
    /// are sized from it).
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
        let (path, closed) = self.path(paint.width)?;
        shapes::rasterize_with_head(&path, self.head(paint.width).as_ref(), paint, closed, canvas.0, canvas.1)
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
        }
    }
}

/// A shape kept as an editable object on a layer.
#[derive(Clone, Debug)]
pub struct VectorShape {
    pub geom: ShapeGeom,
    pub paint: ShapePaint,
    /// Cached rasterization (clipped to the canvas); shared so document snapshots stay cheap.
    raster: Option<Arc<RasterShape>>,
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
        Self { geom, paint, raster: raster.map(Arc::new) }
    }

    pub fn raster(&self) -> Option<&RasterShape> {
        self.raster.as_deref()
    }

    /// Memory held by this shape's raster alone (not shared with another snapshot), for the
    /// history's memory budget.
    pub fn unshared_bytes(&self) -> usize {
        self.raster.as_ref().filter(|r| Arc::strong_count(r) == 1).map_or(0, |r| r.pixmap.data().len())
    }

    /// Canvas area covered by the shape.
    pub fn bounds(&self) -> PixelRect {
        self.raster.as_ref().map_or(PixelRect::EMPTY, |r| r.bounds())
    }

    /// Re-renders the cached raster (after a canvas size change).
    pub fn rasterize(&mut self, canvas: (u32, u32)) {
        self.raster = self.geom.rasterize(&self.paint, canvas).map(Arc::new);
    }

    /// Same geometry and style (the raster follows from them).
    pub fn same_as(&self, other: &VectorShape) -> bool {
        self.geom == other.geom && self.paint == other.paint
    }

    /// Whether the visible shape covers `p`, give or take `tol` pixels.
    pub fn hit(&self, p: Point, tol: f32) -> bool {
        self.raster().is_some_and(|r| r.hit(p, tol))
    }

    /// Draws the shape onto `out`, whose pixel (0,0) is canvas pixel `origin`.
    pub fn draw_into(&self, out: &mut Pixmap, origin: (i32, i32)) {
        if let Some(r) = self.raster() {
            crate::raster::draw_over(out, &r.pixmap, r.x - origin.0, r.y - origin.1, 1.0);
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
}
