//! The Settings page (theme and "About").

use super::icons::regular as ic;
use super::widgets::{self, IconButton};
use crate::app::PaintApp;
use crate::commands::Command;
use crate::settings::ThemeMode;
use crate::theme;
use egui::{Align2, FontId, Id, Rect, RichText, Stroke, pos2, vec2};

pub const REPO_URL: &str = "https://github.com/belizariogr/bgpaint";

impl PaintApp {
    pub fn settings_page(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        let tr = self.tr;
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.window_bg)).show(ui, |ui| {
            let r = ui.max_rect();
            let back = Rect::from_min_size(pos2(r.left() + 8.0, r.top() + 12.0), vec2(36.0, 32.0));
            if IconButton::new(ic::BACK).size(18.0).show(ui, &t, back, Id::unique("settings-back")).clicked()
                || ctx.input(|i| i.key_pressed(egui::Key::Escape))
            {
                self.execute(&ctx, Command::CloseSettings);
            }
            ui.painter().text(
                pos2(r.left() + 52.0, back.center().y),
                Align2::LEFT_CENTER,
                tr.settings,
                FontId::new(20.0, theme::semibold()),
                t.text,
            );

            let col_w = ((r.width() - 120.0) * 0.62).clamp(360.0, 780.0);
            let left = Rect::from_min_size(pos2(r.left() + 52.0, r.top() + 70.0), vec2(col_w, r.height() - 90.0));
            let right = Rect::from_min_size(pos2(left.right() + 44.0, left.top()), vec2(320.0, left.height()));

            let mut lui = ui.new_child(egui::UiBuilder::new().max_rect(left));
            lui.label(RichText::new(tr.appearance).font(FontId::new(14.0, theme::semibold())).color(t.text));
            lui.add_space(6.0);
            let card = Rect::from_min_size(lui.cursor().min, vec2(col_w, 48.0 + 3.0 * 36.0 + 12.0));
            lui.painter().rect(card, 6, t.ribbon_bg, Stroke::new(1.0, t.ribbon_border), egui::StrokeKind::Inside);
            lui.painter().hline(card.x_range(), card.top() + 48.0, Stroke::new(1.0, t.ribbon_border));
            widgets::paint_icon(lui.painter(), pos2(card.left() + 22.0, card.top() + 24.0), ic::BRUSH, 18.0, t.text);
            widgets::label(lui.painter(), pos2(card.left() + 48.0, card.top() + 24.0), Align2::LEFT_CENTER, tr.app_theme, 14.0, t.text);
            let mut y = card.top() + 54.0;
            for (mode, label) in
                [(ThemeMode::Light, tr.theme_light), (ThemeMode::Dark, tr.theme_dark), (ThemeMode::System, tr.theme_system)]
            {
                let rect = Rect::from_min_size(pos2(card.left() + 48.0, y), vec2(col_w - 60.0, 36.0));
                let mut rui = lui.new_child(egui::UiBuilder::new().max_rect(rect));
                if widgets::radio(&mut rui, &t, self.settings.theme == mode, label).clicked() {
                    self.set_theme_mode(&ctx, mode);
                }
                y += 36.0;
            }

            let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(right));
            rui.label(RichText::new(tr.about).font(FontId::new(14.0, theme::semibold())).color(t.text));
            rui.add_space(4.0);
            rui.label(RichText::new(format!("BGPaint {}", env!("CARGO_PKG_VERSION"))).size(13.0).color(t.text));
            rui.label(RichText::new(tr.about_text).size(13.0).color(t.text_secondary));
            rui.label(RichText::new("© 2026 Belizario — MIT").size(13.0).color(t.text_secondary));
            rui.add_space(12.0);
            for (label, url) in [
                (tr.source_code, REPO_URL.to_owned()),
                (tr.license, format!("{REPO_URL}/blob/main/LICENSE")),
                (tr.third_party, format!("{REPO_URL}/blob/main/THIRD_PARTY.md")),
            ] {
                let resp = rui.add(egui::Label::new(RichText::new(label).size(13.0).color(t.accent)).sense(egui::Sense::click()));
                if resp.hovered() {
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    crate::platform::open_url(&url);
                }
                rui.add_space(4.0);
            }
        });
    }
}
