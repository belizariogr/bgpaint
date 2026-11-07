//! Bottom status bar: cursor position, selection size, canvas size and zoom controls.

use super::icons::regular as ic;
use super::menubar::MenuKind;
use super::widgets::{self, IconButton};
use crate::app::PaintApp;
use crate::canvas::view::{MAX_ZOOM, MIN_ZOOM};
use crate::commands::Command;
use egui::{Align2, Id, Rect, Sense, pos2, vec2};

pub const STATUS_HEIGHT: f32 = 34.0;

impl PaintApp {
    pub fn statusbar(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        let tr = self.tr;
        egui::Panel::bottom("statusbar")
            .exact_size(STATUS_HEIGHT)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(t.window_bg))
            .show(ui, |ui| {
                let r = ui.max_rect();
                let cy = r.center().y;
                let p = ui.painter().clone();
                let item = |x: f32, glyph: char, text: Option<String>, tip: &str, ui: &egui::Ui| {
                    widgets::paint_icon(&p, pos2(r.left() + x, cy), glyph, 16.0, t.text);
                    if let Some(text) = text {
                        widgets::label(&p, pos2(r.left() + x + 14.0, cy), Align2::LEFT_CENTER, &text, 12.0, t.text);
                    }
                    let hit = Rect::from_min_size(pos2(r.left() + x - 10.0, r.top()), vec2(110.0, r.height()));
                    ui.interact(hit, Id::unique(("status", tip)), Sense::hover()).on_hover_text(tip);
                };
                item(16.0, ic::CURSOR, self.status.cursor.map(|(x, y)| format!("{x}, {y}px")), tr.cursor_position, ui);
                widgets::vseparator(&p, &t, r.left() + 131.0, cy - 9.0, cy + 9.0);
                item(146.0, ic::SELECTION_SIZE, self.status.selection.map(|(w, h)| format!("{w} × {h}px")), tr.selection_size, ui);
                widgets::vseparator(&p, &t, r.left() + 258.0, cy - 9.0, cy + 9.0);
                item(275.0, ic::CANVAS_SIZE, Some(format!("{} × {}px", self.doc.width(), self.doc.height())), tr.canvas_size, ui);
                if let Some(size) = self.file.disk_size {
                    widgets::vseparator(&p, &t, r.left() + 420.0, cy - 9.0, cy + 9.0);
                    widgets::label(&p, pos2(r.left() + 436.0, cy), Align2::LEFT_CENTER, &human_size(size), 12.0, t.text);
                }

                // Zoom controls, right aligned.
                let right = r.right();
                let fit = Rect::from_center_size(pos2(right - 373.0, cy), vec2(32.0, 28.0));
                if IconButton::new(ic::ZOOM_FIT).size(18.0).tooltip(tr.fit_to_window).show(ui, &t, fit, Id::unique("st-fit")).clicked() {
                    self.execute(ui.ctx(), Command::ZoomFit);
                }
                let combo = Rect::from_min_max(pos2(right - 350.0, cy - 15.0), pos2(right - 252.0, cy + 15.0));
                let resp = ui.interact(combo, Id::unique("st-zoom-combo"), Sense::click());
                let open = self.ui.open_menu == Some(MenuKind::Zoom);
                widgets::paint_combo_box(ui, &t, combo, &resp, &self.view.zoom_percent(), 12.0, open);
                if resp.on_hover_text(tr.zoom_level).clicked() {
                    self.toggle_menu(MenuKind::Zoom, combo);
                }
                let zo = Rect::from_center_size(pos2(right - 233.0, cy), vec2(28.0, 28.0));
                if IconButton::new(ic::ZOOM_OUT).size(16.0).tooltip(tr.zoom_out).show(ui, &t, zo, Id::unique("st-zo")).clicked() {
                    self.execute(ui.ctx(), Command::ZoomOut);
                }
                let slider = Rect::from_min_max(pos2(right - 220.0, cy - 12.0), pos2(right - 78.0, cy + 12.0));
                let (lmin, lmax) = (MIN_ZOOM.ln(), MAX_ZOOM.ln());
                let mut v = (self.view.zoom.ln() - lmin) / (lmax - lmin);
                let before = v;
                let sresp = widgets::slider(ui, &t, slider, Id::unique("st-zoom-slider"), &mut v, false);
                if (v - before).abs() > f32::EPSILON && (sresp.dragged() || sresp.clicked()) {
                    let z = (lmin + v * (lmax - lmin)).exp();
                    self.view.zoom_center(z);
                }
                let zi = Rect::from_center_size(pos2(right - 62.0, cy), vec2(28.0, 28.0));
                if IconButton::new(ic::ZOOM_IN).size(16.0).tooltip(tr.zoom_in).show(ui, &t, zi, Id::unique("st-zi")).clicked() {
                    self.execute(ui.ctx(), Command::ZoomIn);
                }
            });
    }
}

pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{:.1} MB", b / KB / KB)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sizes() {
        assert_eq!(super::human_size(10), "10 B");
        assert_eq!(super::human_size(2048), "2.0 KB");
        assert_eq!(super::human_size(5 * 1024 * 1024), "5.0 MB");
    }
}
