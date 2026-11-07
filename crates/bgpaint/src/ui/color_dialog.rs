//! The "Edit colors" dialog.

use super::color_target::ColorTarget;
use super::dialogs::{Footer, Modal, dialog};
use super::icons::regular as ic;
use super::menu::{self, Entry};
use super::ribbon::{paint_swatch, to_egui};
use super::widgets::{self, IconButton};
use crate::app::PaintApp;
use crate::theme::Theme;
use bgpaint_core::color::Color;
use bgpaint_core::history::Change;
use egui::{Color32, Id, Rect, RichText, Sense, Stroke, pos2, vec2};

/// The 48 basic colors, in the order of Paint's dialog (12 × 4).
pub const BASIC: [u32; 48] = [
    0xFF8080, 0xFF0000, 0x804040, 0x800000, 0x400000, 0x80FFFF, 0x00FFFF, 0x0080FF, 0x0000FF, 0x0000A0, 0x004080, 0x000080, //
    0xFFFF80, 0xFFFF00, 0xFF8040, 0xFF8000, 0x804000, 0x808000, 0x8080FF, 0x8000FF, 0x0080C0, 0x000040, 0x400040, 0x400040, //
    0x80FF80, 0x80FF00, 0x00FF00, 0x00FF40, 0x00FF80, 0x808040, 0xFF80C0, 0xFF80FF, 0xFF00FF, 0xFF0080, 0x8080C0, 0x800040, //
    0x008000, 0x008040, 0x008080, 0x408080, 0x004000, 0x004040, 0x800080, 0x400080, 0x000000, 0x808080, 0xC0C0C0, 0xFFFFFF,
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Rgb,
    Hsv,
}

pub struct ColorDialog {
    target: ColorTarget,
    h: f32,
    s: f32,
    v: f32,
    hex: String,
    mode: Mode,
    mode_open: bool,
    /// The custom color slot being edited: changes made with the spectrum, the value slider and
    /// the fields are written into it. An empty slot is filled on the first change.
    custom_slot: Option<usize>,
    /// The custom colors changed since the dialog opened (saved when it closes).
    custom_dirty: bool,
}

impl ColorDialog {
    fn color(&self) -> Color {
        Color::from_hsv(self.h, self.s, self.v)
    }

    fn set(&mut self, c: Color) {
        let (h, s, v) = c.to_hsv();
        // Keep the hue when the color has no saturation, so the spectrum marker does not jump.
        if s > 0.0 && v > 0.0 {
            self.h = h;
        }
        if v > 0.0 {
            self.s = s;
        }
        self.v = v;
        self.hex = c.to_hex_string();
    }
}

fn spectrum_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let id = Id::unique("color-spectrum");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    let n = 256usize;
    let mut pixels = Vec::with_capacity(n * n);
    for y in 0..n {
        for x in 0..n {
            let c = Color::from_hsv(x as f32 / (n - 1) as f32 * 360.0, 1.0 - y as f32 / (n - 1) as f32, 1.0);
            pixels.push(to_egui(c));
        }
    }
    let tex = ctx.load_texture("spectrum", egui::ColorImage::new([n, n], pixels), egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

impl PaintApp {
    pub fn open_color_dialog(&mut self, target: ColorTarget) {
        let c = match target {
            ColorTarget::Active => {
                if self.colors.editing_secondary {
                    self.colors.secondary
                } else {
                    self.colors.primary
                }
            }
            ColorTarget::Background => self.doc.background().color,
        };
        let mut d = ColorDialog {
            target,
            h: 0.0,
            s: 0.0,
            v: 0.0,
            hex: String::new(),
            mode: Mode::Rgb,
            mode_open: false,
            custom_slot: None,
            custom_dirty: false,
        };
        d.set(c);
        self.ui.modal = Some(Modal::EditColors(d));
    }

    pub fn color_dialog(&mut self, ctx: &egui::Context, t: &Theme, mut d: ColorDialog) {
        let tr = self.tr;
        let spectrum = spectrum_texture(ctx);
        let mut custom = self.dialog_custom_colors();
        let mut custom_changed = false;
        let mut mode_anchor = Rect::NOTHING;
        let before = d.color();
        // Set when a swatch was clicked: picking a color is not an edit of the selected slot.
        let mut picked = false;
        let r = dialog(ctx, t, "color-dialog", tr.edit_colors, 650.0, (tr.ok, Some(tr.cancel), None), |ui| {
            ui.horizontal_top(|ui| {
                // Spectrum: hue across, saturation down.
                let (sr, resp) = ui.allocate_exact_size(vec2(256.0, 256.0), Sense::click_and_drag());
                ui.painter().image(spectrum.id(), sr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                if let Some(p) =
                    resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked() || resp.is_pointer_button_down_on())
                {
                    d.h = ((p.x - sr.left()) / sr.width()).clamp(0.0, 1.0) * 360.0;
                    d.s = 1.0 - ((p.y - sr.top()) / sr.height()).clamp(0.0, 1.0);
                    if d.v == 0.0 {
                        d.v = 1.0;
                    }
                    d.hex = d.color().to_hex_string();
                }
                let marker = pos2(sr.left() + d.h / 360.0 * sr.width(), sr.top() + (1.0 - d.s) * sr.height());
                ui.painter().circle_stroke(marker, 6.0, Stroke::new(2.0, Color32::WHITE));
                ui.painter().circle_stroke(marker, 7.5, Stroke::new(1.0, Color32::BLACK));
                ui.add_space(8.0);
                // Current color preview.
                let (pr, _) = ui.allocate_exact_size(vec2(40.0, 256.0), Sense::hover());
                ui.painter().rect(pr, 4, to_egui(d.color()), Stroke::new(1.0, t.input_border), egui::StrokeKind::Inside);
                ui.add_space(8.0);
                // Value slider with a gradient from the pure color to black.
                let (vr, vresp) = ui.allocate_exact_size(vec2(20.0, 256.0), Sense::click_and_drag());
                let track = Rect::from_center_size(vr.center(), vec2(8.0, 256.0));
                let top = to_egui(Color::from_hsv(d.h, d.s, 1.0));
                let mut mesh = egui::Mesh::default();
                mesh.colored_vertex(track.left_top(), top);
                mesh.colored_vertex(track.right_top(), top);
                mesh.colored_vertex(track.right_bottom(), Color32::BLACK);
                mesh.colored_vertex(track.left_bottom(), Color32::BLACK);
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(0, 2, 3);
                ui.painter().add(egui::Shape::mesh(mesh));
                if let Some(p) =
                    vresp.interact_pointer_pos().filter(|_| vresp.dragged() || vresp.clicked() || vresp.is_pointer_button_down_on())
                {
                    d.v = 1.0 - ((p.y - track.top()) / track.height()).clamp(0.0, 1.0);
                    d.hex = d.color().to_hex_string();
                }
                let knob = pos2(track.center().x, track.top() + (1.0 - d.v) * track.height());
                ui.painter().circle(knob, 7.0, Color32::WHITE, Stroke::new(1.0, Color32::from_gray(80)));
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    let resp = widgets::text_field(ui, t, &mut d.hex, 130.0, Id::unique("cd-hex"));
                    if resp.changed()
                        && let Some(c) = Color::parse_hex(&d.hex)
                    {
                        let hex = d.hex.clone();
                        d.set(c);
                        d.hex = hex;
                    }
                    if resp.lost_focus() {
                        d.hex = d.color().to_hex_string();
                    }
                    ui.add_space(8.0);
                    let mresp = widgets::combo_box(ui, t, if d.mode == Mode::Rgb { "RGB" } else { "HSV" }, 130.0);
                    let mr = mresp.rect;
                    if mresp.clicked() {
                        d.mode_open = !d.mode_open;
                    }
                    mode_anchor = mr;
                    ui.add_space(8.0);
                    let c = d.color();
                    let (labels, mut values, maxes) = match d.mode {
                        Mode::Rgb => ([tr.red, tr.green, tr.blue], [i64::from(c.r), i64::from(c.g), i64::from(c.b)], [255, 255, 255]),
                        Mode::Hsv => (
                            [tr.hue, tr.saturation, tr.value],
                            [d.h.round() as i64 % 360, (d.s * 100.0).round() as i64, (d.v * 100.0).round() as i64],
                            [359, 100, 100],
                        ),
                    };
                    let mut changed = false;
                    for i in 0..3 {
                        ui.horizontal(|ui| {
                            changed |= widgets::number_field(
                                ui,
                                t,
                                &mut values[i],
                                0..=maxes[i],
                                92.0,
                                "",
                                Id::unique(("cd-field", i, d.mode == Mode::Rgb)),
                            );
                            ui.label(RichText::new(labels[i]).size(14.0).color(t.text));
                        });
                        ui.add_space(2.0);
                    }
                    if changed {
                        match d.mode {
                            Mode::Rgb => d.set(Color::rgb(values[0] as u8, values[1] as u8, values[2] as u8)),
                            Mode::Hsv => {
                                d.h = values[0] as f32;
                                d.s = values[1] as f32 / 100.0;
                                d.v = values[2] as f32 / 100.0;
                                d.hex = d.color().to_hex_string();
                            }
                        }
                    }
                });
            });
            ui.add_space(18.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(tr.basic_colors).size(14.0).color(t.text));
                    ui.add_space(6.0);
                    let (gr, _) = ui.allocate_exact_size(vec2(12.0 * 30.0, 4.0 * 30.0), Sense::hover());
                    for (i, hex) in BASIC.iter().enumerate() {
                        let c = pos2(gr.left() + 15.0 + (i % 12) as f32 * 30.0, gr.top() + 15.0 + (i / 12) as f32 * 30.0);
                        let col = Color::from_hex(*hex);
                        let resp = ui.interact(Rect::from_center_size(c, vec2(26.0, 26.0)), Id::unique(("basic", i)), Sense::click());
                        if resp.hovered() {
                            ui.painter().circle_stroke(c, 12.0, Stroke::new(1.5, t.text_secondary));
                        }
                        paint_swatch(ui.painter(), t, c, 10.0, col);
                        if resp.clicked() {
                            d.set(col);
                            d.custom_slot = None;
                            picked = true;
                        }
                    }
                });
                ui.add_space(24.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tr.custom_colors).size(14.0).color(t.text));
                        let (ar, _) = ui.allocate_exact_size(vec2(28.0, 24.0), Sense::hover());
                        if IconButton::new(ic::ADD).size(16.0).show(ui, t, ar, Id::unique("cd-add")).clicked() {
                            let c = d.color();
                            custom.retain(|x| *x != Some(c));
                            custom.insert(0, Some(c));
                            custom.truncate(24);
                            custom_changed = true;
                            // Later edits go to the color just added.
                            d.custom_slot = Some(0);
                        }
                    });
                    ui.add_space(2.0);
                    let (gr, _) = ui.allocate_exact_size(vec2(6.0 * 30.0, 4.0 * 30.0), Sense::hover());
                    for i in 0..24 {
                        let c = pos2(gr.left() + 15.0 + (i % 6) as f32 * 30.0, gr.top() + 15.0 + (i / 6) as f32 * 30.0);
                        let resp = ui.interact(Rect::from_center_size(c, vec2(26.0, 26.0)), Id::unique(("custom-dlg", i)), Sense::click());
                        let selected = d.custom_slot == Some(i);
                        if selected {
                            ui.painter().circle_stroke(c, 12.5, Stroke::new(2.0, t.accent));
                        } else if resp.hovered() {
                            ui.painter().circle_stroke(c, 12.0, Stroke::new(1.5, t.text_secondary));
                        }
                        let col = custom.get(i).copied().flatten();
                        match col {
                            Some(col) => paint_swatch(ui.painter(), t, c, 10.0, col),
                            None => {
                                ui.painter().circle_stroke(c, 9.5, Stroke::new(1.0, t.swatch_ring));
                            }
                        }
                        let tip = if col.is_some() { tr.edit_custom_color } else { tr.define_custom_color };
                        if resp.on_hover_text(tip).clicked() {
                            picked = true;
                            if selected {
                                d.custom_slot = None;
                            } else {
                                d.custom_slot = Some(i);
                                if let Some(col) = col {
                                    d.set(col);
                                }
                            }
                        }
                    }
                });
            });
            ui.add_space(8.0);
        });
        if d.mode_open {
            let entries = vec![
                Entry::item(None, "RGB", Mode::Rgb).marked(d.mode == Mode::Rgb),
                Entry::item(None, "HSV", Mode::Hsv).marked(d.mode == Mode::Hsv),
            ];
            let out = menu::show(ctx, t, Id::unique("cd-mode"), pos2(mode_anchor.left(), mode_anchor.bottom() + 4.0), &entries);
            if let Some(m) = out.action {
                d.mode = m;
                d.mode_open = false;
            } else if ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer.interact_pos().is_some_and(|p| !out.rects.iter().any(|r| r.contains(p)) && !mode_anchor.contains(p))
            }) {
                d.mode_open = false;
            }
        }
        // Edits made while a custom slot is selected redefine that slot.
        if !picked
            && !custom_changed
            && let Some(i) = d.custom_slot
            && d.color() != before
        {
            custom[i] = Some(d.color());
            custom_changed = true;
        }
        if custom_changed {
            self.settings.dialog_custom_colors = custom.iter().map(|c| c.map(|c| c.to_hex_string()).unwrap_or_default()).collect();
            d.custom_dirty = true;
        }
        // Dragging in the spectrum changes the slot every frame, so the custom colors are written
        // to disk once, when the dialog closes; OK and Cancel both keep them.
        if d.custom_dirty && r != Footer::None {
            self.settings.save();
        }
        match r {
            Footer::Primary => {
                let c = d.color();
                match d.target {
                    ColorTarget::Active => {
                        // A selected custom slot is redefined; otherwise the color joins the custom row.
                        let redefine = matches!(self.colors.selected, Some(crate::app::Swatch::Custom(_)));
                        self.set_active_color(c);
                        if !redefine {
                            self.add_recent_color(c);
                        }
                    }
                    ColorTarget::Background => {
                        let mut bg = self.doc.background();
                        bg.color = c;
                        bg.visible = true;
                        self.commit_interaction();
                        let before = self.doc.snapshot();
                        self.doc.set_background(bg);
                        self.history.push("background color", vec![Change::document(before)]);
                    }
                }
            }
            Footer::None => self.ui.modal = Some(Modal::EditColors(d)),
            _ => {}
        }
    }

    fn dialog_custom_colors(&self) -> Vec<Option<Color>> {
        let mut v: Vec<Option<Color>> = self.settings.dialog_custom_colors.iter().map(|s| Color::parse_hex(s)).collect();
        v.resize(24, None);
        v
    }

    /// Puts a color chosen in the dialog into the palette's custom row (most recent first).
    pub fn add_recent_color(&mut self, c: Color) {
        let row = &mut self.colors.custom;
        if row.contains(&Some(c)) {
            return;
        }
        match row.iter().position(Option::is_none) {
            Some(i) => row[i] = Some(c),
            None => {
                // Full: drop the oldest (leftmost) and append.
                row.rotate_left(1);
                row[9] = Some(c);
            }
        }
    }
}
