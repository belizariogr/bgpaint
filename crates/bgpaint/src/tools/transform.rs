//! A rotatable box with Paint's editing handles, shared by shapes, selections and text boxes.

use super::interaction::Handle;
use crate::canvas::view::View;
use crate::theme::Theme;
use crate::ui::icons::regular as ic;
use crate::ui::widgets;
use bgpaint_core::geom::{Point, RectF};
use egui::{Color32, CursorIcon, Pos2, Stroke};

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct BoxGeom {
    /// The box before rotation.
    pub rect: RectF,
    /// Clockwise rotation around the rect center, in degrees.
    pub angle: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BoxHit {
    Inside,
    Handle(Handle),
    Rotate,
}

impl BoxHit {
    pub fn cursor(self, angle: f32) -> CursorIcon {
        match self {
            BoxHit::Inside => CursorIcon::Move,
            BoxHit::Rotate => CursorIcon::Alias,
            BoxHit::Handle(h) => rotated_cursor(h, angle),
        }
    }
}

fn rotated_cursor(h: Handle, angle: f32) -> CursorIcon {
    let base = match h {
        Handle::E | Handle::W => 0,
        Handle::NE | Handle::SW => 1,
        Handle::N | Handle::S => 2,
        Handle::NW | Handle::SE => 3,
    };
    let steps = ((angle.rem_euclid(180.0) + 22.5) / 45.0) as i32;
    match (base - steps).rem_euclid(4) {
        0 => CursorIcon::ResizeHorizontal,
        1 => CursorIcon::ResizeNeSw,
        2 => CursorIcon::ResizeVertical,
        _ => CursorIcon::ResizeNwSe,
    }
}

fn rotate(p: Point, c: Point, deg: f32) -> Point {
    if deg == 0.0 {
        return p;
    }
    let (s, co) = deg.to_radians().sin_cos();
    let (dx, dy) = (p.x - c.x, p.y - c.y);
    Point::new(c.x + dx * co - dy * s, c.y + dx * s + dy * co)
}

impl BoxGeom {
    pub fn new(rect: RectF) -> Self {
        Self { rect, angle: 0.0 }
    }

    pub fn center(&self) -> Point {
        self.rect.center()
    }

    /// World position of a point given in the unrotated box frame.
    pub fn to_world(&self, p: Point) -> Point {
        rotate(p, self.center(), self.angle)
    }

    /// Unrotated box frame position of a world point.
    pub fn to_local(&self, p: Point) -> Point {
        rotate(p, self.center(), -self.angle)
    }

    pub fn corners(&self) -> [Point; 4] {
        let r = self.rect;
        [r.min, Point::new(r.max.x, r.min.y), r.max, Point::new(r.min.x, r.max.y)].map(|p| self.to_world(p))
    }

    fn handle_point(&self, h: Handle) -> Point {
        let (ax, ay) = h.anchor();
        let r = self.rect;
        self.to_world(Point::new(r.min.x + r.width() * ax, r.min.y + r.height() * ay))
    }

    fn rotate_point(&self, offset: f32) -> Point {
        let r = self.rect;
        self.to_world(Point::new(r.center().x, r.min.y - offset))
    }

    /// What is under `p`; `tol` and `rotate_offset` are in canvas pixels.
    pub fn hit(&self, p: Point, tol: f32, rotate_offset: f32) -> Option<BoxHit> {
        if self.rotate_point(rotate_offset).distance(p) <= tol * 1.6 {
            return Some(BoxHit::Rotate);
        }
        for h in Handle::ALL {
            if self.handle_point(h).distance(p) <= tol {
                return Some(BoxHit::Handle(h));
            }
        }
        let local = self.to_local(p);
        let r = self.rect;
        let inside = local.x >= r.min.x - tol / 2.0
            && local.x <= r.max.x + tol / 2.0
            && local.y >= r.min.y - tol / 2.0
            && local.y <= r.max.y + tol / 2.0;
        inside.then_some(BoxHit::Inside)
    }

    /// Result of dragging `hit` of `self` from `start` to `p`. `keep_ratio` (Shift) keeps the aspect
    /// ratio on corner handles and snaps rotation to 15°.
    pub fn drag(&self, hit: BoxHit, start: Point, p: Point, keep_ratio: bool) -> BoxGeom {
        match hit {
            BoxHit::Inside => BoxGeom { rect: self.rect.translate(p.x - start.x, p.y - start.y), angle: self.angle },
            BoxHit::Rotate => {
                let c = self.center();
                let a0 = (start.y - c.y).atan2(start.x - c.x);
                let a1 = (p.y - c.y).atan2(p.x - c.x);
                let mut angle = (self.angle + (a1 - a0).to_degrees()).rem_euclid(360.0);
                if keep_ratio {
                    angle = (angle / 15.0).round() * 15.0;
                }
                BoxGeom { rect: self.rect, angle }
            }
            BoxHit::Handle(h) => {
                let (ls, lp) = (self.to_local(start), self.to_local(p));
                let (dx, dy) = (lp.x - ls.x, lp.y - ls.y);
                let (ax, ay) = h.anchor();
                let r = self.rect;
                let (mut x0, mut y0, mut x1, mut y1) = (r.min.x, r.min.y, r.max.x, r.max.y);
                if ax == 0.0 {
                    x0 = (x0 + dx).min(x1 - 1.0);
                } else if ax == 1.0 {
                    x1 = (x1 + dx).max(x0 + 1.0);
                }
                if ay == 0.0 {
                    y0 = (y0 + dy).min(y1 - 1.0);
                } else if ay == 1.0 {
                    y1 = (y1 + dy).max(y0 + 1.0);
                }
                if keep_ratio && h.is_corner() && r.width() > 0.0 && r.height() > 0.0 {
                    let ratio = r.width() / r.height();
                    let (w, hgt) = (x1 - x0, y1 - y0);
                    let (nw, nh) = if w / hgt > ratio { (hgt * ratio, hgt) } else { (w, w / ratio) };
                    if ax == 0.0 {
                        x0 = x1 - nw;
                    } else {
                        x1 = x0 + nw;
                    }
                    if ay == 0.0 {
                        y0 = y1 - nh;
                    } else {
                        y1 = y0 + nh;
                    }
                }
                // Keep the opposite anchor fixed in world space despite the rotation.
                let local_center = Point::new((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                let world_center = self.to_world(local_center);
                let (hw, hh) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
                BoxGeom {
                    rect: RectF::from_points(
                        Point::new(world_center.x - hw, world_center.y - hh),
                        Point::new(world_center.x + hw, world_center.y + hh),
                    ),
                    angle: self.angle,
                }
            }
        }
    }

    /// Paints the dashed frame, the 8 resize handles and (optionally) the rotate handle.
    pub fn paint(&self, view: &View, painter: &egui::Painter, t: &Theme, rotate_handle: bool) {
        let pts: Vec<Pos2> = self.corners().iter().map(|p| view.to_screen(*p)).collect();
        let mut closed = pts.clone();
        closed.push(pts[0]);
        painter.add(egui::Shape::line(closed.clone(), Stroke::new(1.0, Color32::WHITE)));
        for w in closed.windows(2) {
            painter.extend(egui::Shape::dashed_line(&[w[0], w[1]], Stroke::new(1.0, Color32::from_gray(20)), 3.0, 3.0));
        }
        for h in Handle::ALL {
            let s = view.to_screen(self.handle_point(h));
            painter.rect(widgets::square(s, 7.0), 0, Color32::WHITE, Stroke::new(1.0, Color32::from_gray(60)), egui::StrokeKind::Inside);
        }
        if rotate_handle {
            let rp = self.rotate_point(24.0 / view.scale());
            let s = view.to_screen(rp);
            painter.circle(s, 10.0, t.panel_bg, Stroke::new(1.0, t.text_secondary));
            widgets::paint_icon(painter, s, ic::ROTATE_CUSTOM, 13.0, t.text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Point, b: Point) -> bool {
        a.distance(b) < 1e-3
    }

    #[test]
    fn resize_keeps_opposite_corner_when_rotated() {
        let g = BoxGeom { rect: RectF::from_xywh(0.0, 0.0, 100.0, 50.0), angle: 30.0 };
        let fixed = g.corners()[0];
        let se = g.handle_point(Handle::SE);
        let moved = Point::new(se.x + 20.0, se.y + 10.0);
        let g2 = g.drag(BoxHit::Handle(Handle::SE), se, moved, false);
        assert!(close(g2.corners()[0], fixed), "{:?} vs {:?}", g2.corners()[0], fixed);
        assert!(g2.rect.width() > 100.0);
    }

    #[test]
    fn hits_and_moves() {
        let g = BoxGeom::new(RectF::from_xywh(10.0, 10.0, 40.0, 20.0));
        assert_eq!(g.hit(Point::new(30.0, 20.0), 4.0, 10.0), Some(BoxHit::Inside));
        assert_eq!(g.hit(Point::new(50.0, 30.0), 4.0, 10.0), Some(BoxHit::Handle(Handle::SE)));
        assert_eq!(g.hit(Point::new(30.0, 0.0), 4.0, 10.0), Some(BoxHit::Rotate));
        assert_eq!(g.hit(Point::new(90.0, 90.0), 4.0, 10.0), None);
        let m = g.drag(BoxHit::Inside, Point::new(0.0, 0.0), Point::new(5.0, -5.0), false);
        assert_eq!(m.rect, RectF::from_xywh(15.0, 5.0, 40.0, 20.0));
    }

    #[test]
    fn keep_ratio_on_corners() {
        let g = BoxGeom::new(RectF::from_xywh(0.0, 0.0, 40.0, 20.0));
        let m = g.drag(BoxHit::Handle(Handle::SE), Point::new(40.0, 20.0), Point::new(100.0, 25.0), true);
        assert!((m.rect.width() / m.rect.height() - 2.0).abs() < 1e-3);
    }
}
