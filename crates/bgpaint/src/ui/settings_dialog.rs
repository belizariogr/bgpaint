//! The Settings dialog (theme, language, updates and "About").

use super::dialogs::{self, Footer, Modal, dialog};
use super::menu::{self, Entry};
use super::widgets;
use crate::app::PaintApp;
use crate::i18n::LANGUAGES;
use crate::settings::ThemeMode;
use crate::theme::Theme;
use egui::{Id, Rect, RichText, Stroke, pos2};

pub const REPO_URL: &str = "https://github.com/belizariogr/bgpaint";

impl PaintApp {
    pub fn settings_dialog(&mut self, ctx: &egui::Context, t: &Theme) {
        let tr = self.tr;
        let mut theme = self.settings.theme;
        let open_id = Id::unique("settings-language-open");
        let mut language_open = ctx.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
        let mut combo = Rect::NOTHING;
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
            dialogs::section(ui, t, tr.language);
            let current = self.settings.language.as_deref().and_then(|c| LANGUAGES.iter().find(|(code, _)| *code == c));
            let resp = widgets::combo_box(ui, t, current.map_or(tr.theme_system, |(_, name)| name), 260.0);
            if resp.clicked() {
                language_open = !language_open;
            }
            combo = resp.rect;
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
        if language_open && r == Footer::None && self.ui.modal.is_none() {
            let mut entries = vec![Entry::item(None, tr.theme_system, None).marked(self.settings.language.is_none())];
            entries.extend(
                LANGUAGES
                    .iter()
                    .map(|&(code, name)| Entry::item(None, name, Some(code)).marked(self.settings.language.as_deref() == Some(code))),
            );
            let out = menu::show(ctx, t, Id::unique("settings-language-menu"), pos2(combo.left(), combo.bottom() + 4.0), &entries);
            if let Some(code) = out.action {
                language_open = false;
                self.set_language(ctx, code);
            } else if ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer.interact_pos().is_some_and(|p| !out.rects.iter().any(|r| r.contains(p)) && !combo.contains(p))
            }) {
                language_open = false;
            }
        } else {
            language_open = false;
        }
        ctx.data_mut(|d| d.insert_temp(open_id, language_open));
        if theme != self.settings.theme {
            self.set_theme_mode(ctx, theme);
        }
        // The update dialog may have taken over (see `PaintApp::poll_updater`).
        if r == Footer::None && self.ui.modal.is_none() {
            self.ui.modal = Some(Modal::Settings);
        }
    }
}

impl PaintApp {
    /// Switches the interface to `code` (from [`LANGUAGES`]), or back to the system language with
    /// `None`, and remembers the choice.
    fn set_language(&mut self, ctx: &egui::Context, code: Option<&str>) {
        #[cfg(not(target_os = "macos"))]
        let _ = ctx;
        self.settings.language = code.map(str::to_owned);
        self.settings.save();
        self.tr = crate::i18n::resolve(code);
        // The native menu bar is built with the strings of the time: build it again (the old
        // one goes first, so its teardown can't undo the new one).
        #[cfg(target_os = "macos")]
        {
            self.native_menu = None;
            self.native_menu = Some(crate::mac_menu::NativeMenu::install(self, ctx));
        }
    }
}

fn separator(ui: &mut egui::Ui, t: &Theme) {
    ui.add_space(10.0);
    let (line, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(line.x_range(), line.center().y, Stroke::new(1.0, t.separator));
    ui.add_space(6.0);
}
