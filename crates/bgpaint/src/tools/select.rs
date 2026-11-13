//! Rectangle and free-form selection, floating content (move, resize, rotate, Ctrl+drag copy,
//! transparent selection), clipboard and crop.

use super::interaction::Active;
use super::transform::{BoxGeom, BoxHit};
use super::{SelectMode, Tool};
use crate::app::PaintApp;
use crate::canvas::view::View;
use crate::theme::Theme;
use crate::ui::menubar::MenuKind;
use bgpaint_core::document::ActivePreview;
use bgpaint_core::geom::{PixelRect, Point, RectF};
use bgpaint_core::history::Change;
use bgpaint_core::raster;
use bgpaint_core::selection::{SelectionMask, key_out};
use bgpaint_core::shapes::{self, RasterShape};
use bgpaint_core::tiny_skia::{Pixmap, Transform};
use egui::{Color32, Modifiers, PointerButton, Pos2, Stroke};
use std::sync::Arc;

/// Pixels lifted off the layer and being moved/transformed.
struct Floating {
    /// Content as lifted (or pasted).
    original: Arc<Pixmap>,
    /// `original` after "transparent selection" keyed out Color 2 (the same image when that is
    /// off).
    pixels: Arc<Pixmap>,
    /// Undo records already applied to the document (lift, Ctrl+drag stamps, canvas growth).
    changes: Vec<Change>,
}

#[derive(Clone)]
enum SelDrag {
    Create,
    /// Dragging a handle or the content. `lift` is `Some(copy)` until the first movement lifts
    /// the pixels, so a click without a drag changes nothing.
    Box {
        hit: BoxHit,
        start: Point,
        orig: BoxGeom,
        lift: Option<bool>,
    },
}

pub struct SelectOp {
    mode: SelectMode,
    mask: Option<SelectionMask>,
    lasso: Vec<Point>,
    start: Point,
    current: Point,
    pub geom: BoxGeom,
    floating: Option<Floating>,
    drag: Option<SelDrag>,
    /// Where a transformed (scaled/rotated) floating selection lands (on the canvas) and the
    /// transform that draws its pixels there. It is drawn straight from the pixels into each
    /// region the canvas asks for, so dragging a handle costs nothing until the result shows.
    transformed: Option<(PixelRect, Transform)>,
    /// Position of an untransformed floating selection, drawn straight from its pixels (moving a
    /// large selection then costs no copies).
    exact: Option<(i32, i32)>,
    canvas: (u32, u32),
}

impl ActivePreview for SelectOp {
    fn bounds(&self) -> PixelRect {
        match (&self.exact, &self.floating, &self.transformed) {
            (Some((x, y)), Some(f), _) => {
                PixelRect::from_xywh(*x, *y, f.pixels.width(), f.pixels.height()).clamp_to(self.canvas.0, self.canvas.1)
            }
            (_, Some(_), Some((area, _))) => *area,
            _ => PixelRect::EMPTY,
        }
    }

    fn apply(&self, region: &mut Pixmap, rect: PixelRect) {
        match (&self.exact, &self.floating, &self.transformed) {
            (Some((x, y)), Some(f), _) => raster::draw_over(region, &f.pixels, x - rect.x0, y - rect.y0, 1.0),
            (_, Some(f), Some((area, t))) if !area.intersect(&rect).is_empty() => {
                raster::draw_transformed(region, &f.pixels, t.post_translate(-rect.x0 as f32, -rect.y0 as f32), true);
            }
            _ => {}
        }
    }
}

impl SelectOp {
    pub fn preview(&self) -> Option<&dyn ActivePreview> {
        self.floating.as_ref().map(|_| self as &dyn ActivePreview)
    }

    pub fn established(&self) -> bool {
        self.mask.is_some() || self.floating.is_some()
    }

    /// Renders the floating pixels at the current box; returns the area to redraw.
    fn update(&mut self) -> PixelRect {
        let before = self.bounds();
        self.transformed = None;
        self.exact = None;
        if let Some(f) = &self.floating {
            let r = self.geom.rect;
            let same_size = (r.width() - f.pixels.width() as f32).abs() < 1e-3 && (r.height() - f.pixels.height() as f32).abs() < 1e-3;
            if self.geom.angle == 0.0 && same_size {
                self.exact = Some((r.min.x.round() as i32, r.min.y.round() as i32));
            } else {
                let (whole, t) = shapes::box_transform(&f.pixels, r, self.geom.center(), self.geom.angle);
                let area = whole.clamp_to(self.canvas.0, self.canvas.1);
                self.transformed = Some((area, t.unwrap_or_else(|| Transform::from_translate(r.min.x, r.min.y))));
            }
        }
        before.union(&self.bounds())
    }

    /// The floating content as it shows, rendered once (to stamp or copy it).
    fn rendered(&self) -> Option<RasterShape> {
        let f = self.floating.as_ref()?;
        match self.exact {
            Some((x, y)) => {
                let whole = PixelRect::from_xywh(x, y, f.pixels.width(), f.pixels.height());
                let area = whole.clamp_to(self.canvas.0, self.canvas.1);
                RasterShape::render(area, area != whole, |band, r| raster::paste_region(band, &f.pixels, x - r.x0, y - r.y0))
            }
            None => render_floating(&f.pixels, &self.geom, self.canvas),
        }
    }

    fn pixel_rect(&self) -> PixelRect {
        let r = self.geom.rect;
        PixelRect::new(r.min.x.round() as i32, r.min.y.round() as i32, r.max.x.round() as i32, r.max.y.round() as i32)
    }
}

/// Draws `pixels` stretched into `geom`, clipped to the canvas.
pub(crate) fn render_floating(pixels: &Pixmap, geom: &BoxGeom, canvas: (u32, u32)) -> Option<RasterShape> {
    bgpaint_core::shapes::render_box(pixels, geom.rect, geom.center(), geom.angle, canvas)
}

impl PaintApp {
    fn new_select_op(&self, p: Point) -> SelectOp {
        SelectOp {
            mode: self.tools.select_mode,
            mask: None,
            lasso: vec![p],
            start: p,
            current: p,
            geom: BoxGeom::new(RectF::from_points(p, p)),
            floating: None,
            drag: Some(SelDrag::Create),
            transformed: None,
            exact: None,
            canvas: (self.doc.width(), self.doc.height()),
        }
    }

    pub fn select_pointer_down(&mut self, p: Point, pos: Pos2, button: PointerButton, m: Modifiers) {
        if button == PointerButton::Secondary {
            self.ui.open_menu = Some(MenuKind::Canvas(pos));
            self.ui.menu_anchor = egui::Rect::from_center_size(pos, egui::vec2(1.0, 1.0));
            return;
        }
        let tol = 6.0 / self.view.scale();
        let rot = 24.0 / self.view.scale();
        let hit = match &self.active {
            Active::Select(op) if op.established() => op.geom.hit(p, tol, rot),
            _ => None,
        };
        if let Some(hit) = hit {
            if let Active::Select(op) = &mut self.active {
                op.drag = Some(SelDrag::Box { hit, start: p, orig: op.geom, lift: Some(m.command) });
            }
            return;
        }
        self.commit_interaction();
        self.active = Active::Select(self.new_select_op(p));
    }

    pub fn select_pointer_move(&mut self, op: &mut SelectOp, p: Point, m: Modifiers) {
        match op.drag.clone() {
            Some(SelDrag::Create) => {
                let (w, h) = (self.doc.width() as f32, self.doc.height() as f32);
                let p = Point::new(p.x.clamp(0.0, w), p.y.clamp(0.0, h));
                let p = if m.shift && op.mode == SelectMode::Rectangle {
                    let (dx, dy) = (p.x - op.start.x, p.y - op.start.y);
                    let s = dx.abs().max(dy.abs());
                    Point::new(op.start.x + s * dx.signum(), op.start.y + s * dy.signum())
                } else {
                    p
                };
                op.current = p;
                if op.mode == SelectMode::FreeForm && op.lasso.last().is_none_or(|l| l.distance(p) >= 1.0) {
                    op.lasso.push(p);
                }
            }
            Some(SelDrag::Box { hit, start, orig, lift }) => {
                if let Some(copy) = lift {
                    if copy && op.floating.is_some() {
                        // Ctrl+drag a floating selection: leave a copy behind.
                        self.stamp_floating(op);
                    }
                    self.ensure_floating(op, copy);
                    op.drag = Some(SelDrag::Box { hit, start, orig, lift: None });
                }
                // Moves snap to whole pixels so the content is never resampled.
                let p = if hit == BoxHit::Inside {
                    Point::new(start.x + (p.x - start.x).round(), start.y + (p.y - start.y).round())
                } else {
                    p
                };
                op.geom = orig.drag(hit, start, p, m.shift);
                let d = op.update();
                self.preview_dirty.add(d);
            }
            None => {}
        }
    }

    pub fn select_pointer_up(&mut self, op: &mut SelectOp, _p: Point, _b: PointerButton) {
        if let Some(SelDrag::Create) = op.drag.take() {
            let (w, h) = op.canvas;
            op.mask = match op.mode {
                SelectMode::Rectangle => {
                    let r = PixelRect::new(
                        op.start.x.min(op.current.x).round() as i32,
                        op.start.y.min(op.current.y).round() as i32,
                        op.start.x.max(op.current.x).round() as i32,
                        op.start.y.max(op.current.y).round() as i32,
                    )
                    .clamp_to(w, h);
                    (!r.is_empty()).then(|| SelectionMask::rect(r))
                }
                SelectMode::FreeForm => SelectionMask::polygon(&op.lasso, w, h),
            };
            if let Some(mask) = &op.mask {
                let r = mask.rect;
                op.geom = BoxGeom::new(RectF::from_xywh(r.x0 as f32, r.y0 as f32, r.width() as f32, r.height() as f32));
            }
        }
    }

    /// Lifts the selected pixels into a floating layer (leaving a transparent hole unless `copy`).
    fn ensure_floating(&mut self, op: &mut SelectOp, copy: bool) {
        if op.floating.is_some() {
            return;
        }
        let Some(mask) = op.mask.clone() else { return };
        let Some(original) = mask.extract(&self.doc.active_layer().pixels) else { return };
        let mut changes = Vec::new();
        if !copy && let Some(c) = self.doc.edit_active(mask.rect, |layer| mask.clear(layer)) {
            changes.push(c);
        }
        let original = Arc::new(original);
        let pixels = self.keyed(&original);
        op.floating = Some(Floating { original, pixels, changes });
        let d = op.update();
        self.preview_dirty.add(d);
    }

    fn keyed(&self, original: &Arc<Pixmap>) -> Arc<Pixmap> {
        if !self.tools.transparent_selection {
            return original.clone();
        }
        let mut p = (**original).clone();
        key_out(&mut p, self.colors.secondary);
        Arc::new(p)
    }

    /// Draws the floating pixels into the layer without ending the selection.
    fn stamp_floating(&mut self, op: &mut SelectOp) {
        let Some(r) = op.rendered() else { return };
        if let Some(c) = self.doc.edit_active(r.bounds(), |layer| r.draw_into(layer, (0, 0)))
            && let Some(f) = &mut op.floating
        {
            f.changes.push(c);
        }
    }

    pub fn commit_selection(&mut self, mut op: SelectOp) {
        if op.floating.is_none() {
            return;
        }
        self.stamp_floating(&mut op);
        self.preview_dirty.add(op.bounds());
        if let Some(f) = op.floating.take() {
            self.history.push("selection", f.changes);
        }
    }

    pub fn cancel_selection(&mut self, mut op: SelectOp) {
        if let Some(f) = op.floating.take() {
            for mut c in f.changes.into_iter().rev() {
                c.apply(&mut self.doc);
            }
        }
        self.preview_dirty.add(op.bounds());
    }

    pub fn has_selection(&self) -> bool {
        matches!(&self.active, Active::Select(op) if op.established())
    }

    pub fn selection_size(&self) -> Option<(u32, u32)> {
        match &self.active {
            Active::Select(op) if op.established() => Some((op.geom.rect.width().round() as u32, op.geom.rect.height().round() as u32)),
            Active::Select(op) if op.drag.is_some() => {
                let r = RectF::from_points(op.start, op.current);
                Some((r.width().round() as u32, r.height().round() as u32))
            }
            _ => None,
        }
    }

    pub fn select_cursor(&self, pos: Pos2) -> egui::CursorIcon {
        if let Active::Select(op) = &self.active
            && op.established()
        {
            let p = self.view.to_canvas(pos);
            if let Some(h) = op.geom.hit(p, 6.0 / self.view.scale(), 24.0 / self.view.scale()) {
                return h.cursor(op.geom.angle);
            }
        }
        egui::CursorIcon::Crosshair
    }

    /// Selected pixels of the active layer as it looks, vector objects included (or the floating
    /// content).
    fn selected_pixels(&self) -> Option<Pixmap> {
        let Active::Select(op) = &self.active else { return None };
        if let Some(f) = &op.floating {
            return Some(match op.exact {
                Some(_) => (*f.pixels).clone(),
                None => op.rendered()?.to_pixmap(),
            });
        }
        op.mask.as_ref()?.extract(&self.doc.layer_image(self.doc.active_index()))
    }

    pub fn copy(&mut self) {
        if let Some(p) = self.selected_pixels() {
            crate::clipboard::set_image(&p);
        }
    }

    pub fn cut(&mut self) {
        if self.has_selection() {
            self.copy();
            self.delete_selection();
        }
    }

    pub fn copy_visible(&mut self) {
        let rect = match &self.active {
            Active::Select(op) if op.established() => op.pixel_rect().clamp_to(self.doc.width(), self.doc.height()),
            _ => self.doc.bounds(),
        };
        if !rect.is_empty() {
            crate::clipboard::set_image(&self.doc.flatten_region(rect, true));
        }
    }

    pub fn paste(&mut self) {
        if let Some(p) = crate::clipboard::get_image() {
            self.paste_pixmap(p);
        }
    }

    /// Pastes an image as a floating selection at the top-left of the visible canvas, growing the
    /// canvas when the image does not fit (as Paint offers to).
    pub fn paste_pixmap(&mut self, pixmap: Pixmap) {
        self.commit_interaction();
        let mut changes = Vec::new();
        if pixmap.width() > self.doc.width() || pixmap.height() > self.doc.height() {
            let before = self.doc.snapshot();
            self.doc.resize_canvas(self.doc.width().max(pixmap.width()), self.doc.height().max(pixmap.height()), 0, 0);
            changes.push(Change::document(before, &self.doc));
        }
        let (a, _) = self.view.visible_canvas();
        let x = a.x.max(0.0).floor().min((self.doc.width() - pixmap.width()) as f32);
        let y = a.y.max(0.0).floor().min((self.doc.height() - pixmap.height()) as f32);
        let rect = PixelRect::from_xywh(x as i32, y as i32, pixmap.width(), pixmap.height());
        let mut op = self.new_select_op(Point::new(x, y));
        op.drag = None;
        op.mask = Some(SelectionMask::rect(rect));
        op.geom = BoxGeom::new(RectF::from_xywh(x, y, pixmap.width() as f32, pixmap.height() as f32));
        let original = Arc::new(pixmap);
        let pixels = self.keyed(&original);
        op.floating = Some(Floating { original, pixels, changes });
        let d = op.update();
        self.preview_dirty.add(d);
        self.tool = Tool::Select;
        self.active = Active::Select(op);
    }

    pub fn select_all(&mut self) {
        self.commit_interaction();
        self.tool = Tool::Select;
        let mut op = self.new_select_op(Point::new(0.0, 0.0));
        op.drag = None;
        let r = self.doc.bounds();
        op.mask = Some(SelectionMask::rect(r));
        op.geom = BoxGeom::new(RectF::from_xywh(0.0, 0.0, r.width() as f32, r.height() as f32));
        self.active = Active::Select(op);
    }

    pub fn invert_selection(&mut self) {
        let mask = match &self.active {
            Active::Select(op) if op.floating.is_some() => Some(SelectionMask::rect(op.pixel_rect())),
            Active::Select(op) => op.mask.clone(),
            _ => None,
        };
        let Some(mask) = mask else { return };
        self.commit_interaction();
        let Some(inv) = mask.inverted(self.doc.width(), self.doc.height()) else { return };
        let mut op = self.new_select_op(Point::new(0.0, 0.0));
        op.drag = None;
        let r = inv.rect;
        op.geom = BoxGeom::new(RectF::from_xywh(r.x0 as f32, r.y0 as f32, r.width() as f32, r.height() as f32));
        op.mask = Some(inv);
        self.active = Active::Select(op);
    }

    pub fn delete_selection(&mut self) {
        // A shape that is still editable is discarded (a picked-up vector object is deleted).
        // Only taken out when it is a shape: anything else (a selection) stays for below.
        // A marked arrow point goes alone.
        if let Active::Shape(op) = &mut self.active
            && op.delete_selected_point()
        {
            return;
        }
        if matches!(self.active, Active::Shape(_)) {
            if let Active::Shape(op) = std::mem::take(&mut self.active) {
                self.delete_shape(op);
            }
            return;
        }
        if !matches!(self.active, Active::Select(_)) {
            return;
        }
        let Active::Select(mut op) = std::mem::take(&mut self.active) else { return };
        if let Some(f) = op.floating.take() {
            // The hole is already cut; dropping the floating pixels deletes them.
            self.preview_dirty.add(op.bounds());
            self.history.push("delete", f.changes);
        } else if let Some(mask) = &op.mask {
            let change = self.doc.edit_active(mask.rect, |layer| mask.clear(layer));
            self.history.push_one("delete", change);
        }
    }

    pub fn set_transparent_selection(&mut self, on: bool) {
        self.tools.transparent_selection = on;
        if let Active::Select(op) = &mut self.active
            && let Some(f) = &mut op.floating
        {
            f.pixels = if on {
                let mut p = (*f.original).clone();
                key_out(&mut p, self.colors.secondary);
                Arc::new(p)
            } else {
                f.original.clone()
            };
            let d = op.update();
            self.preview_dirty.add(d);
        }
    }

    pub fn crop_to_selection(&mut self) {
        let (rect, mask) = match &self.active {
            Active::Select(op) if op.established() => (op.pixel_rect().clamp_to(self.doc.width(), self.doc.height()), op.mask.clone()),
            _ => return,
        };
        self.commit_interaction();
        if rect.is_empty() {
            return;
        }
        let before = self.doc.snapshot();
        let keep_shape = mask.filter(|m| !m.is_rect() && m.rect == rect);
        // A rectangular crop only moves the content: vector objects move along and stay editable.
        if keep_shape.is_none() {
            self.doc.map_layers_shifted(-rect.x0, -rect.y0, |p| {
                raster::copy_region(p, rect).unwrap_or_else(|| raster::pixmap(rect.width(), rect.height()))
            });
            self.history.push("crop", vec![Change::document(before, &self.doc)]);
            return;
        }
        self.doc.map_layers(|p| {
            let mut out = raster::copy_region(p, rect).unwrap_or_else(|| raster::pixmap(rect.width(), rect.height()));
            if let Some(m) = &keep_shape {
                // Free-form crop: pixels outside the lasso become transparent.
                let local = m.inverted(p.width(), p.height());
                if let Some(local) = local {
                    let mut full = p.clone();
                    local.clear(&mut full);
                    out = raster::copy_region(&full, rect).unwrap_or(out);
                }
            }
            out
        });
        self.history.push("crop", vec![Change::document(before, &self.doc)]);
    }

    /// Applies a pixel transform (rotate, flip, invert...) to the floating selection, if any.
    pub fn transform_selection(&mut self, f: &dyn Fn(&Pixmap) -> Pixmap) -> bool {
        if !matches!(&self.active, Active::Select(op) if op.established()) {
            return false;
        }
        let Active::Select(mut op) = std::mem::take(&mut self.active) else { return false };
        self.ensure_floating(&mut op, false);
        if let Some(fl) = &mut op.floating {
            // Bake a pending stretch first so the transform applies to what the user sees.
            let (cw, ch) = (op.geom.rect.width().round().max(1.0) as u32, op.geom.rect.height().round().max(1.0) as u32);
            if (cw, ch) != (fl.original.width(), fl.original.height()) {
                fl.original = Arc::new(raster::resize(&fl.original, cw, ch));
            }
            fl.original = Arc::new(f(&fl.original));
            fl.pixels = self.keyed(&fl.original);
            let c = op.geom.rect.center();
            let (w, h) = (fl.original.width() as f32, fl.original.height() as f32);
            op.geom.rect = RectF::from_xywh((c.x - w / 2.0).round(), (c.y - h / 2.0).round(), w, h);
        }
        let d = op.update();
        self.preview_dirty.add(d);
        self.active = Active::Select(op);
        true
    }

    /// Size of the selection content (for the Resize dialog).
    pub fn selection_content_size(&self) -> Option<(u32, u32)> {
        match &self.active {
            Active::Select(op) if op.established() => Some((op.geom.rect.width().round() as u32, op.geom.rect.height().round() as u32)),
            _ => None,
        }
    }

    /// Arrow keys nudge the selection by one pixel.
    pub fn selection_keys(&mut self, ctx: &egui::Context) {
        if !self.has_selection() || ctx.egui_wants_keyboard_input() || self.ui.modal.is_some() {
            return;
        }
        let mut d = (0.0f32, 0.0f32);
        ctx.input_mut(|i| {
            for (key, v) in [
                (egui::Key::ArrowLeft, (-1.0, 0.0)),
                (egui::Key::ArrowRight, (1.0, 0.0)),
                (egui::Key::ArrowUp, (0.0, -1.0)),
                (egui::Key::ArrowDown, (0.0, 1.0)),
            ] {
                while i.consume_key(egui::Modifiers::NONE, key) {
                    d.0 += v.0;
                    d.1 += v.1;
                }
            }
        });
        if d == (0.0, 0.0) {
            return;
        }
        let Active::Select(mut op) = std::mem::take(&mut self.active) else { return };
        self.ensure_floating(&mut op, false);
        op.geom.rect = op.geom.rect.translate(d.0, d.1);
        let dirty = op.update();
        self.preview_dirty.add(dirty);
        self.active = Active::Select(op);
    }
}

/// Marquee / handles of the selection.
pub fn paint_overlay(op: &SelectOp, view: &View, painter: &egui::Painter, t: &Theme) {
    let dark = Color32::from_gray(20);
    match &op.drag {
        Some(SelDrag::Create) => {
            if op.mode == SelectMode::FreeForm {
                let pts: Vec<Pos2> = op.lasso.iter().map(|p| view.to_screen(*p)).collect();
                if pts.len() > 1 {
                    painter.add(egui::Shape::line(pts.clone(), Stroke::new(1.0, Color32::WHITE)));
                    painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.0, dark), 3.0, 3.0));
                }
            } else {
                let r = egui::Rect::from_two_pos(view.to_screen(op.start), view.to_screen(op.current));
                crate::ui::widgets::dashed_rect(painter, r, dark, Color32::WHITE);
            }
        }
        _ => {
            if op.floating.is_none()
                && op.mode == SelectMode::FreeForm
                && op.lasso.len() > 2
                && op.mask.as_ref().is_some_and(|m| !m.is_rect())
            {
                let mut pts: Vec<Pos2> = op.lasso.iter().map(|p| view.to_screen(*p)).collect();
                pts.push(pts[0]);
                painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.0, dark), 3.0, 3.0));
            }
            if op.established() {
                op.geom.paint(view, painter, t, true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floating_render_exact_and_scaled() {
        let mut p = raster::pixmap(4, 2);
        raster::fill_region(&mut p, PixelRect::from_size(4, 2), bgpaint_core::color::Color::BLACK);
        let g = BoxGeom::new(RectF::from_xywh(3.0, 5.0, 4.0, 2.0));
        let r = render_floating(&p, &g, (20, 20)).unwrap();
        assert_eq!(r.bounds(), PixelRect::new(3, 5, 7, 7));
        let g2 = BoxGeom::new(RectF::from_xywh(0.0, 0.0, 8.0, 4.0));
        let r2 = render_floating(&p, &g2, (20, 20)).unwrap();
        assert_eq!(r2.bounds(), PixelRect::new(0, 0, 8, 4));
        assert!(render_floating(&p, &BoxGeom::new(RectF::from_xywh(50.0, 50.0, 4.0, 2.0)), (20, 20)).is_none());
    }
}
