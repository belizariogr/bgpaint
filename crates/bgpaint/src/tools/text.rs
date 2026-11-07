//! Text tool (implemented in stage 5).

use crate::app::PaintApp;
use bgpaint_core::document::ActivePreview;
use bgpaint_core::geom::{PixelRect, Point};
use bgpaint_core::tiny_skia::Pixmap;
use egui::{PointerButton, Pos2};

pub struct TextOp;

impl ActivePreview for TextOp {
    fn bounds(&self) -> PixelRect {
        PixelRect::EMPTY
    }
    fn apply(&self, _region: &mut Pixmap, _rect: PixelRect) {}
}

impl PaintApp {
    pub fn text_pointer_down(&mut self, _p: Point, _pos: Pos2, _b: PointerButton) {}
    pub fn text_pointer_move(&mut self, _op: &mut TextOp, _p: Point) {}
    pub fn text_pointer_up(&mut self, _op: &mut TextOp, _p: Point) {}
    pub fn commit_text(&mut self, _op: TextOp) {}
}
