//! Selection tool (implemented in stage 4).

use crate::app::PaintApp;
use bgpaint_core::document::ActivePreview;
use bgpaint_core::geom::Point;
use bgpaint_core::tiny_skia::Pixmap;
use egui::{Modifiers, PointerButton, Pos2};

pub struct SelectOp;

impl SelectOp {
    pub fn preview(&self) -> Option<&dyn ActivePreview> {
        None
    }
}

impl PaintApp {
    pub fn select_pointer_down(&mut self, _p: Point, _pos: Pos2, _b: PointerButton, _m: Modifiers) {}
    pub fn select_pointer_move(&mut self, _op: &mut SelectOp, _p: Point, _m: Modifiers) {}
    pub fn select_pointer_up(&mut self, _op: &mut SelectOp, _p: Point, _b: PointerButton) {}
    pub fn commit_selection(&mut self, _op: SelectOp) {}
    pub fn cancel_selection(&mut self, _op: SelectOp) {}
    pub fn has_selection(&self) -> bool {
        false
    }
    pub fn paste_pixmap(&mut self, _p: Pixmap) {}
}

impl PaintApp {
    pub fn cut(&mut self) {}
    pub fn copy(&mut self) {}
    pub fn paste(&mut self) {}
    pub fn copy_visible(&mut self) {}
    pub fn select_all(&mut self) {}
    pub fn invert_selection(&mut self) {}
    pub fn delete_selection(&mut self) {}
    pub fn set_transparent_selection(&mut self, v: bool) {
        self.tools.transparent_selection = v;
    }
    pub fn crop_to_selection(&mut self) {}
    pub fn transform_selection(&mut self, _f: &dyn Fn(&Pixmap) -> Pixmap) -> bool {
        false
    }
    pub fn selection_size(&self) -> Option<(u32, u32)> {
        None
    }
    pub fn select_cursor(&self, _pos: Pos2) -> egui::CursorIcon {
        egui::CursorIcon::Crosshair
    }
    pub fn paint_tool_overlays(&self, _painter: &egui::Painter, _t: &crate::theme::Theme) {}
}
