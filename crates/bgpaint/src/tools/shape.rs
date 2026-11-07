//! Shape tool (implemented in stage 3).

use crate::app::PaintApp;
use bgpaint_core::document::ActivePreview;
use bgpaint_core::geom::{PixelRect, Point};
use bgpaint_core::tiny_skia::Pixmap;
use egui::{Modifiers, PointerButton, Pos2};

pub struct ShapeOp;

impl ActivePreview for ShapeOp {
    fn bounds(&self) -> PixelRect {
        PixelRect::EMPTY
    }
    fn apply(&self, _region: &mut Pixmap, _rect: PixelRect) {}
}

impl PaintApp {
    pub fn shape_pointer_down(&mut self, _p: Point, _pos: Pos2, _b: PointerButton, _m: Modifiers) {}
    pub fn shape_pointer_move(&mut self, _op: &mut ShapeOp, _p: Point, _m: Modifiers) {}
    pub fn shape_pointer_up(&mut self, _op: &mut ShapeOp, _p: Point, _b: PointerButton) {}
    pub fn commit_shape(&mut self, _op: ShapeOp) {}
}
