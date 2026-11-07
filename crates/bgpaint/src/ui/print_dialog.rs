//! Print, Page setup and Print preview (one dialog with a live page preview).

use super::dialogs::{self, Footer, Modal, dialog};
use super::menu::{self, Entry};
use super::widgets;
use crate::app::PaintApp;
use crate::commands::Command;
use crate::platform::{self, Orientation, Paper, PrintSettings};
use crate::theme::Theme;
use egui::{Align2, Color32, Id, Rect, RichText, Sense, Stroke, pos2, vec2};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PrintMode {
    Print,
    PageSetup,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dropdown {
    None,
    Printer,
    Paper,
}

pub struct PrintDialog {
    mode: PrintMode,
    settings: PrintSettings,
    printers: Vec<String>,
    /// The printer list, being read from CUPS on a worker thread (lpstat may be slow).
    printers_rx: Option<std::sync::mpsc::Receiver<(Vec<String>, Option<String>)>>,
    copies: i64,
    margin: i64,
    open: Dropdown,
    anchors: (Rect, Rect),
}

impl PaintApp {
    pub fn open_print(&mut self, cmd: Command) {
        self.commit_interaction();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(platform::printers());
        });
        let settings = self.print_settings.clone();
        let mode = if cmd == Command::PageSetup { PrintMode::PageSetup } else { PrintMode::Print };
        let copies = i64::from(settings.copies);
        let margin = settings.margin_mm.round() as i64;
        self.ui.modal = Some(Modal::Print(PrintDialog {
            mode,
            settings,
            printers: Vec::new(),
            printers_rx: Some(rx),
            copies,
            margin,
            open: Dropdown::None,
            anchors: (Rect::NOTHING, Rect::NOTHING),
        }));
    }

    pub fn print_dialog(&mut self, ctx: &egui::Context, t: &Theme, mut d: PrintDialog) {
        let tr = self.tr;
        if let Some(rx) = &d.printers_rx {
            match rx.try_recv() {
                Ok((printers, default)) => {
                    if d.settings.printer.as_ref().is_none_or(|p| !printers.contains(p)) {
                        d.settings.printer = default.or_else(|| printers.first().cloned());
                    }
                    d.printers = printers;
                    d.printers_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(50)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => d.printers_rx = None,
            }
        }
        let title = if d.mode == PrintMode::PageSetup { tr.page_setup } else { tr.print };
        let primary = if d.mode == PrintMode::PageSetup { tr.ok } else { tr.print };
        let (iw, ih) = (self.doc.width() as f32, self.doc.height() as f32);
        let r = dialog(ctx, t, "print-dialog", title, 720.0, (primary, Some(tr.cancel), None), |ui| {
            ui.horizontal_top(|ui| {
                // Page preview.
                let s = &d.settings;
                let (mut pw, mut ph) = s.paper.size_mm();
                if s.orientation == Orientation::Landscape {
                    std::mem::swap(&mut pw, &mut ph);
                }
                let box_size = vec2(300.0, 340.0);
                let (area, _) = ui.allocate_exact_size(box_size, Sense::hover());
                let k = (box_size.x / pw).min(box_size.y / ph) * 0.92;
                let page = Rect::from_center_size(area.center(), vec2(pw * k, ph * k));
                ui.painter().add(egui::Shadow { offset: [0, 2], blur: 8, spread: 0, color: t.canvas_shadow }.as_shape(page, 2));
                ui.painter().rect_filled(page, 2, Color32::WHITE);
                let m = s.margin_mm * k;
                let printable = page.shrink(m.min(page.width() / 2.0 - 1.0).min(page.height() / 2.0 - 1.0));
                ui.painter().rect_stroke(printable, 0, Stroke::new(1.0, Color32::from_gray(210)), egui::StrokeKind::Inside);
                let scale = if s.fit { (printable.width() / iw).min(printable.height() / ih) } else { 25.4 / 96.0 * k };
                let size = vec2(iw * scale, ih * scale);
                let img =
                    if s.center { Rect::from_center_size(printable.center(), size) } else { Rect::from_min_size(printable.min, size) };
                let painter = ui.painter_at(printable);
                self.tiles.paint(&painter, img.min, scale, ctx.pixels_per_point());
                ui.add_space(20.0);
                ui.vertical(|ui| {
                    ui.set_width(330.0);
                    let label = |ui: &mut egui::Ui, s: &str| {
                        ui.label(RichText::new(s).size(14.0).color(t.text));
                    };
                    if d.mode == PrintMode::Print {
                        label(ui, tr.printer);
                        let name = match (&d.settings.printer, &d.printers_rx) {
                            (Some(p), _) => p.clone(),
                            (None, Some(_)) => "…".to_owned(),
                            (None, None) => tr.no_printers.to_owned(),
                        };
                        d.anchors.0 = combo(ui, t, &name, 330.0, &mut d.open, Dropdown::Printer);
                        ui.add_space(8.0);
                        label(ui, tr.copies);
                        widgets::number_field(ui, t, &mut d.copies, 1..=999, 120.0, "", Id::unique("pd-copies"));
                        ui.add_space(8.0);
                    }
                    label(ui, tr.paper_size);
                    d.anchors.1 = combo(ui, t, d.settings.paper.name(), 330.0, &mut d.open, Dropdown::Paper);
                    ui.add_space(8.0);
                    dialogs::section(ui, t, tr.orientation);
                    ui.horizontal(|ui| {
                        if widgets::radio(ui, t, d.settings.orientation == Orientation::Portrait, tr.portrait).clicked() {
                            d.settings.orientation = Orientation::Portrait;
                        }
                        if widgets::radio(ui, t, d.settings.orientation == Orientation::Landscape, tr.landscape).clicked() {
                            d.settings.orientation = Orientation::Landscape;
                        }
                    });
                    dialogs::section(ui, t, tr.scaling);
                    ui.horizontal(|ui| {
                        if widgets::radio(ui, t, d.settings.fit, tr.fit_to_page).clicked() {
                            d.settings.fit = true;
                        }
                        if widgets::radio(ui, t, !d.settings.fit, tr.actual_size).clicked() {
                            d.settings.fit = false;
                        }
                    });
                    ui.add_space(4.0);
                    label(ui, tr.margins);
                    if widgets::number_field(ui, t, &mut d.margin, 0..=60, 120.0, "", Id::unique("pd-margin")) {
                        d.settings.margin_mm = d.margin as f32;
                    }
                    ui.add_space(4.0);
                    widgets::checkbox(ui, t, &mut d.settings.center, tr.center_on_page);
                });
            });
            ui.add_space(8.0);
        });
        // Dropdowns.
        match d.open {
            Dropdown::Printer => {
                let entries: Vec<Entry<Option<String>>> = d
                    .printers
                    .iter()
                    .map(|p| Entry::item(None, p, Some(p.clone())).marked(d.settings.printer.as_ref() == Some(p)))
                    .collect();
                let entries = if entries.is_empty() { vec![Entry::item(None, tr.no_printers, None).enabled(false)] } else { entries };
                let out = menu::show(ctx, t, Id::unique("pd-printer-menu"), pos2(d.anchors.0.left(), d.anchors.0.bottom() + 4.0), &entries);
                if let Some(p) = out.action {
                    d.settings.printer = p;
                    d.open = Dropdown::None;
                } else if outside(ctx, &out.rects, d.anchors.0) {
                    d.open = Dropdown::None;
                }
            }
            Dropdown::Paper => {
                let entries: Vec<Entry<Paper>> =
                    Paper::ALL.iter().map(|&p| Entry::item(None, p.name(), p).marked(d.settings.paper == p)).collect();
                let out = menu::show(ctx, t, Id::unique("pd-paper-menu"), pos2(d.anchors.1.left(), d.anchors.1.bottom() + 4.0), &entries);
                if let Some(p) = out.action {
                    d.settings.paper = p;
                    d.open = Dropdown::None;
                } else if outside(ctx, &out.rects, d.anchors.1) {
                    d.open = Dropdown::None;
                }
            }
            Dropdown::None => {}
        }
        d.settings.copies = d.copies.clamp(1, 999) as u32;
        match r {
            Footer::Primary => {
                self.print_settings = d.settings.clone();
                if d.mode == PrintMode::Print {
                    self.print_now(ctx, d.settings);
                }
            }
            Footer::None => self.ui.modal = Some(Modal::Print(d)),
            _ => {}
        }
    }

    fn print_now(&mut self, ctx: &egui::Context, settings: PrintSettings) {
        let flat = self.doc.flatten();
        let png = match bgpaint_core::io::encode(&flat, bgpaint_core::io::SaveFormat::Png) {
            Ok(p) => p,
            Err(e) => {
                self.show_toast(format!("{}: {e}", self.tr.print_failed));
                return;
            }
        };
        let (ok, failed) = (self.tr.print_sent, self.tr.print_failed);
        self.run_in_background(ctx, move || {
            Some(match platform::print_png(&png, &settings) {
                Ok(()) => ok.to_owned(),
                Err(e) => format!("{failed}: {e}"),
            })
        });
    }
}

fn combo(ui: &mut egui::Ui, t: &Theme, text: &str, width: f32, open: &mut Dropdown, which: Dropdown) -> Rect {
    let (r, resp) = ui.allocate_exact_size(vec2(width, 32.0), Sense::click());
    widgets::paint_standard_button(ui, t, r, &resp, "", true);
    widgets::label(ui.painter(), pos2(r.left() + 12.0, r.center().y), Align2::LEFT_CENTER, text, 14.0, t.text);
    widgets::paint_chevron(ui.painter(), pos2(r.right() - 14.0, r.center().y), t.text, 12.0);
    if resp.clicked() {
        *open = if *open == which { Dropdown::None } else { which };
    }
    r
}

fn outside(ctx: &egui::Context, rects: &[Rect], anchor: Rect) -> bool {
    ctx.input(|i| {
        i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !rects.iter().any(|r| r.contains(p)) && !anchor.contains(p))
    })
}
