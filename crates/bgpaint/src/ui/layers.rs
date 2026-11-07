//! The Layers panel on the right: add button, layer thumbnails (top layer first) and the
//! background card at the bottom.

use super::icons::regular as ic;
use super::menubar::MenuKind;
use super::widgets::{self, IconButton};
use crate::app::PaintApp;
use crate::commands::Command;
use bgpaint_core::document::LayerId;
use bgpaint_core::raster;
use egui::{Color32, Id, Rect, Sense, Stroke, TextureHandle, pos2, vec2};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const PANEL_WIDTH: f32 = 112.0;
const THUMB: f32 = 76.0;

/// Cached layer thumbnails.
#[derive(Default)]
pub struct Thumbnails {
    textures: HashMap<LayerId, TextureHandle>,
    pub stale: bool,
    last: Option<Instant>,
    structure: u64,
    size: (u32, u32),
}

impl Thumbnails {
    fn refresh(&mut self, ctx: &egui::Context, app_doc: &bgpaint_core::document::Document) {
        let structure_changed = self.structure != app_doc.structure_version() || self.size != (app_doc.width(), app_doc.height());
        let due = self.last.is_none_or(|t| t.elapsed() > Duration::from_millis(300));
        if !(structure_changed || (self.stale && due)) {
            if self.stale {
                ctx.request_repaint_after(Duration::from_millis(320));
            }
            return;
        }
        self.structure = app_doc.structure_version();
        self.size = (app_doc.width(), app_doc.height());
        self.stale = false;
        self.last = Some(Instant::now());
        let (w, h) = fit(app_doc.width(), app_doc.height());
        let ids: Vec<LayerId> = app_doc.layers().iter().map(|l| l.id).collect();
        self.textures.retain(|id, _| ids.contains(id));
        for layer in app_doc.layers() {
            let small = raster::thumbnail(&layer.pixels, w, h);
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
        self.thumbs.refresh(&ctx, &self.doc);
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
                    let n = self.doc.layers().len();
                    for i in (0..n).rev() {
                        self.layer_card(ui, &t, i);
                        ui.add_space(8.0);
                    }
                });
            });
    }

    fn layer_card(&mut self, ui: &mut egui::Ui, t: &crate::theme::Theme, i: usize) {
        let ctx = ui.ctx().clone();
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), THUMB + 8.0), Sense::click());
        let card = Rect::from_center_size(rect.center(), vec2(THUMB + 8.0, THUMB + 8.0));
        let layer = &self.doc.layers()[i];
        let active = i == self.doc.active_index();
        let visible = layer.visible;
        let id = layer.id;
        let p = ui.painter();
        if active {
            p.rect(card, 6, t.hover, Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
        } else if resp.hovered() {
            p.rect_filled(card, 6, t.hover);
        }
        let inner = card.shrink(6.0);
        let (w, h) = fit(self.doc.width(), self.doc.height());
        let s = (inner.width() / w as f32).min(inner.height() / h as f32);
        let img = Rect::from_center_size(inner.center(), vec2(w as f32 * s, h as f32 * s));
        self.checker.paint(&ctx, p, img, [Color32::from_gray(255), Color32::from_gray(204)]);
        if let Some(tex) = self.thumbs.textures.get(&id) {
            p.image(
                tex.id(),
                img,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                if visible { Color32::WHITE } else { Color32::from_white_alpha(90) },
            );
        }
        // Visibility toggle (shown on hover or when hidden).
        let eye = Rect::from_center_size(pos2(card.right() - 14.0, card.top() + 14.0), vec2(22.0, 22.0));
        let eye_resp = ui.interact(eye, Id::unique(("eye", id.0)), Sense::click());
        if resp.hovered() || eye_resp.hovered() || !visible {
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
        resp.on_hover_text(tip);
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
            self.checker.paint(&ctx, ui.painter(), img, [Color32::from_gray(255), Color32::from_gray(204)]);
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
