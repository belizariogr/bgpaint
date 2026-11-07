//! The update dialog, and the "Updates" section of Settings (see [`crate::update`]).

use super::dialogs::{self, Footer, Modal, dialog};
use super::widgets;
use crate::app::PaintApp;
use crate::i18n::Strings;
use crate::theme::Theme;
use crate::update::{RELEASES_URL, Stage, Updater};
use egui::{Color32, Id, RichText};

/// `template` with its `{version}` filled in.
fn fill(template: &str, version: &str) -> String {
    template.replace("{version}", version)
}

/// A clickable link in the accent color.
fn link(ui: &mut egui::Ui, t: &Theme, text: &str, url: &str) {
    let resp = ui.add(egui::Label::new(RichText::new(text).size(13.0).color(t.accent)).sense(egui::Sense::click()));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        crate::platform::open_url(url);
    }
}

/// One line about where the updater is, for Settings (None: nothing to say).
fn status(updater: &Updater, tr: &Strings) -> Option<String> {
    Some(match &updater.stage {
        Stage::Idle => return None,
        Stage::Checking => tr.update_checking.to_owned(),
        Stage::UpToDate => tr.update_up_to_date.to_owned(),
        Stage::CheckFailed(_) => tr.update_check_failed.to_owned(),
        Stage::Downloading { version, received, total } => {
            let received = received.load(std::sync::atomic::Ordering::Relaxed);
            let text = fill(tr.update_downloading, version);
            if *total > 0 { format!("{text} {}%", (received * 100 / total).min(100)) } else { text }
        }
        Stage::Ready(p) => fill(tr.update_ready_short, &p.version),
        Stage::Password { prepared, .. } => fill(tr.update_ready_short, &prepared.version),
        Stage::Installing { version, .. } => fill(tr.update_installing, version),
        Stage::Installed { version, .. } => fill(tr.update_installed, version),
        Stage::Available { version, .. } => fill(tr.update_available, version),
        Stage::Failed(_) => tr.update_failed.to_owned(),
    })
}

impl PaintApp {
    /// The "Updates" section of the Settings dialog.
    pub fn update_settings(&mut self, ui: &mut egui::Ui, t: &Theme) {
        let tr = self.tr;
        dialogs::section(ui, t, tr.updates);
        let mut auto = self.settings.check_updates;
        widgets::checkbox(ui, t, &mut auto, tr.check_updates_automatically);
        if auto != self.settings.check_updates {
            self.settings.check_updates = auto;
            self.settings.save();
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let width = widgets::text_width(ui, tr.check_for_updates, egui::FontId::proportional(14.0)) + 32.0;
            let busy = self.updater.busy();
            let clicked = ui.add_enabled_ui(!busy, |ui| widgets::standard_button(ui, t, tr.check_for_updates, width)).inner.clicked();
            if clicked && !busy {
                self.updater.check(ui.ctx(), true);
            }
            if let Some(text) = status(&self.updater, tr) {
                ui.label(RichText::new(text).size(13.0).color(t.text_secondary));
            }
        });
        if let Stage::CheckFailed(e) = &self.updater.stage {
            ui.label(RichText::new(e).size(12.0).color(t.text_secondary));
        }
        if matches!(self.updater.stage, Stage::Downloading { .. }) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    pub fn update_dialog(&mut self, ctx: &egui::Context, t: &Theme) {
        let tr = self.tr;
        let (message, buttons, notes) = match &self.updater.stage {
            Stage::Ready(p) => (fill(tr.update_ready, &p.version), (tr.update_install, None, Some(tr.update_later)), true),
            Stage::Password { prepared, .. } => {
                (fill(tr.update_password, &prepared.version), (tr.update_install, None, Some(tr.cancel)), false)
            }
            Stage::Installing { version, .. } => (fill(tr.update_installing, version), (tr.close, None, None), false),
            Stage::Installed { version, .. } => {
                (fill(tr.update_installed, version), (tr.update_restart, None, Some(tr.update_later)), true)
            }
            Stage::Available { version, manager: Some(m) } => {
                (fill(tr.update_managed, version).replace("{manager}", m), (tr.close, None, None), true)
            }
            Stage::Available { version, manager: None } => {
                (fill(tr.update_unsupported, version), (tr.update_download_page, None, Some(tr.close)), false)
            }
            Stage::Failed(e) => (format!("{}\n\n{e}", tr.update_failed), (tr.update_download_page, None, Some(tr.close)), false),
            _ => return,
        };
        let mut submit = false;
        let stage = &mut self.updater.stage;
        let r = dialog(ctx, t, "update-dialog", tr.update_title, 460.0, buttons, |ui| {
            ui.label(RichText::new(&message).size(14.0).color(t.text));
            if let Stage::Password { password, wrong, focus, .. } = stage {
                ui.add_space(12.0);
                let resp = widgets::password_field(ui, t, password, ui.available_width(), Id::unique("update-password"));
                if std::mem::take(focus) {
                    resp.request_focus();
                }
                submit = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if *wrong {
                    let red = if t.dark { Color32::from_rgb(0xFF, 0x99, 0xA4) } else { Color32::from_rgb(0xC4, 0x2B, 0x1C) };
                    ui.add_space(4.0);
                    ui.label(RichText::new(tr.update_wrong_password).size(13.0).color(red));
                }
            }
            ui.add_space(8.0);
            if notes {
                link(ui, t, tr.update_whats_new, RELEASES_URL);
                ui.add_space(4.0);
            }
        });
        let stage = &self.updater.stage;
        let installable = matches!(stage, Stage::Ready(_) | Stage::Password { .. });
        let installed = matches!(stage, Stage::Installed { .. });
        let download_page = matches!(stage, Stage::Available { manager: None, .. } | Stage::Failed(_));
        let password = matches!(stage, Stage::Password { .. });
        match r {
            Footer::Primary if installable => self.updater.install(ctx, tr.update_admin_prompt),
            Footer::Primary if installed => {
                self.updater.dismiss();
                self.updater.relaunch = true;
                self.begin_exit();
            }
            Footer::Primary => {
                if download_page {
                    crate::platform::open_url(RELEASES_URL);
                }
                self.updater.dismiss();
            }
            Footer::Secondary | Footer::Cancel if password => self.updater.cancel_password(),
            Footer::Secondary | Footer::Cancel => self.updater.dismiss(),
            Footer::None if submit => self.updater.install(ctx, tr.update_admin_prompt),
            Footer::None => {}
        }
        if self.updater.wants_dialog() && self.ui.modal.is_none() {
            self.ui.modal = Some(Modal::Update);
        }
    }
}
