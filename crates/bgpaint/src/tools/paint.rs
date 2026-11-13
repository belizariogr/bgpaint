//! Freehand tools (pencil, brushes, eraser) and the one-click tools (fill, color picker, magnifier).

use super::Tool;
use crate::app::PaintApp;
use bgpaint_core::brush::{BlendMode, BrushKind, Stroke, apply_mask};
use bgpaint_core::color::Color;
use bgpaint_core::document::ActivePreview;
use bgpaint_core::fill::Flood;
use bgpaint_core::geom::{PixelRect, Point};
use bgpaint_core::tiny_skia::Pixmap;

/// A stroke being drawn.
pub struct StrokeOp {
    pub stroke: Stroke,
    pub color: Color,
    pub opacity: f32,
    pub mode: BlendMode,
    pub label: &'static str,
}

impl ActivePreview for StrokeOp {
    fn bounds(&self) -> PixelRect {
        self.stroke.mask().bounds()
    }

    fn apply(&self, region: &mut Pixmap, rect: PixelRect) {
        apply_mask(region, rect, self.stroke.mask(), self.color, self.opacity, self.mode);
    }
}

impl PaintApp {
    /// The color for a pointer button (left = Color 1, right = Color 2).
    pub fn button_color(&self, secondary: bool) -> Color {
        if secondary { self.colors.secondary } else { self.colors.primary }
    }

    pub fn begin_stroke(&mut self, p: Point, secondary: bool) -> Option<StrokeOp> {
        let (kind, label) = match self.tool {
            Tool::Pencil => (BrushKind::Pencil, "pencil"),
            Tool::Eraser => (BrushKind::Eraser, "eraser"),
            Tool::Brush => (self.tools.brush, "brush"),
            _ => return None,
        };
        let settings = self.tools.stroke(self.tool)?;
        let seed = (self.started.elapsed().as_nanos() as u32) | 1;
        let mut stroke = Stroke::new(kind, settings.size, self.doc.width(), self.doc.height(), seed);
        let dirty = stroke.add_point(p);
        self.preview_dirty.add(dirty);
        Some(StrokeOp {
            stroke,
            color: self.button_color(secondary),
            opacity: settings.opacity,
            mode: if kind == BrushKind::Eraser { BlendMode::Erase } else { BlendMode::Over },
            label,
        })
    }

    pub fn extend_stroke(&mut self, op: &mut StrokeOp, p: Point) {
        let dirty = op.stroke.add_point(p);
        self.preview_dirty.add(dirty);
    }

    pub fn finish_stroke(&mut self, op: StrokeOp) {
        let StrokeOp { stroke, color, opacity, mode, label } = op;
        let mask = stroke.finish();
        let bounds = mask.bounds();
        // Only the tiles the stroke covers are saved for undo.
        let change = self.doc.edit_active_where(
            bounds,
            |r| mask.any_in(r),
            |layer| {
                let full = PixelRect::from_size(layer.width(), layer.height());
                apply_mask(layer, full, &mask, color, opacity, mode);
            },
        );
        self.preview_dirty.add(bounds);
        self.history.push_one(label, change);
    }

    pub fn fill_at(&mut self, p: Point, secondary: bool) {
        let (x, y) = p.floor_i();
        let color = self.button_color(secondary);
        // On a vector object the fill would only go into the pixels hidden under it (its own
        // colors come from the Shapes tool).
        if self.doc.vector_mode() && self.doc.object_at(p, 0.0).is_some() {
            return;
        }
        // The area is found on the layer as it looks (vector outlines bound it too); the color goes
        // into its pixels, under the objects.
        let Some(flood) = Flood::at(&self.doc.layer_look(self.doc.active_index()), x, y, 0) else { return };
        // The region and its one pixel fringe: only those tiles are saved for undo.
        let change = self.doc.edit_active_where(flood.bounds(), |r| flood.mask.any_in(r.expand(1)), |layer| flood.paint(layer, color));
        self.history.push_one("fill", change);
    }

    pub fn pick_color(&mut self, p: Point, secondary: bool) {
        let (x, y) = p.floor_i();
        if let Some(c) = self.doc.sample(x, y) {
            if secondary == self.colors.editing_secondary {
                self.set_active_color(c);
            } else {
                if secondary {
                    self.colors.secondary = c;
                } else {
                    self.colors.primary = c;
                }
                self.on_colors_changed();
            }
            // Like Paint, the picker hands control back to the previous tool.
            if self.previous_tool != Tool::Picker {
                self.tool = self.previous_tool;
            }
        }
    }

    pub fn magnify(&mut self, at: egui::Pos2, out: bool) {
        let z = self.view.next_preset(!out);
        self.view.zoom_at(z, at);
    }
}
