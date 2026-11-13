//! The Layers panel on the right: add button, layer thumbnails (top layer first) and the
//! background card at the bottom. Layers can be dragged to a new place in the stack.

use super::icons::regular as ic;
use super::menubar::MenuKind;
use super::widgets::{self, IconButton};
use crate::app::PaintApp;
use crate::commands::Command;
use bgpaint_core::document::{ActivePreview, Document, LayerId};
use bgpaint_core::geom::PixelRect;
use bgpaint_core::raster;
use bgpaint_core::tiny_skia::{Pixmap, Transform};
use egui::{Color32, CursorIcon, Id, Rect, Sense, Stroke, TextureHandle, Vec2, pos2, vec2};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const PANEL_WIDTH: f32 = 112.0;
/// The checkerboard behind layer thumbnails.
const CHECKER: [Color32; 2] = [Color32::from_gray(255), Color32::from_gray(204)];
const THUMB: f32 = 76.0;
/// Gap between two layer cards (the drop marker sits in its middle).
const GAP: f32 = 8.0;
/// Distance from the list's top or bottom edge where a drag scrolls the list.
const AUTO_SCROLL_EDGE: f32 = 28.0;
/// Opacity of the card that follows the pointer while a layer is dragged.
const GHOST_OPACITY: f32 = 0.6;

/// A layer being dragged in the panel.
#[derive(Clone, Copy)]
struct LayerDrag {
    layer: LayerId,
    /// Pointer position relative to the card's center when the drag started.
    grab: Vec2,
}

/// Largest tool preview (canvas pixels) drawn into its layer's thumbnail while in progress.
const PREVIEW_MAX_AREA: u64 = 4096 * 4096;

/// Cached layer thumbnails.
#[derive(Default)]
pub struct Thumbnails {
    textures: HashMap<LayerId, TextureHandle>,
    /// Layers whose thumbnail is out of date.
    stale: Vec<LayerId>,
    last: Option<Instant>,
    structure: u64,
    size: (u32, u32),
}

impl Thumbnails {
    /// Marks the thumbnail of `layer` out of date.
    pub fn touch(&mut self, layer: LayerId) {
        if !self.stale.contains(&layer) {
            self.stale.push(layer);
        }
    }

    /// `preview` is the tool in progress on the active layer (a picked-up object, a floating
    /// selection, a stroke...), shown in that layer's thumbnail as on the canvas.
    fn refresh(&mut self, ctx: &egui::Context, app_doc: &Document, preview: Option<&dyn ActivePreview>) {
        let structure_changed = self.structure != app_doc.structure_version() || self.size != (app_doc.width(), app_doc.height());
        let due = self.last.is_none_or(|t| t.elapsed() > Duration::from_millis(300));
        let stale = !self.stale.is_empty();
        if !(structure_changed || (stale && due)) {
            if stale {
                ctx.request_repaint_after(Duration::from_millis(320));
            }
            return;
        }
        self.structure = app_doc.structure_version();
        self.size = (app_doc.width(), app_doc.height());
        // After a structural change every thumbnail is redone, else only the layers that changed.
        let only = (!structure_changed).then(|| std::mem::take(&mut self.stale));
        self.stale.clear();
        self.last = Some(Instant::now());
        let (w, h) = fit(app_doc.width(), app_doc.height());
        let ids: Vec<LayerId> = app_doc.layers().iter().map(|l| l.id).collect();
        self.textures.retain(|id, _| ids.contains(id));
        let (sx, sy) = (w as f32 / app_doc.width() as f32, h as f32 / app_doc.height() as f32);
        let draw_scaled = |small: &mut Pixmap, src: &Pixmap, x: i32, y: i32| {
            let t = Transform::from_row(sx, 0.0, 0.0, sy, x as f32 * sx, y as f32 * sy);
            raster::draw_transformed(small, src, t, true);
        };
        let active = app_doc.active_index();
        for (i, layer) in app_doc.layers().iter().enumerate() {
            if only.as_ref().is_some_and(|only| !only.contains(&layer.id)) && self.textures.contains_key(&layer.id) {
                continue;
            }
            let preview =
                preview.filter(|_| i == active).map(|p| (p, p.bounds().intersect(&app_doc.bounds()))).filter(|(_, b)| !b.is_empty());
            // With its vector objects, as it looks on the canvas: drawn straight at thumbnail
            // size rather than on a full-size copy of the layer.
            let mut small = raster::thumbnail(&layer.pixels, w, h);
            // A preview drawn into the layer's pixels (below its objects) may also erase them, so
            // the thumbnail pixels it covers are recomputed from a copy of just that area. (Over a
            // huge area that copy is too slow to repeat; the thumbnail catches up once the tool is done.)
            if let Some((p, b)) = preview.filter(|(p, b)| !p.above_objects() && b.area() <= PREVIEW_MAX_AREA) {
                let tb = PixelRect::new(
                    (b.x0 as f32 * sx).floor() as i32,
                    (b.y0 as f32 * sy).floor() as i32,
                    (b.x1 as f32 * sx).ceil() as i32,
                    (b.y1 as f32 * sy).ceil() as i32,
                );
                let src = PixelRect::new(
                    (tb.x0 as f32 / sx).floor() as i32,
                    (tb.y0 as f32 / sy).floor() as i32,
                    (tb.x1 as f32 / sx).ceil() as i32,
                    (tb.y1 as f32 / sy).ceil() as i32,
                )
                .intersect(&app_doc.bounds());
                if let Some(mut region) = raster::copy_region(&layer.pixels, src) {
                    p.apply(&mut region, src);
                    raster::thumbnail_into(&mut small, &region, (src.x0, src.y0), (app_doc.width(), app_doc.height()), tb);
                }
            }
            for r in layer.objects.iter().filter_map(|o| o.raster()) {
                for (x, y, tile) in r.tiles() {
                    draw_scaled(&mut small, tile, x, y);
                }
            }
            if let Some((p, b)) = preview.filter(|(p, _)| p.above_objects()) {
                match p.rendered() {
                    // Scaled straight from the shape's tiles: the cost follows the thumbnail.
                    Some(r) => {
                        for (x, y, tile) in r.tiles() {
                            draw_scaled(&mut small, tile, x, y);
                        }
                    }
                    None if b.area() <= PREVIEW_MAX_AREA => {
                        let mut region = raster::pixmap(b.width(), b.height());
                        p.apply(&mut region, b);
                        draw_scaled(&mut small, &region, b.x0, b.y0);
                    }
                    None => {}
                }
            }
            let img = egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], small.data());
            match self.textures.get_mut(&layer.id) {
                Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                None => {
                    let t = ctx.load_texture(format!("layer-{}", layer.id.0), img, egui::TextureOptions::LINEAR);
                    self.textures.insert(layer.id, t);
                }
            }
        }
    }
}

fn fit(w: u32, h: u32) -> (u32, u32) {
    let s = (THUMB * 2.0) / w.max(h) as f32;
    (((w as f32 * s).round() as u32).max(1), ((h as f32 * s).round() as u32).max(1))
}

impl PaintApp {
    pub fn layers_panel(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        let ctx = ui.ctx().clone();
        self.thumbs.refresh(&ctx, &self.doc, self.active.preview());
        egui::Panel::right("layers")
            .exact_size(PANEL_WIDTH)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(t.canvas_area).inner_margin(egui::Margin { left: 0, right: 8, top: 8, bottom: 8 }))
            .show(ui, |ui| {
                let outer = ui.max_rect();
                ui.painter().rect(outer, 8, t.ribbon_bg, Stroke::new(1.0, t.ribbon_border), egui::StrokeKind::Inside);
                // Add layer
                let add = Rect::from_center_size(pos2(outer.center().x, outer.top() + 20.0), vec2(36.0, 30.0));
                if IconButton::new(ic::ADD_CIRCLE)
                    .size(18.0)
                    .tooltip(self.tr.add_layer)
                    .show(ui, &t, add, Id::unique("add-layer"))
                    .clicked()
                {
                    self.execute(&ctx, Command::AddLayer);
                }
                // Background card at the bottom
                let bg_rect = Rect::from_center_size(
                    pos2(outer.center().x, outer.bottom() - 12.0 - THUMB / 2.0 - 6.0),
                    vec2(THUMB + 8.0, THUMB + 8.0),
                );
                ui.painter().hline(outer.left() + 8.0..=outer.right() - 8.0, bg_rect.top() - 10.0, Stroke::new(1.0, t.separator));
                self.background_card(ui, &t, bg_rect);

                // Layer list, top layer first, scrollable.
                let list = Rect::from_min_max(pos2(outer.left(), add.bottom() + 8.0), pos2(outer.right(), bg_rect.top() - 18.0));
                let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list));
                egui::ScrollArea::vertical().id_salt("layers-scroll").show(&mut list_ui, |ui| {
                    ui.set_width(list.width());
                    let drag_id = Id::unique("layer-drag");
                    let drag: Option<LayerDrag> = ctx.data(|d| d.get_temp(drag_id));
                    let dragging = drag.map(|d| d.layer);
                    let n = self.doc.layers().len();
                    // Display order, top layer first: (layer index, card response).
                    let mut rows = Vec::with_capacity(n);
                    for i in (0..n).rev() {
                        let id = self.doc.layers()[i].id;
                        let resp = self.layer_card(ui, &t, i, dragging);
                        if resp.drag_started() {
                            let grab = ctx.input(|i| i.pointer.press_origin()).map_or(Vec2::ZERO, |p| p - resp.rect.center());
                            ctx.data_mut(|d| d.insert_temp(drag_id, LayerDrag { layer: id, grab }));
                            self.execute(&ctx, Command::SelectLayer(i));
                        }
                        rows.push((i, resp));
                        ui.add_space(GAP);
                    }
                    let Some(drag) = drag else { return };
                    let Some(r0) = rows.iter().position(|(i, _)| self.doc.layers()[*i].id == drag.layer) else {
                        ctx.data_mut(|d| d.remove::<LayerDrag>(drag_id));
                        return;
                    };
                    ctx.set_cursor_icon(CursorIcon::Grabbing);
                    let pointer = ctx.input(|i| i.pointer.latest_pos());
                    let drop = pointer.and_then(|p| drop_target(&rows, r0, p.y));
                    if let Some((slot, _)) = drop {
                        paint_drop_marker(ui, &t, &rows, slot, list);
                    }
                    // A faded copy of the card follows the pointer up and down the list.
                    if let Some(p) = pointer {
                        let y = (p.y - drag.grab.y).clamp(list.top(), list.bottom());
                        let card = Rect::from_center_size(pos2(rows[r0].1.rect.center().x, y), vec2(THUMB + 8.0, THUMB + 8.0));
                        self.paint_drag_ghost(&ctx, &t, rows[r0].0, card);
                    }
                    // Scroll while holding the layer near the list's top or bottom edge.
                    if let Some(p) = pointer {
                        let speed = if p.y < list.top() + AUTO_SCROLL_EDGE {
                            8.0
                        } else if p.y > list.bottom() - AUTO_SCROLL_EDGE {
                            -8.0
                        } else {
                            0.0
                        };
                        if speed != 0.0 {
                            ui.scroll_with_delta(vec2(0.0, speed));
                            ctx.request_repaint();
                        }
                    }
                    let stopped = rows[r0].1.drag_stopped() || !ctx.input(|i| i.pointer.any_down());
                    if stopped {
                        ctx.data_mut(|d| d.remove::<LayerDrag>(drag_id));
                        if let Some((_, to)) = drop {
                            self.execute(&ctx, Command::ReorderLayer(rows[r0].0, to));
                        }
                    }
                });
            });
    }

    /// Draws one layer card and returns its response (clicks select, drags reorder).
    fn layer_card(&mut self, ui: &mut egui::Ui, t: &crate::theme::Theme, i: usize, dragging: Option<LayerId>) -> egui::Response {
        let ctx = ui.ctx().clone();
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), THUMB + 8.0), Sense::click_and_drag());
        let card = Rect::from_center_size(rect.center(), vec2(THUMB + 8.0, THUMB + 8.0));
        let layer = &self.doc.layers()[i];
        let active = i == self.doc.active_index();
        let visible = layer.visible;
        let id = layer.id;
        let lifted = dragging == Some(id);
        let p = ui.painter();
        if active {
            p.rect(card, 6, t.hover, Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
        } else if resp.hovered() && dragging.is_none() {
            p.rect_filled(card, 6, t.hover);
        }
        let img = self.thumb_rect(card);
        self.checker.paint(&ctx, p, img, CHECKER);
        if let Some(tex) = self.thumbs.textures.get(&id) {
            p.image(
                tex.id(),
                img,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                if visible { Color32::WHITE } else { Color32::from_white_alpha(90) },
            );
        }
        // The layer being dragged stays faded in its old place until it is dropped.
        if lifted {
            p.rect_filled(card, 6, t.ribbon_bg.gamma_multiply(0.6));
        }
        // Visibility toggle (shown on hover or when hidden).
        let eye = Rect::from_center_size(pos2(card.right() - 14.0, card.top() + 14.0), vec2(22.0, 22.0));
        let eye_resp = ui.interact(eye, Id::unique(("eye", id.0)), Sense::click());
        if (dragging.is_none() && (resp.hovered() || eye_resp.hovered())) || !visible {
            ui.painter().rect_filled(eye, 4, t.panel_bg.gamma_multiply(0.9));
            widgets::paint_icon(ui.painter(), eye.center(), if visible { ic::EYE } else { ic::EYE_OFF }, 14.0, t.text);
        }
        let tip = format!("{} {}", self.tr.layer_n, self.doc.layers()[i].name);
        if eye_resp.on_hover_text(if visible { self.tr.hide_layer } else { self.tr.show_layer }).clicked() {
            self.execute(&ctx, Command::ToggleLayerVisible(i));
        } else if resp.clicked() {
            self.execute(&ctx, Command::SelectLayer(i));
        }
        if resp.secondary_clicked() {
            self.execute(&ctx, Command::SelectLayer(i));
            self.ui.open_menu = Some(MenuKind::Layer(i));
            self.ui.menu_anchor = Rect::from_center_size(ctx.pointer_interact_pos().unwrap_or(card.center()), vec2(1.0, 1.0));
        }
        if dragging.is_none() { resp.on_hover_text(tip) } else { resp }
    }

    /// Where a layer's thumbnail sits inside its card.
    fn thumb_rect(&self, card: Rect) -> Rect {
        let inner = card.shrink(6.0);
        let (w, h) = fit(self.doc.width(), self.doc.height());
        let s = (inner.width() / w as f32).min(inner.height() / h as f32);
        Rect::from_center_size(inner.center(), vec2(w as f32 * s, h as f32 * s))
    }

    /// The semi-transparent card drawn under the pointer while layer `i` is dragged.
    fn paint_drag_ghost(&mut self, ctx: &egui::Context, t: &crate::theme::Theme, i: usize, card: Rect) {
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::unique("layer-drag-ghost")));
        let fade = |c: Color32| c.gamma_multiply(GHOST_OPACITY);
        p.rect(card, 6, fade(t.hover), Stroke::new(1.5, fade(t.accent)), egui::StrokeKind::Inside);
        let img = self.thumb_rect(card);
        self.checker.paint_tinted(ctx, &p, img, CHECKER, fade(Color32::WHITE));
        let layer = &self.doc.layers()[i];
        if let Some(tex) = self.thumbs.textures.get(&layer.id) {
            let tint = if layer.visible { Color32::WHITE } else { Color32::from_white_alpha(90) };
            p.image(tex.id(), img, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), fade(tint));
        }
    }

    fn background_card(&mut self, ui: &mut egui::Ui, t: &crate::theme::Theme, card: Rect) {
        let ctx = ui.ctx().clone();
        let resp = ui.interact(card, Id::unique("bg-card"), Sense::click());
        let bg = self.doc.background();
        if resp.hovered() {
            ui.painter().rect_filled(card, 6, t.hover);
        }
        let inner = card.shrink(8.0);
        let (w, h) = fit(self.doc.width(), self.doc.height());
        let s = (inner.width() / w as f32).min(inner.height() / h as f32);
        let img = Rect::from_center_size(inner.center(), vec2(w as f32 * s, h as f32 * s));
        if bg.visible {
            ui.painter().rect_filled(img, 0, super::ribbon::to_egui(bg.color));
        } else {
            self.checker.paint(&ctx, ui.painter(), img, CHECKER);
            widgets::paint_icon(ui.painter(), pos2(card.right() - 14.0, card.top() + 14.0), ic::EYE_OFF, 14.0, t.text);
        }
        ui.painter().rect_stroke(img, 0, Stroke::new(1.0, t.separator), egui::StrokeKind::Outside);
        let resp = resp.on_hover_text(self.tr.background);
        if resp.secondary_clicked() || resp.clicked() {
            self.ui.open_menu = Some(MenuKind::Background);
            self.ui.menu_anchor = Rect::from_center_size(ctx.pointer_interact_pos().unwrap_or(card.center()), vec2(1.0, 1.0));
        }
    }
}

/// Where a layer dragged from display row `r0` lands when the pointer is at height `y`:
/// `(slot, layer index)`, where `slot` is the gap (0 = above the first row) the marker is
/// drawn in. `None` when the drop would leave the layer where it is.
fn drop_target(rows: &[(usize, egui::Response)], r0: usize, y: f32) -> Option<(usize, usize)> {
    let n = rows.len();
    let slot = rows.iter().filter(|(_, r)| r.rect.center().y < y).count();
    if slot == r0 || slot == r0 + 1 {
        return None;
    }
    let row = if slot > r0 { slot - 1 } else { slot };
    Some((slot, n - 1 - row))
}

/// The accent line in the gap where the dragged layer will be dropped.
fn paint_drop_marker(ui: &egui::Ui, t: &crate::theme::Theme, rows: &[(usize, egui::Response)], slot: usize, list: Rect) {
    let y = match slot {
        0 => rows[0].1.rect.top() - GAP / 2.0,
        s if s == rows.len() => rows[s - 1].1.rect.bottom() + GAP / 2.0,
        s => (rows[s - 1].1.rect.bottom() + rows[s].1.rect.top()) / 2.0,
    };
    let half = (THUMB + 8.0) / 2.0;
    let x = rows[0].1.rect.center().x;
    // Not clipped to the scroll area: the slot above the first card sits on its edge.
    let painter = egui::Painter::new(ui.ctx().clone(), ui.layer_id(), list.expand(GAP));
    let stroke = Stroke::new(2.5, t.accent);
    painter.line_segment([pos2(x - half + 3.0, y), pos2(x + half - 3.0, y)], stroke);
    painter.circle_stroke(pos2(x - half, y), 3.0, stroke);
    painter.circle_stroke(pos2(x + half, y), 3.0, stroke);
}
