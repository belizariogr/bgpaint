//! The pointer state machine on the canvas.

use super::Tool;
use super::paint::StrokeOp;
use crate::app::PaintApp;
use bgpaint_core::document::ActivePreview;
use bgpaint_core::geom::{PixelRect, Point};
use bgpaint_core::history::Change;
use egui::{Modifiers, PointerButton, Pos2};

/// Canvas resize handles, as compass directions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

impl Handle {
    pub const ALL: [Handle; 8] = [Handle::N, Handle::S, Handle::E, Handle::W, Handle::NE, Handle::NW, Handle::SE, Handle::SW];

    /// Position on a unit square (0..1).
    pub fn anchor(self) -> (f32, f32) {
        match self {
            Handle::N => (0.5, 0.0),
            Handle::S => (0.5, 1.0),
            Handle::E => (1.0, 0.5),
            Handle::W => (0.0, 0.5),
            Handle::NE => (1.0, 0.0),
            Handle::NW => (0.0, 0.0),
            Handle::SE => (1.0, 1.0),
            Handle::SW => (0.0, 1.0),
        }
    }

    pub fn cursor(self) -> egui::CursorIcon {
        use egui::CursorIcon as C;
        match self {
            Handle::N | Handle::S => C::ResizeVertical,
            Handle::E | Handle::W => C::ResizeHorizontal,
            Handle::NE | Handle::SW => C::ResizeNeSw,
            Handle::NW | Handle::SE => C::ResizeNwSe,
        }
    }

    /// Moves the edges of `r` affected by this handle by (`dx`,`dy`).
    pub fn apply(self, r: PixelRect, dx: i32, dy: i32) -> PixelRect {
        let (ax, ay) = self.anchor();
        let mut out = r;
        if ax == 0.0 {
            out.x0 = (r.x0 + dx).min(r.x1 - 1);
        } else if ax == 1.0 {
            out.x1 = (r.x1 + dx).max(r.x0 + 1);
        }
        if ay == 0.0 {
            out.y0 = (r.y0 + dy).min(r.y1 - 1);
        } else if ay == 1.0 {
            out.y1 = (r.y1 + dy).max(r.y0 + 1);
        }
        out
    }
}

pub struct ResizeOp {
    pub handle: Handle,
    pub start: Point,
    /// The new canvas area expressed in current canvas coordinates.
    pub rect: PixelRect,
}

/// What the pointer is currently doing on the canvas.
#[derive(Default)]
pub enum Active {
    #[default]
    None,
    Stroke {
        op: StrokeOp,
        button: PointerButton,
    },
    Resize(ResizeOp),
    Pan {
        last: Pos2,
        button: PointerButton,
    },
    Shape(super::shape::ShapeOp),
    Select(super::select::SelectOp),
    Text(super::text::TextOp),
}

impl Active {
    pub fn preview(&self) -> Option<&dyn ActivePreview> {
        match self {
            Active::Stroke { op, .. } => Some(op),
            Active::Shape(op) => Some(op),
            Active::Select(op) => op.preview(),
            Active::Text(op) => Some(op),
            _ => None,
        }
    }

    pub fn is_idle(&self) -> bool {
        matches!(self, Active::None)
    }
}

impl PaintApp {
    pub fn pointer_down(&mut self, pos: Pos2, button: PointerButton, modifiers: Modifiers, space: bool) {
        if button == PointerButton::Middle || (space && button == PointerButton::Primary) {
            self.active = Active::Pan { last: pos, button };
            return;
        }
        let secondary = button == PointerButton::Secondary;
        if button != PointerButton::Primary && !secondary {
            return;
        }
        if self.active.is_idle()
            && button == PointerButton::Primary
            && let Some(h) = self.handle_at(pos)
        {
            let start = self.view.to_canvas(pos);
            self.active = Active::Resize(ResizeOp { handle: h, start, rect: self.doc.bounds() });
            return;
        }
        let p = self.view.to_canvas(pos);
        match self.tool {
            Tool::Pencil | Tool::Brush | Tool::Eraser => {
                self.commit_interaction();
                if let Some(op) = self.begin_stroke(p, secondary) {
                    self.active = Active::Stroke { op, button };
                }
            }
            Tool::Fill => {
                self.commit_interaction();
                self.fill_at(p, secondary);
            }
            Tool::Picker => self.pick_color(p, secondary),
            Tool::Magnifier => self.magnify(pos, secondary),
            Tool::Shape => self.shape_pointer_down(p, pos, button, modifiers),
            Tool::Select => self.select_pointer_down(p, pos, button, modifiers),
            Tool::Text => self.text_pointer_down(p, pos, button, modifiers),
        }
    }

    pub fn pointer_move(&mut self, pos: Pos2, modifiers: Modifiers) {
        let p = self.view.to_canvas(pos);
        let mut active = std::mem::take(&mut self.active);
        match &mut active {
            Active::Stroke { op, .. } => self.extend_stroke(op, p),
            Active::Resize(op) => {
                let (dx, dy) = ((p.x - op.start.x).round() as i32, (p.y - op.start.y).round() as i32);
                op.rect = op.handle.apply(self.doc.bounds(), dx, dy);
                op.rect = clamp_dragged(op.rect, op.handle);
            }
            Active::Pan { last, .. } => {
                self.view.scroll(pos - *last);
                *last = pos;
            }
            Active::Shape(op) => self.shape_pointer_move(op, p, modifiers),
            Active::Select(op) => self.select_pointer_move(op, p, modifiers),
            Active::Text(op) => self.text_pointer_move(op, p),
            Active::None => {}
        }
        if matches!(self.active, Active::None) {
            self.active = active;
        }
    }

    pub fn pointer_up(&mut self, pos: Pos2, button: PointerButton) {
        let p = self.view.to_canvas(pos);
        let active = std::mem::take(&mut self.active);
        match active {
            Active::Stroke { op, button: b } if b == button => self.finish_stroke(op),
            Active::Resize(op) => {
                let r = op.rect;
                if r != self.doc.bounds() {
                    let before = self.doc.snapshot();
                    self.doc.resize_canvas(r.width(), r.height(), -r.x0, -r.y0);
                    self.history.push("resize canvas", vec![Change::document(before)]);
                }
            }
            Active::Pan { button: b, .. } if b == button => {}
            Active::Shape(op) => {
                if let Some(op) = self.shape_pointer_up(op, p, button) {
                    self.active = Active::Shape(op);
                }
            }
            Active::Select(mut op) => {
                self.select_pointer_up(&mut op, p, button);
                // A click that selected nothing leaves no operation behind.
                if op.established() {
                    self.active = Active::Select(op);
                }
            }
            Active::Text(mut op) => {
                self.text_pointer_up(&mut op, p);
                self.active = Active::Text(op);
            }
            other => self.active = other,
        }
    }

    /// Called every frame (airbrush spraying while the pointer is held still).
    pub fn pointer_hold(&mut self, ctx: &egui::Context) {
        if let Active::Stroke { op, .. } = &mut self.active
            && let Some(d) = op.stroke.hold()
        {
            self.preview_dirty.add(d);
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    /// Finishes whatever is in progress (floating selection, shape, text) into the document.
    pub fn commit_interaction(&mut self) {
        match std::mem::take(&mut self.active) {
            Active::Stroke { op, .. } => self.finish_stroke(op),
            Active::Shape(op) => self.commit_shape(op),
            Active::Select(op) => self.commit_selection(op),
            Active::Text(op) => self.commit_text(op),
            _ => {}
        }
    }

    /// Abandons whatever is in progress without changing the document.
    pub fn cancel_interaction(&mut self) {
        let active = std::mem::take(&mut self.active);
        if let Some(p) = active.preview() {
            self.preview_dirty.add(p.bounds());
        }
        match active {
            Active::Select(op) => self.cancel_selection(op),
            Active::Shape(op) => self.cancel_shape(op),
            _ => {}
        }
    }

    /// Screen rectangles of the 8 canvas resize handles.
    pub fn handle_rects(&self) -> Vec<(Handle, egui::Rect)> {
        let r = self.view.canvas_rect();
        Handle::ALL
            .iter()
            .map(|&h| {
                let (ax, ay) = h.anchor();
                let c = egui::pos2(r.left() + r.width() * ax, r.top() + r.height() * ay);
                let off = egui::vec2(
                    if ax == 0.0 {
                        -3.0
                    } else if ax == 1.0 {
                        3.0
                    } else {
                        0.0
                    },
                    if ay == 0.0 {
                        -3.0
                    } else if ay == 1.0 {
                        3.0
                    } else {
                        0.0
                    },
                );
                (h, egui::Rect::from_center_size(c + off, egui::vec2(7.0, 7.0)))
            })
            .collect()
    }

    pub fn handle_at(&self, pos: Pos2) -> Option<Handle> {
        if !matches!(
            self.tool,
            Tool::Pencil | Tool::Brush | Tool::Eraser | Tool::Fill | Tool::Picker | Tool::Select | Tool::Shape | Tool::Text
        ) {
            return None;
        }
        self.handle_rects().into_iter().find(|(_, r)| r.expand(4.0).contains(pos)).map(|(h, _)| h)
    }
}

impl PaintApp {
    /// Applies the current outline/fill/size/colors to the shape or text being edited.
    pub fn on_shape_style_changed(&mut self) {
        self.restyle_shape();
        self.restyle_text();
    }

    /// Handles and frames of the operation in progress.
    pub fn paint_tool_overlays(&self, painter: &egui::Painter, t: &crate::theme::Theme) {
        match &self.active {
            Active::Shape(op) => super::shape::paint_overlay(op, &self.view, painter, t),
            Active::Select(op) => super::select::paint_overlay(op, &self.view, painter, t),
            Active::Text(op) => super::text::paint_overlay(op, &self.view, painter, t),
            _ => {}
        }
    }
}

/// Limits a canvas rectangle to the supported side and area.
pub fn clamp_area(mut r: PixelRect) -> PixelRect {
    use bgpaint_core::raster::{MAX_AREA, MAX_SIDE};
    r.x1 = r.x1.min(r.x0 + MAX_SIDE as i32);
    r.y1 = r.y1.min(r.y0 + MAX_SIDE as i32);
    while r.area() > MAX_AREA {
        let w = r.width() as u64;
        r.y1 = r.y0 + (MAX_AREA / w.max(1)) as i32;
    }
    r
}

/// Limits a canvas rectangle resized by dragging `h`, shrinking only the edges being dragged.
pub fn clamp_dragged(mut r: PixelRect, h: Handle) -> PixelRect {
    use bgpaint_core::raster::{MAX_AREA, MAX_SIDE};
    let (ax, ay) = h.anchor();
    let max = MAX_SIDE as i32;
    if r.x1 - r.x0 > max {
        if ax == 0.0 { r.x0 = r.x1 - max } else { r.x1 = r.x0 + max }
    }
    if r.y1 - r.y0 > max {
        if ay == 0.0 { r.y0 = r.y1 - max } else { r.y1 = r.y0 + max }
    }
    if r.area() > MAX_AREA {
        if ay != 0.5 {
            let hgt = (MAX_AREA / u64::from(r.width().max(1))) as i32;
            if ay == 0.0 { r.y0 = r.y1 - hgt } else { r.y1 = r.y0 + hgt }
        } else {
            let w = (MAX_AREA / u64::from(r.height().max(1))) as i32;
            if ax == 0.0 { r.x0 = r.x1 - w } else { r.x1 = r.x0 + w }
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragged_edges_are_clamped() {
        let r = clamp_dragged(PixelRect::new(-40_000, 0, 100, 100), Handle::W);
        assert_eq!((r.x1, r.width()), (100, bgpaint_core::raster::MAX_SIDE));
        let r = clamp_dragged(PixelRect::new(0, 0, 30_000, 30_000), Handle::SE);
        assert!(r.area() <= bgpaint_core::raster::MAX_AREA);
        assert_eq!((r.x0, r.y0), (0, 0));
        assert_eq!(Handle::SE.apply(PixelRect::new(0, 0, 10, 10), -20, 5), PixelRect::new(0, 0, 1, 15));
    }
}
