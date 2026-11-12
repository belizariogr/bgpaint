//! Modal dialogs, drawn in Windows 11's ContentDialog style.

use super::widgets;
use crate::app::PaintApp;
use crate::files::Pending;
use crate::theme::{self, Theme};
use egui::{Color32, CornerRadius, FontId, Id, Margin, RichText, Stroke, vec2};

/// The dialog currently shown.
pub enum Modal {
    ConfirmSave(Pending),
    Error(String),
    ResizeSkew(super::image_dialogs::ResizeState),
    Properties(super::image_dialogs::PropertiesState),
    RotateCustom(super::image_dialogs::RotateState),
    EditColors(super::color_dialog::ColorDialog),
    Print(super::print_dialog::PrintDialog),
    Settings,
    Update,
}

/// What the footer buttons of a dialog produced.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Footer {
    None,
    Primary,
    Secondary,
    Cancel,
}

/// The footer buttons of a dialog: the accent (default) button, an optional secondary one and an
/// optional Cancel.
pub type Buttons<'a> = (&'a str, Option<&'a str>, Option<&'a str>);

/// How the footer lays out its buttons.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// The platform's usual layout: equal-width buttons filling the row (Fluent), or right
    /// aligned and sized to their titles (macOS).
    Platform,
    /// A save alert on every platform: buttons sized to their titles, right aligned with the
    /// default button last. A `destructive` secondary button (deleting a document that exists
    /// nowhere else) sits alone on the left in red.
    Alert { destructive: bool },
}

/// Shows a dialog with a title, a body and up to three footer buttons
/// (`primary` is the accent button). Escape triggers Cancel.
pub fn dialog(
    ctx: &egui::Context,
    t: &Theme,
    id: &str,
    title: &str,
    width: f32,
    buttons: Buttons,
    body: impl FnOnce(&mut egui::Ui),
) -> Footer {
    dialog_with(ctx, t, (id, title, width), (buttons, Layout::Platform), body)
}

/// [`dialog`] with an explicit footer [`Layout`]; `head` is (id, title, width).
fn dialog_with(
    ctx: &egui::Context,
    t: &Theme,
    head: (&str, &str, f32),
    footer: (Buttons, Layout),
    body: impl FnOnce(&mut egui::Ui),
) -> Footer {
    let (id, title, width) = head;
    let (buttons, layout) = footer;
    let mut result = Footer::None;
    let frame = egui::Frame::NONE
        .fill(t.dialog_bg)
        .stroke(Stroke::new(1.0, t.menu_border))
        .corner_radius(CornerRadius::same(8))
        .shadow(egui::Shadow { offset: [0, 16], blur: 48, spread: 0, color: Color32::from_black_alpha(if t.dark { 120 } else { 60 }) });
    egui::Modal::new(Id::unique(id)).backdrop_color(Color32::from_black_alpha(if t.dark { 40 } else { 18 })).frame(frame).show(ctx, |ui| {
        ui.set_width(width);
        egui::Frame::NONE.inner_margin(Margin { left: 24, right: 24, top: 22, bottom: 18 }).show(ui, |ui| {
            ui.set_width(width - 48.0);
            ui.set_max_width(width - 48.0);
            ui.label(RichText::new(title).font(FontId::new(20.0, theme::semibold())).color(t.text));
            ui.add_space(10.0);
            body(ui);
        });
        let destructive = layout == Layout::Alert { destructive: true };
        if cfg!(target_os = "macos") {
            // No footer band; buttons sized to their titles, as in AppKit alerts.
            let bw = |ui: &egui::Ui, text: &str| (widgets::text_width(ui, text, widgets::mac::font()) + 32.0).max(84.0);
            egui::Frame::NONE.inner_margin(Margin { left: 24, right: 24, top: 6, bottom: 18 }).show(ui, |ui| {
                ui.set_width(width - 48.0);
                result = aligned_buttons(ui, t, buttons, destructive, 10.0, bw);
            });
        } else {
            egui::Frame::NONE
                .fill(t.dialog_footer)
                .corner_radius(CornerRadius { nw: 0, ne: 0, sw: 8, se: 8 })
                .inner_margin(Margin::same(24))
                .show(ui, |ui| {
                    ui.set_width(width - 48.0);
                    result = if layout == Layout::Platform {
                        fluent_buttons(ui, t, buttons, width - 48.0)
                    } else {
                        let bw = |ui: &egui::Ui, text: &str| (widgets::text_width(ui, text, FontId::proportional(14.0)) + 40.0).max(96.0);
                        aligned_buttons(ui, t, buttons, destructive, 8.0, bw)
                    };
                });
        }
    });
    if result == Footer::None {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            result = Footer::Cancel;
        } else if ctx.input(|i| i.key_pressed(egui::Key::Enter)) && !ctx.egui_wants_keyboard_input() {
            result = Footer::Primary;
        }
    }
    result
}

/// Fluent's ContentDialog row: equal-width buttons filling `width`, the accent one first.
fn fluent_buttons(ui: &mut egui::Ui, t: &Theme, buttons: Buttons, width: f32) -> Footer {
    let mut result = Footer::None;
    let n = 1 + usize::from(buttons.1.is_some()) + usize::from(buttons.2.is_some());
    let bw = (width - 8.0 * (n as f32 - 1.0)) / n as f32;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if widgets::accent_button(ui, t, buttons.0, bw).clicked() {
            result = Footer::Primary;
        }
        if let Some(s) = buttons.1
            && widgets::standard_button(ui, t, s, bw).clicked()
        {
            result = Footer::Secondary;
        }
        if let Some(c) = buttons.2
            && widgets::standard_button(ui, t, c, bw).clicked()
        {
            result = Footer::Cancel;
        }
    });
    result
}

/// A row of buttons sized by `bw` and right aligned with the default button last
/// (`[Don't Save] [Cancel] [Save]`); a `destructive` secondary button sits alone on the left in
/// red, as in AppKit's save alerts (`[Delete]      [Cancel] [Save]`). Buttons are created left
/// to right so Tab moves focus in reading order.
fn aligned_buttons(
    ui: &mut egui::Ui,
    t: &Theme,
    buttons: Buttons,
    destructive: bool,
    spacing: f32,
    bw: impl Fn(&egui::Ui, &str) -> f32,
) -> Footer {
    let mut result = Footer::None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = spacing;
        let mut right = 0.0;
        for text in [buttons.1.filter(|_| !destructive), buttons.2, Some(buttons.0)].into_iter().flatten() {
            right += bw(ui, text) + spacing;
        }
        if let Some(s) = buttons.1.filter(|_| destructive)
            && widgets::destructive_button(ui, t, s, bw(ui, s)).clicked()
        {
            result = Footer::Secondary;
        }
        ui.add_space((ui.available_width() - right + spacing).max(0.0));
        if let Some(s) = buttons.1.filter(|_| !destructive)
            && widgets::standard_button(ui, t, s, bw(ui, s)).clicked()
        {
            result = Footer::Secondary;
        }
        if let Some(c) = buttons.2
            && widgets::standard_button(ui, t, c, bw(ui, c)).clicked()
        {
            result = Footer::Cancel;
        }
        if widgets::accent_button(ui, t, buttons.0, bw(ui, buttons.0)).clicked() {
            result = Footer::Primary;
        }
    });
    result
}

/// A section heading inside a dialog.
pub fn section(ui: &mut egui::Ui, t: &Theme, text: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(text).font(FontId::new(14.0, theme::semibold())).color(t.text));
    ui.add_space(2.0);
}

impl PaintApp {
    pub fn modals(&mut self, ctx: &egui::Context) {
        let Some(modal) = self.ui.modal.take() else { return };
        let t = self.theme.clone();
        let tr = self.tr;
        match modal {
            Modal::ConfirmSave(action) => {
                let name = self.file.display_name(tr);
                // "Delete" (red, on the left) is offered only for a document that exists nowhere
                // but in memory: never saved, or its file was deleted. A file still on disk gets
                // the usual "Don't Save", right aligned with the other buttons. Checked once when
                // the dialog opens, so the buttons don't move under the pointer (or stat the
                // disk every frame).
                let memo = Id::unique("confirm-save-only-in-memory");
                let file = &self.file;
                let only_in_memory =
                    ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(memo, || file.path.is_none() || file.missing_on_disk()));
                let discard = if only_in_memory { tr.discard_document } else { tr.dont_save };
                let footer = ((tr.save, Some(discard), Some(tr.cancel)), Layout::Alert { destructive: only_in_memory });
                let r = dialog_with(ctx, &t, ("confirm-save", tr.unsaved_title, 460.0), footer, |ui| {
                    ui.label(RichText::new(format!("{} {name}?", tr.unsaved_question)).size(14.0).color(t.text));
                    ui.add_space(8.0);
                });
                if r != Footer::None {
                    ctx.data_mut(|d| d.remove_temp::<bool>(memo));
                }
                match r {
                    Footer::Primary => self.resolve_unsaved(ctx, action, true),
                    Footer::Secondary => self.resolve_unsaved(ctx, action, false),
                    Footer::Cancel => {
                        self.updater.relaunch = false;
                        self.exit_discarded.clear();
                        self.return_to_tab = None;
                    }
                    Footer::None => self.ui.modal = Some(Modal::ConfirmSave(action)),
                }
            }
            Modal::Error(text) => {
                let r = dialog(ctx, &t, "error", tr.error_title, 440.0, (tr.close, None, None), |ui| {
                    ui.label(RichText::new(&text).size(14.0).color(t.text));
                    ui.add_space(8.0);
                });
                if r == Footer::None {
                    self.ui.modal = Some(Modal::Error(text));
                }
            }
            Modal::ResizeSkew(state) => self.resize_dialog(ctx, &t, state),
            Modal::Properties(state) => self.properties_dialog(ctx, &t, state),
            Modal::RotateCustom(state) => self.rotate_dialog(ctx, &t, state),
            Modal::EditColors(state) => self.color_dialog(ctx, &t, state),
            Modal::Print(state) => self.print_dialog(ctx, &t, state),
            Modal::Settings => self.settings_dialog(ctx, &t),
            Modal::Update => self.update_dialog(ctx, &t),
        }
    }
}

/// A row of field captions, each starting at an x offset so it lines up with its field below.
pub fn label_row(ui: &mut egui::Ui, t: &Theme, labels: &[(&str, f32)]) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), egui::Sense::hover());
    for (text, x) in labels {
        ui.painter().text(
            egui::pos2(rect.left() + x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            *text,
            FontId::proportional(14.0),
            t.text,
        );
    }
}
