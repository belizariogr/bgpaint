//! The Shapes tool. A shape stays editable (move, resize, rotate, restyle) until the user clicks
//! outside of it or switches tools, exactly like Paint.

use super::transform::{BoxGeom, BoxHit};
use crate::app::PaintApp;
use crate::canvas::view::View;
use crate::theme::Theme;
use crate::ui::widgets;
use bgpaint_core::color::Color;
use bgpaint_core::document::ActivePreview;
use bgpaint_core::geom::{PixelRect, Point, RectF};
use bgpaint_core::raster;
use bgpaint_core::shapes::{self, RasterShape, ShapeKind, ShapePaint, ShapeStyle};
use bgpaint_core::tiny_skia::{Path, Pixmap, Transform};
use egui::{Modifiers, PointerButton, Pos2};
use std::time::Instant;

/// Geometry of the shape being edited.
#[derive(Clone, Debug)]
pub enum Geom {
    /// Any shape drawn inside a (possibly rotated) box. Free polygons keep their points
    /// normalized to the box.
    Box {
        kind: ShapeKind,
        geom: BoxGeom,
        points: Option<Vec<(f32, f32)>>,
    },
    Line {
        a: Point,
        b: Point,
    },
    /// Cubic curve; `stage` counts the bends applied so far (0, 1 or 2).
    Curve {
        a: Point,
        b: Point,
        c1: Point,
        c2: Point,
        stage: u8,
    },
    /// Polygon being clicked point by point.
    PolygonDraft {
        points: Vec<Point>,
    },
}

#[derive(Clone, Debug)]
enum Drag {
    Create { start: Point },
    Box(BoxHit, Point, BoxGeom),
    Endpoint(bool),
    Bend(u8),
    PolyPoint,
}

pub struct ShapeOp {
    pub geom: Geom,
    pub paint: ShapePaint,
    drag: Option<Drag>,
    raster: Option<RasterShape>,
    canvas: (u32, u32),
    last_click: Option<(Instant, Point)>,
}

impl ActivePreview for ShapeOp {
    fn bounds(&self) -> PixelRect {
        self.raster.as_ref().map_or(PixelRect::EMPTY, RasterShape::bounds)
    }

    fn apply(&self, region: &mut Pixmap, rect: PixelRect) {
        if let Some(r) = &self.raster {
            raster::draw_over(region, &r.pixmap, r.x - rect.x0, r.y - rect.y0, 1.0);
        }
    }
}

fn snap_45(a: Point, b: Point) -> Point {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt();
    let angle = dy.atan2(dx);
    let step = std::f32::consts::FRAC_PI_4;
    let snapped = (angle / step).round() * step;
    Point::new(a.x + len * snapped.cos(), a.y + len * snapped.sin())
}

fn square_from(a: Point, b: Point) -> Point {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let s = dx.abs().max(dy.abs());
    Point::new(a.x + s * dx.signum(), a.y + s * dy.signum())
}

impl ShapeOp {
    fn path(&self) -> Option<(Path, bool)> {
        match &self.geom {
            Geom::Box { kind, geom, points } => {
                let r = geom.rect;
                let path = match points {
                    Some(pts) => {
                        let abs: Vec<Point> =
                            pts.iter().map(|&(u, v)| Point::new(r.min.x + u * r.width(), r.min.y + v * r.height())).collect();
                        shapes::polygon_path(&abs, true)?
                    }
                    None => shapes::box_shape_path(*kind, &r)?,
                };
                let c = r.center();
                let path = if geom.angle != 0.0 { path.transform(Transform::from_rotate_at(geom.angle, c.x, c.y))? } else { path };
                Some((path, true))
            }
            Geom::Line { a, b } => Some((shapes::line_path(*a, *b)?, false)),
            Geom::Curve { a, b, c1, c2, .. } => Some((shapes::curve_path(*a, *c1, *c2, *b)?, false)),
            Geom::PolygonDraft { points } => Some((shapes::polygon_path(points, false)?, false)),
        }
    }

    /// Re-rasterizes after a change; returns the canvas area to redraw.
    fn update(&mut self) -> PixelRect {
        let before = self.bounds();
        self.raster = self.path().and_then(|(p, closed)| shapes::rasterize(&p, &self.paint, closed, self.canvas.0, self.canvas.1));
        before.union(&self.bounds())
    }

    fn editing(&self) -> bool {
        self.drag.is_none()
    }
}

impl PaintApp {
    fn shape_paint(&self, swap: bool) -> ShapePaint {
        let (outline, fill) =
            if swap { (self.colors.secondary, self.colors.primary) } else { (self.colors.primary, self.colors.secondary) };
        ShapePaint {
            outline: self.tools.outline,
            fill: self.tools.fill,
            width: self.tools.shape_width,
            outline_color: outline,
            fill_color: fill,
        }
    }

    pub fn shape_pointer_down(&mut self, p: Point, _pos: Pos2, button: PointerButton, _m: Modifiers) {
        let tol = 6.0 / self.view.scale();
        let rotate_off = 24.0 / self.view.scale();
        // Interact with the shape being edited.
        if let super::interaction::Active::Shape(op) = &mut self.active {
            if button == PointerButton::Primary {
                if let Geom::PolygonDraft { points } = &mut op.geom {
                    let close = points.len() > 2 && points.first().is_some_and(|f| f.distance(p) <= tol * 1.5);
                    let double = op.last_click.is_some_and(|(t, q)| t.elapsed().as_millis() < 450 && q.distance(p) <= tol * 2.0);
                    if close || double {
                        if double && points.len() > 3 {
                            // The first click of the double click added a vertex; drop it.
                            points.pop();
                        }
                        finish_polygon(op);
                        let d = op.update();
                        self.preview_dirty.add(d);
                    } else {
                        points.push(p);
                        op.last_click = Some((Instant::now(), p));
                        op.drag = Some(Drag::PolyPoint);
                        let d = op.update();
                        self.preview_dirty.add(d);
                    }
                    return;
                }
                let hit = match &op.geom {
                    Geom::Box { geom, .. } => geom.hit(p, tol, rotate_off).map(|h| Drag::Box(h, p, geom.clone())),
                    Geom::Line { a, b } => {
                        if a.distance(p) <= tol * 1.5 {
                            Some(Drag::Endpoint(false))
                        } else if b.distance(p) <= tol * 1.5 {
                            Some(Drag::Endpoint(true))
                        } else {
                            let g = BoxGeom { rect: RectF::from_points(*a, *b), angle: 0.0 };
                            near_segment(*a, *b, p, tol + op.paint.width).then_some(Drag::Box(BoxHit::Inside, p, g))
                        }
                    }
                    Geom::Curve { stage, .. } if *stage < 2 => Some(Drag::Bend(*stage + 1)),
                    _ => None,
                };
                if let Some(h) = hit {
                    op.drag = Some(h);
                    return;
                }
            }
        }
        self.commit_interaction();
        let swap = button == PointerButton::Secondary;
        let kind = self.tools.shape;
        let geom = match kind {
            ShapeKind::Line => Geom::Line { a: p, b: p },
            ShapeKind::Curve => Geom::Curve { a: p, b: p, c1: p, c2: p, stage: 0 },
            ShapeKind::Polygon => Geom::PolygonDraft { points: vec![p, p] },
            k => Geom::Box { kind: k, geom: BoxGeom { rect: RectF::from_points(p, p), angle: 0.0 }, points: None },
        };
        let mut op = ShapeOp {
            geom,
            paint: self.shape_paint(swap),
            drag: Some(Drag::Create { start: p }),
            raster: None,
            canvas: (self.doc.width(), self.doc.height()),
            last_click: Some((Instant::now(), p)),
        };
        let d = op.update();
        self.preview_dirty.add(d);
        self.active = super::interaction::Active::Shape(op);
    }

    pub fn shape_pointer_move(&mut self, op: &mut ShapeOp, p: Point, m: Modifiers) {
        let Some(drag) = op.drag.clone() else { return };
        match (&mut op.geom, drag) {
            (Geom::Box { geom, .. }, Drag::Create { start }) => {
                let end = if m.shift { square_from(start, p) } else { p };
                geom.rect = RectF::from_points(start, end);
            }
            (Geom::Line { b, .. }, Drag::Create { start }) => *b = if m.shift { snap_45(start, p) } else { p },
            (Geom::Curve { b, c1, c2, .. }, Drag::Create { start }) => {
                *b = if m.shift { snap_45(start, p) } else { p };
                *c1 = start.lerp(*b, 1.0 / 3.0);
                *c2 = start.lerp(*b, 2.0 / 3.0);
            }
            (Geom::PolygonDraft { points }, Drag::Create { .. } | Drag::PolyPoint) => {
                if let [.., prev, last] = points.as_mut_slice() {
                    *last = if m.shift { snap_45(*prev, p) } else { p };
                }
            }
            (Geom::Box { geom, .. }, Drag::Box(hit, start, orig)) => *geom = orig.drag(hit, start, p, m.shift),
            (Geom::Line { a, b }, Drag::Box(_, start, _)) => {
                let (dx, dy) = (p.x - start.x, p.y - start.y);
                *a = Point::new(a.x + dx, a.y + dy);
                *b = Point::new(b.x + dx, b.y + dy);
                op.drag = Some(Drag::Box(BoxHit::Inside, p, BoxGeom::default()));
            }
            (Geom::Line { a, b }, Drag::Endpoint(end)) => {
                if end {
                    *b = if m.shift { snap_45(*a, p) } else { p };
                } else {
                    *a = if m.shift { snap_45(*b, p) } else { p };
                }
            }
            (Geom::Curve { a, b, c1, c2, .. }, Drag::Bend(stage)) => {
                // Pull the curve so it passes near the pointer (Paint's bend behaviour).
                let target = Point::new(2.0 * p.x - (a.x + b.x) / 2.0, 2.0 * p.y - (a.y + b.y) / 2.0);
                if stage == 1 {
                    *c1 = target;
                    *c2 = target;
                } else {
                    *c2 = target;
                }
            }
            _ => {}
        }
        let d = op.update();
        self.preview_dirty.add(d);
    }

    /// Ends a drag; returns the shape if it is still being edited.
    pub fn shape_pointer_up(&mut self, mut op: ShapeOp, _p: Point, _button: PointerButton) -> Option<ShapeOp> {
        let Some(drag) = op.drag.take() else { return Some(op) };
        match (&mut op.geom, drag) {
            (Geom::Curve { stage, .. }, Drag::Create { .. }) => *stage = 0,
            (Geom::Curve { stage, .. }, Drag::Bend(s)) => *stage = s,
            (Geom::PolygonDraft { .. }, _) => {
                // Keep collecting points; the next click adds a vertex.
                let n = match &op.geom {
                    Geom::PolygonDraft { points } => points.len(),
                    _ => 0,
                };
                if n < 2 {
                    return None;
                }
            }
            (Geom::Box { geom, .. }, Drag::Create { .. }) => {
                if geom.rect.width() < 1.0 && geom.rect.height() < 1.0 {
                    // A click without drag draws nothing.
                    let d = op.bounds();
                    op.raster = None;
                    self.preview_dirty.add(d);
                }
            }
            _ => {}
        }
        // A curve is committed after its second bend.
        if matches!(op.geom, Geom::Curve { stage: 2, .. }) {
            self.commit_shape(op);
            return None;
        }
        Some(op)
    }

    pub fn commit_shape(&mut self, mut op: ShapeOp) {
        if let Geom::PolygonDraft { points } = &op.geom {
            if points.len() >= 2 {
                finish_polygon(&mut op);
                op.update();
            }
        }
        let Some(r) = op.raster.take() else { return };
        let bounds = r.bounds();
        let change = self.doc.edit_active(bounds, |layer| raster::draw_over(layer, &r.pixmap, r.x, r.y, 1.0));
        self.preview_dirty.add(bounds);
        self.history.push_one("shape", change);
    }

    /// Applies new outline/fill/size/colors to the shape being edited.
    pub fn restyle_shape(&mut self) {
        let paint = self.shape_paint(false);
        if let super::interaction::Active::Shape(op) = &mut self.active {
            // Keep a right-button color swap.
            let swapped = op.paint.outline_color == self.colors.secondary
                && op.paint.fill_color == self.colors.primary
                && self.colors.primary != self.colors.secondary;
            op.paint = ShapePaint {
                outline_color: if swapped { paint.fill_color } else { paint.outline_color },
                fill_color: if swapped { paint.outline_color } else { paint.fill_color },
                ..paint
            };
            let d = op.update();
            self.preview_dirty.add(d);
        }
    }

    pub fn shape_cursor(&self, pos: Pos2) -> Option<egui::CursorIcon> {
        let super::interaction::Active::Shape(op) = &self.active else { return None };
        let p = self.view.to_canvas(pos);
        let tol = 6.0 / self.view.scale();
        match &op.geom {
            Geom::Box { geom, .. } if op.editing() => geom.hit(p, tol, 24.0 / self.view.scale()).map(|h| h.cursor(geom.angle)),
            Geom::Line { a, b } if op.editing() => {
                if a.distance(p) <= tol * 1.5 || b.distance(p) <= tol * 1.5 {
                    Some(egui::CursorIcon::Crosshair)
                } else if near_segment(*a, *b, p, tol + op.paint.width) {
                    Some(egui::CursorIcon::Move)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

fn near_segment(a: Point, b: Point, p: Point, tol: f32) -> bool {
    let (vx, vy) = (b.x - a.x, b.y - a.y);
    let len2 = vx * vx + vy * vy;
    let t = if len2 == 0.0 { 0.0 } else { (((p.x - a.x) * vx + (p.y - a.y) * vy) / len2).clamp(0.0, 1.0) };
    Point::new(a.x + vx * t, a.y + vy * t).distance(p) <= tol
}

/// Turns a clicked polygon into an editable box shape with normalized points.
fn finish_polygon(op: &mut ShapeOp) {
    let Geom::PolygonDraft { points } = &op.geom else { return };
    if points.len() < 2 {
        return;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in points {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    let rect = RectF::from_points(Point::new(x0, y0), Point::new(x1.max(x0 + 1.0), y1.max(y0 + 1.0)));
    let norm = points.iter().map(|p| ((p.x - rect.min.x) / rect.width(), (p.y - rect.min.y) / rect.height())).collect();
    op.geom = Geom::Box { kind: ShapeKind::Polygon, geom: BoxGeom { rect, angle: 0.0 }, points: Some(norm) };
    op.drag = None;
}

/// Draws the editing handles of the shape.
pub fn paint_overlay(op: &ShapeOp, view: &View, painter: &egui::Painter, t: &Theme) {
    match &op.geom {
        Geom::Box { geom, .. } if op.editing() => geom.paint(view, painter, t, true),
        Geom::Line { a, b } if op.editing() => {
            for p in [a, b] {
                let s = view.to_screen(*p);
                painter.rect(
                    widgets::square(s, 7.0),
                    0,
                    egui::Color32::WHITE,
                    egui::Stroke::new(1.0, egui::Color32::from_gray(60)),
                    egui::StrokeKind::Inside,
                );
            }
        }
        Geom::Curve { a, b, .. } => {
            for p in [a, b] {
                painter.circle_stroke(view.to_screen(*p), 3.5, egui::Stroke::new(1.0, t.accent));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapping() {
        let s = snap_45(Point::new(0.0, 0.0), Point::new(10.0, 1.0));
        assert!((s.y).abs() < 1e-3 && (s.x - 10.05).abs() < 0.1);
        let q = square_from(Point::new(0.0, 0.0), Point::new(-4.0, 9.0));
        assert_eq!((q.x, q.y), (-9.0, 9.0));
        assert!(near_segment(Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(5.0, 2.0), 3.0));
        assert!(!near_segment(Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(15.0, 0.0), 3.0));
    }

    #[test]
    fn polygon_finishes_into_normalized_box() {
        let mut op = ShapeOp {
            geom: Geom::PolygonDraft { points: vec![Point::new(10.0, 10.0), Point::new(30.0, 10.0), Point::new(20.0, 40.0)] },
            paint: ShapePaint {
                outline: ShapeStyle::Solid,
                fill: ShapeStyle::None,
                width: 1.0,
                outline_color: Color::BLACK,
                fill_color: Color::WHITE,
            },
            drag: None,
            raster: None,
            canvas: (100, 100),
            last_click: None,
        };
        finish_polygon(&mut op);
        let Geom::Box { geom, points: Some(pts), .. } = &op.geom else { panic!() };
        assert_eq!(geom.rect, RectF::from_xywh(10.0, 10.0, 20.0, 30.0));
        assert_eq!(pts[2], (0.5, 1.0));
        assert!(!op.update().is_empty());
    }
}
