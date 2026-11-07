//! The Settings dialog (theme and "About").

use super::dialogs::{self, Footer, Modal, dialog};
use super::widgets;
use crate::app::PaintApp;
use crate::settings::ThemeMode;
use crate::theme::Theme;
use egui::{RichText, Stroke};

pub const REPO_URL: &str = "https://github.com/belizariogr/bgpaint";

impl PaintApp {
    pub fn settings_dialog(&mut self, ctx: &egui::Context, t: &Theme) {
        let tr = self.tr;
        let mut theme = self.settings.theme;
        let r = dialog(ctx, t, "settings-dialog", tr.settings, 440.0, (tr.close, None, None), |ui| {
            dialogs::section(ui, t, tr.app_theme);
            for (mode, label) in
                [(ThemeMode::Light, tr.theme_light), (ThemeMode::Dark, tr.theme_dark), (ThemeMode::System, tr.theme_system)]
            {
                if widgets::radio(ui, t, theme == mode, label).clicked() {
                    theme = mode;
                }
            }
            separator(ui, t);
            self.update_settings(ui, t);
            separator(ui, t);
            dialogs::section(ui, t, tr.about);
            ui.label(RichText::new(format!("BGPaint {}", env!("CARGO_PKG_VERSION"))).size(14.0).color(t.text));
            ui.label(RichText::new(tr.about_text).size(13.0).color(t.text_secondary));
            ui.label(RichText::new("© 2026 Belizario — MIT").size(13.0).color(t.text_secondary));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                for (label, url) in [
                    (tr.source_code, REPO_URL.to_owned()),
                    (tr.license, format!("{REPO_URL}/blob/main/LICENSE")),
                    (tr.third_party, format!("{REPO_URL}/blob/main/THIRD_PARTY.md")),
                ] {
                    let resp = ui.add(egui::Label::new(RichText::new(label).size(13.0).color(t.accent)).sense(egui::Sense::click()));
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        crate::platform::open_url(&url);
                    }
                }
            });
            ui.add_space(8.0);
        });
        if theme != self.settings.theme {
            self.set_theme_mode(ctx, theme);
        }
        // The update dialog may have taken over (see `PaintApp::poll_updater`).
        if r == Footer::None && self.ui.modal.is_none() {
            self.ui.modal = Some(Modal::Settings);
        }
    }
}

fn separator(ui: &mut egui::Ui, t: &Theme) {
    ui.add_space(10.0);
    let (line, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(line.x_range(), line.center().y, Stroke::new(1.0, t.separator));
    ui.add_space(6.0);
}
