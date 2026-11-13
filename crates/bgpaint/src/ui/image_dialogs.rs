//! "Resize and skew", "Image properties" and "Custom rotate" dialogs.

use super::dialogs::{self, Footer, Modal, dialog};
use super::icons::regular as ic;
use super::widgets::{self, IconButton};
use crate::app::PaintApp;
use crate::theme::Theme;
use bgpaint_core::document::{DEFAULT_HEIGHT, DEFAULT_WIDTH};
use bgpaint_core::history::Change;
use bgpaint_core::raster::{self, MAX_AREA, MAX_SIDE};
use bgpaint_core::tiny_skia::Transform;
use egui::{Id, RichText, vec2};

pub struct ResizeState {
    by_percent: bool,
    h: i64,
    v: i64,
    keep_ratio: bool,
    skew_h: i64,
    skew_v: i64,
    /// Size of what is being resized (selection or image).
    orig: (u32, u32),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Units {
    Inches,
    Centimeters,
    Pixels,
}

pub struct PropertiesState {
    units: Units,
    /// Size in pixels.
    w: u32,
    h: u32,
    text_w: String,
    text_h: String,
}

pub struct RotateState {
    angle: i64,
}

const DPI: f64 = 96.0;

fn to_unit(px: u32, u: Units) -> String {
    match u {
        Units::Pixels => px.to_string(),
        Units::Inches => format!("{:.2}", f64::from(px) / DPI),
        Units::Centimeters => format!("{:.2}", f64::from(px) / DPI * 2.54),
    }
}

fn from_unit(s: &str, u: Units) -> Option<u32> {
    let v: f64 = s.trim().replace(',', ".").parse().ok()?;
    let px = match u {
        Units::Pixels => v,
        Units::Inches => v * DPI,
        Units::Centimeters => v / 2.54 * DPI,
    };
    (px >= 1.0 && px <= f64::from(MAX_SIDE)).then_some(px.round() as u32)
}

/// Result size of a resize, clamped to the supported limits.
fn resized(orig: (u32, u32), by_percent: bool, h: i64, v: i64) -> (u32, u32) {
    let (w, hh) = if by_percent {
        ((orig.0 as f64 * h as f64 / 100.0).round(), (orig.1 as f64 * v as f64 / 100.0).round())
    } else {
        (h as f64, v as f64)
    };
    let (mut w, mut hh) = (w.clamp(1.0, f64::from(MAX_SIDE)) as u32, hh.clamp(1.0, f64::from(MAX_SIDE)) as u32);
    while u64::from(w) * u64::from(hh) > MAX_AREA {
        w = w * 9 / 10;
        hh = hh * 9 / 10;
    }
    (w.max(1), hh.max(1))
}

impl PaintApp {
    pub fn open_resize(&mut self) {
        let orig = self.selection_content_size().unwrap_or((self.doc.width(), self.doc.height()));
        self.ui.modal =
            Some(Modal::ResizeSkew(ResizeState { by_percent: true, h: 100, v: 100, keep_ratio: true, skew_h: 0, skew_v: 0, orig }));
    }

    pub fn open_properties(&mut self) {
        self.commit_interaction();
        let (w, h) = (self.doc.width(), self.doc.height());
        self.ui.modal =
            Some(Modal::Properties(PropertiesState { units: Units::Pixels, w, h, text_w: w.to_string(), text_h: h.to_string() }));
    }

    pub fn open_rotate_custom(&mut self) {
        self.ui.modal = Some(Modal::RotateCustom(RotateState { angle: 0 }));
    }

    pub fn resize_dialog(&mut self, ctx: &egui::Context, t: &Theme, mut s: ResizeState) {
        let tr = self.tr;
        let r = dialog(ctx, t, "resize-dialog", tr.resize_and_skew, 320.0, (tr.ok, Some(tr.cancel), None), |ui| {
            dialogs::section(ui, t, tr.resize);
            ui.horizontal(|ui| {
                if widgets::radio(ui, t, s.by_percent, tr.percentage).clicked() && !s.by_percent {
                    s.by_percent = true;
                    s.h = 100;
                    s.v = 100;
                }
                if widgets::radio(ui, t, !s.by_percent, tr.pixels).clicked() && s.by_percent {
                    s.by_percent = false;
                    s.h = i64::from(s.orig.0);
                    s.v = i64::from(s.orig.1);
                }
            });
            ui.add_space(4.0);
            dialogs::label_row(ui, t, &[(tr.horizontal, 0.0), (tr.vertical, 152.0)]);
            let max = if s.by_percent { 2000 } else { i64::from(MAX_SIDE) };
            let ratio = s.orig.1 as f64 / s.orig.0.max(1) as f64;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if widgets::number_field(ui, t, &mut s.h, 1..=max, 112.0, "", Id::unique("rs-h")) && s.keep_ratio {
                    s.v = if s.by_percent { s.h } else { ((s.h as f64) * ratio).round().max(1.0) as i64 };
                }
                let link = ui.allocate_exact_size(vec2(28.0, 32.0), egui::Sense::hover()).0;
                if IconButton::new(ic::LINK)
                    .size(16.0)
                    .selected(s.keep_ratio)
                    .tooltip(tr.maintain_aspect_ratio)
                    .show(ui, t, link, Id::unique("rs-link"))
                    .clicked()
                {
                    s.keep_ratio = !s.keep_ratio;
                }
                if widgets::number_field(ui, t, &mut s.v, 1..=max, 112.0, "", Id::unique("rs-v")) && s.keep_ratio {
                    s.h = if s.by_percent { s.v } else { ((s.v as f64) / ratio).round().max(1.0) as i64 };
                }
            });
            ui.add_space(8.0);
            dialogs::section(ui, t, tr.skew);
            dialogs::label_row(ui, t, &[(tr.horizontal, 0.0), (tr.vertical, 136.0)]);
            ui.horizontal(|ui| {
                widgets::number_field(ui, t, &mut s.skew_h, -89..=89, 128.0, "°", Id::unique("rs-sh"));
                widgets::number_field(ui, t, &mut s.skew_v, -89..=89, 128.0, "°", Id::unique("rs-sv"));
            });
            ui.add_space(8.0);
        });
        match r {
            Footer::Primary => {
                let (w, h) = resized(s.orig, s.by_percent, s.h, s.v);
                let (kx, ky) = (s.skew_h as f32, s.skew_v as f32);
                let f = move |p: &bgpaint_core::tiny_skia::Pixmap| {
                    let mut out = if (w, h) == (p.width(), p.height()) { p.clone() } else { raster::resize(p, w, h) };
                    if kx != 0.0 || ky != 0.0 {
                        out = raster::skew_degrees(&out, kx, ky);
                    }
                    out
                };
                if self.selection_content_size().is_some() || (w, h) != s.orig || kx != 0.0 || ky != 0.0 {
                    let t = move |ow: u32, oh: u32| {
                        let scale = Transform::from_scale(w as f32 / ow as f32, h as f32 / oh as f32);
                        if kx != 0.0 || ky != 0.0 { scale.post_concat(raster::skew_degrees_transform(w, h, kx, ky)) } else { scale }
                    };
                    self.transform_image("resize", t, f);
                }
            }
            Footer::None => self.ui.modal = Some(Modal::ResizeSkew(s)),
            _ => {}
        }
    }

    pub fn properties_dialog(&mut self, ctx: &egui::Context, t: &Theme, mut s: PropertiesState) {
        let tr = self.tr;
        let saved = self.file.path.as_ref().map(|p| p.display().to_string());
        let disk = self.file.disk_size.map(super::statusbar::human_size);
        let r = dialog(ctx, t, "props-dialog", tr.image_properties_title, 520.0, (tr.ok, Some(tr.cancel), None), |ui| {
            dialogs::section(ui, t, tr.file_attributes);
            egui::Grid::new("props-grid").num_columns(2).min_col_width(230.0).spacing(vec2(16.0, 10.0)).show(ui, |ui| {
                let row = |ui: &mut egui::Ui, k: &str, v: &str| {
                    ui.label(RichText::new(k).size(14.0).color(t.text));
                    ui.add(egui::Label::new(RichText::new(v).size(14.0).color(t.text)).truncate());
                    ui.end_row();
                };
                row(ui, tr.saved_at, saved.as_deref().unwrap_or(tr.not_available));
                row(ui, tr.size_on_disk, disk.as_deref().unwrap_or(tr.not_available));
                row(ui, tr.resolution, "96 DPI");
            });
            ui.add_space(10.0);
            ui.separator();
            dialogs::section(ui, t, tr.units);
            for (u, label) in [(Units::Inches, tr.inches), (Units::Centimeters, tr.centimeters), (Units::Pixels, tr.pixels)] {
                if widgets::radio(ui, t, s.units == u, label).clicked() && s.units != u {
                    s.units = u;
                    s.text_w = to_unit(s.w, u);
                    s.text_h = to_unit(s.h, u);
                }
            }
            ui.add_space(6.0);
            ui.separator();
            dialogs::section(ui, t, tr.image_size);
            dialogs::label_row(ui, t, &[(tr.width, 0.0), (tr.height, 194.0)]);
            ui.horizontal(|ui| {
                if widgets::text_field(ui, t, &mut s.text_w, 186.0, Id::unique("props-w")).changed()
                    && let Some(v) = from_unit(&s.text_w, s.units)
                {
                    s.w = v;
                }
                if widgets::text_field(ui, t, &mut s.text_h, 186.0, Id::unique("props-h")).changed()
                    && let Some(v) = from_unit(&s.text_h, s.units)
                {
                    s.h = v;
                }
                if widgets::standard_button(ui, t, tr.default, 90.0).clicked() {
                    s.w = DEFAULT_WIDTH;
                    s.h = DEFAULT_HEIGHT;
                    s.text_w = to_unit(s.w, s.units);
                    s.text_h = to_unit(s.h, s.units);
                }
            });
            ui.add_space(8.0);
        });
        match r {
            Footer::Primary => {
                let rect = crate::tools::interaction::clamp_area(bgpaint_core::geom::PixelRect::from_size(s.w, s.h));
                if (rect.width(), rect.height()) != (self.doc.width(), self.doc.height()) {
                    let before = self.doc.snapshot();
                    self.doc.resize_canvas(rect.width(), rect.height(), 0, 0);
                    self.history.push("image properties", vec![Change::document(before, &self.doc)]);
                }
            }
            Footer::None => self.ui.modal = Some(Modal::Properties(s)),
            _ => {}
        }
    }

    pub fn rotate_dialog(&mut self, ctx: &egui::Context, t: &Theme, mut s: RotateState) {
        let tr = self.tr;
        let r = dialog(ctx, t, "rotate-dialog", tr.rotate_custom, 340.0, (tr.ok, Some(tr.cancel), None), |ui| {
            dialogs::section(ui, t, tr.angle);
            ui.horizontal(|ui| {
                widgets::number_field(ui, t, &mut s.angle, -360..=360, 100.0, "°", Id::unique("rot-angle"));
                let (rect, _) = ui.allocate_exact_size(vec2(180.0, 32.0), egui::Sense::hover());
                let mut v = (s.angle as f32 + 180.0) / 360.0;
                if widgets::slider(ui, t, rect, Id::unique("rot-slider"), &mut v, false).dragged() {
                    s.angle = (v * 360.0 - 180.0).round() as i64;
                }
            });
            ui.add_space(8.0);
        });
        match r {
            Footer::Primary if s.angle % 360 != 0 => {
                let deg = s.angle as f32;
                let f = move |p: &bgpaint_core::tiny_skia::Pixmap| raster::rotate_degrees(p, deg);
                self.transform_image("rotate", |w, h| raster::rotate_degrees_transform(w, h, deg), f);
            }
            Footer::None => self.ui.modal = Some(Modal::RotateCustom(s)),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_conversion() {
        assert_eq!(to_unit(96, Units::Inches), "1.00");
        assert_eq!(from_unit("2,54", Units::Centimeters), Some(96));
        assert_eq!(from_unit("0", Units::Pixels), None);
        assert_eq!(from_unit("abc", Units::Pixels), None);
    }

    #[test]
    fn resize_math() {
        assert_eq!(resized((200, 100), true, 50, 50), (100, 50));
        assert_eq!(resized((200, 100), false, 300, 20), (300, 20));
        let (w, h) = resized((30000, 30000), true, 100, 100);
        assert!(u64::from(w) * u64::from(h) <= MAX_AREA);
    }
}
