//! Hand-drawn Fluent widgets. Everything is painted explicitly so the look matches Paint exactly.

use crate::theme::{self, Theme};
use egui::{Align2, Color32, CornerRadius, FontId, Id, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

pub const RADIUS: u8 = 4;

pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, theme::icons())
}

/// Paints an icon glyph centered at `center`.
pub fn paint_icon(painter: &egui::Painter, center: Pos2, glyph: char, size: f32, color: Color32) {
    painter.text(center, Align2::CENTER_CENTER, glyph, icon_font(size), color);
}

/// Paints a two-tone icon: a colored filled glyph below the regular outline glyph.
pub fn paint_icon_two_tone(painter: &egui::Painter, center: Pos2, glyph: char, filled: Option<(char, Color32)>, size: f32, color: Color32) {
    if let Some((f, c)) = filled {
        painter.text(center, Align2::CENTER_CENTER, f, FontId::new(size, theme::icons_filled()), c.gamma_multiply(0.55));
    }
    paint_icon(painter, center, glyph, size, color);
}

/// Background of an interactive element according to its state.
pub fn state_fill(t: &Theme, resp: &Response, enabled: bool) -> Color32 {
    if !enabled {
        Color32::TRANSPARENT
    } else if resp.is_pointer_button_down_on() {
        t.pressed
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    }
}

#[derive(Clone, Copy)]
pub struct IconButton<'a> {
    pub glyph: char,
    pub filled: Option<(char, Color32)>,
    pub icon_size: f32,
    pub selected: bool,
    pub enabled: bool,
    pub tooltip: Option<&'a str>,
}

impl<'a> IconButton<'a> {
    pub fn new(glyph: char) -> Self {
        Self { glyph, filled: None, icon_size: 20.0, selected: false, enabled: true, tooltip: None }
    }
    pub fn filled(mut self, glyph: char, color: Color32) -> Self {
        self.filled = Some((glyph, color));
        self
    }
    pub fn selected(mut self, s: bool) -> Self {
        self.selected = s;
        self
    }
    pub fn enabled(mut self, e: bool) -> Self {
        self.enabled = e;
        self
    }
    pub fn tooltip(mut self, t: &'a str) -> Self {
        self.tooltip = Some(t);
        self
    }
    pub fn size(mut self, s: f32) -> Self {
        self.icon_size = s;
        self
    }

    pub fn show(self, ui: &mut Ui, t: &Theme, rect: Rect, id: Id) -> Response {
        let sense = if self.enabled { Sense::click() } else { Sense::hover() };
        let mut resp = ui.interact(rect, id, sense);
        let p = ui.painter();
        let fill = state_fill(t, &resp, self.enabled);
        if self.selected {
            p.rect(rect, RADIUS, t.hover, Stroke::new(1.0, t.selected_border), StrokeKind::Inside);
        } else if fill != Color32::TRANSPARENT {
            p.rect_filled(rect, RADIUS, fill);
        }
        let color = if self.enabled { t.text } else { t.text_disabled };
        let filled = if self.enabled { self.filled } else { None };
        paint_icon_two_tone(p, rect.center(), self.glyph, filled, self.icon_size, color);
        if let Some(tip) = self.tooltip {
            resp = resp.on_hover_text(tip);
        }
        resp
    }
}

/// A small chevron, used next to split buttons.
pub fn paint_chevron(painter: &egui::Painter, center: Pos2, color: Color32, size: f32) {
    paint_icon(painter, center, crate::ui::icons::regular::CHEVRON_DOWN, size, color);
}

/// A clickable chevron region (the dropdown part of a split button).
pub fn chevron_button(ui: &mut Ui, t: &Theme, rect: Rect, id: Id, enabled: bool, open: bool) -> Response {
    let resp = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
    let fill = if open { t.hover } else { state_fill(t, &resp, enabled) };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, RADIUS, fill);
    }
    paint_chevron(ui.painter(), rect.center(), if enabled { t.text } else { t.text_disabled }, 12.0);
    resp
}

/// Vertical separator line between ribbon groups.
pub fn vseparator(painter: &egui::Painter, t: &Theme, x: f32, y0: f32, y1: f32) {
    painter.vline(x.round() + 0.5, y0..=y1, Stroke::new(1.0, t.separator));
}

pub fn label(painter: &egui::Painter, pos: Pos2, anchor: Align2, text: &str, size: f32, color: Color32) -> Rect {
    painter.text(pos, anchor, text, FontId::proportional(size), color)
}

pub fn text_width(ui: &Ui, text: &str, font: FontId) -> f32 {
    ui.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font, Color32::WHITE).size().x)
}

/// Fluent accent ("primary") button.
pub fn accent_button(ui: &mut Ui, t: &Theme, text: &str, width: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 32.0), Sense::click());
    let fill = if resp.is_pointer_button_down_on() {
        t.accent.gamma_multiply(0.85)
    } else if resp.hovered() {
        t.accent_hover
    } else {
        t.accent
    };
    ui.painter().rect_filled(rect, RADIUS, fill);
    label(ui.painter(), rect.center(), Align2::CENTER_CENTER, text, 14.0, t.on_accent);
    resp
}

/// Fluent standard button.
pub fn standard_button(ui: &mut Ui, t: &Theme, text: &str, width: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 32.0), Sense::click());
    paint_standard_button(ui, t, rect, &resp, text, true);
    resp
}

pub fn paint_standard_button(ui: &Ui, t: &Theme, rect: Rect, resp: &Response, text: &str, enabled: bool) {
    let fill = if !enabled {
        t.button_bg
    } else if resp.is_pointer_button_down_on() {
        t.button_bg
    } else if resp.hovered() {
        t.button_hover
    } else {
        t.button_bg
    };
    ui.painter().rect(rect, RADIUS, fill, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    label(ui.painter(), rect.center(), Align2::CENTER_CENTER, text, 14.0, if enabled { t.text } else { t.text_disabled });
}

/// Fluent radio button with a label; returns the click response.
pub fn radio(ui: &mut Ui, t: &Theme, selected: bool, text: &str) -> Response {
    let w = 28.0 + text_width(ui, text, FontId::proportional(14.0));
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
    let c = pos2(rect.left() + 10.0, rect.center().y);
    let p = ui.painter();
    if selected {
        p.circle_filled(c, 10.0, t.accent);
        let inner = if resp.hovered() { 5.0 } else { 4.0 };
        p.circle_filled(c, inner, t.on_accent);
    } else {
        p.circle(c, 9.5, if resp.hovered() { t.hover } else { Color32::TRANSPARENT }, Stroke::new(1.0, t.text_secondary));
    }
    label(p, pos2(rect.left() + 28.0, c.y), Align2::LEFT_CENTER, text, 14.0, t.text);
    resp
}

/// Fluent checkbox with a label.
pub fn checkbox(ui: &mut Ui, t: &Theme, checked: &mut bool, text: &str) -> Response {
    let w = 30.0 + text_width(ui, text, FontId::proportional(14.0));
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
    if resp.clicked() {
        *checked = !*checked;
    }
    paint_checkbox(ui.painter(), t, pos2(rect.left() + 10.0, rect.center().y), *checked, resp.hovered());
    label(ui.painter(), pos2(rect.left() + 28.0, rect.center().y), Align2::LEFT_CENTER, text, 14.0, t.text);
    resp
}

pub fn paint_checkbox(p: &egui::Painter, t: &Theme, center: Pos2, checked: bool, hovered: bool) {
    let r = Rect::from_center_size(center, vec2(20.0, 20.0));
    if checked {
        p.rect_filled(r, RADIUS, t.accent);
        paint_icon(p, center, crate::ui::icons::regular::CHECKMARK, 16.0, t.on_accent);
    } else {
        p.rect(r, RADIUS, if hovered { t.hover } else { Color32::TRANSPARENT }, Stroke::new(1.0, t.text_secondary), StrokeKind::Inside);
    }
}

/// A Fluent text field; returns the inner response. `width` in points.
pub fn text_field(ui: &mut Ui, t: &Theme, value: &mut String, width: f32, id: Id) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 32.0), Sense::hover());
    let inner = rect.shrink2(vec2(10.0, 6.0));
    ui.painter().rect(rect, RADIUS, t.input_bg, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    let edit = egui::TextEdit::singleline(value)
        .id(id)
        .frame(egui::Frame::NONE)
        .font(FontId::proportional(14.0))
        .text_color(t.text)
        .align(egui::Align2::LEFT_CENTER)
        .desired_width(inner.width());
    let resp = ui.put(inner, edit);
    let line = if resp.has_focus() { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.input_underline) };
    let y = rect.bottom() - line.width / 2.0;
    ui.painter().hline(rect.left() + 1.0..=rect.right() - 1.0, y, line);
    resp
}

/// A numeric text field bound to an integer. Returns true when the value changed.
pub fn number_field(ui: &mut Ui, t: &Theme, value: &mut i64, min: i64, max: i64, width: f32, suffix: &str, id: Id) -> bool {
    let buf_id = id.with("buf");
    let mut text: String = ui.data_mut(|d| d.get_temp::<String>(buf_id)).unwrap_or_else(|| format!("{value}{suffix}"));
    let had_focus = ui.memory(|m| m.has_focus(id));
    if !had_focus {
        text = format!("{value}{suffix}");
    }
    let resp = text_field(ui, t, &mut text, width, id);
    let mut changed = false;
    if resp.changed() {
        let digits: String = text.chars().filter(|c| c.is_ascii_digit() || *c == '-').collect();
        if let Ok(v) = digits.parse::<i64>() {
            let v = v.clamp(min, max);
            if v != *value {
                *value = v;
                changed = true;
            }
        }
    }
    if resp.lost_focus() {
        text = format!("{value}{suffix}");
    }
    ui.data_mut(|d| d.insert_temp(buf_id, text));
    changed
}

/// A horizontal or vertical Fluent slider. `value` is normalized to `[0, 1]`.
pub fn slider(ui: &mut Ui, t: &Theme, rect: Rect, id: Id, value: &mut f32, vertical: bool) -> Response {
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let (start, end) = if vertical {
        (pos2(rect.center().x, rect.bottom() - 10.0), pos2(rect.center().x, rect.top() + 10.0))
    } else {
        (pos2(rect.left() + 10.0, rect.center().y), pos2(rect.right() - 10.0, rect.center().y))
    };
    if let Some(pos) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked() || resp.is_pointer_button_down_on())
    {
        let v = if vertical { (start.y - pos.y) / (start.y - end.y) } else { (pos.x - start.x) / (end.x - start.x) };
        *value = v.clamp(0.0, 1.0);
    }
    let p = ui.painter();
    let thumb = start + (end - start) * *value;
    p.line_segment([start, end], Stroke::new(4.0, t.slider_track));
    p.line_segment([start, thumb], Stroke::new(4.0, t.accent));
    p.circle(thumb, 10.0, t.slider_thumb, Stroke::new(1.0, t.panel_border));
    let inner = if resp.is_pointer_button_down_on() {
        5.0
    } else if resp.hovered() {
        7.0
    } else {
        6.0
    };
    p.circle_filled(thumb, inner, t.accent);
    resp
}

/// Draws the frame of a floating panel (menus, flyouts, side panels).
pub fn panel_frame(t: &Theme) -> egui::Frame {
    egui::Frame::NONE
        .fill(t.menu_bg)
        .stroke(Stroke::new(1.0, t.menu_border))
        .corner_radius(CornerRadius::same(8))
        .shadow(egui::Shadow { offset: [0, 8], blur: 16, spread: 0, color: Color32::from_black_alpha(if t.dark { 100 } else { 38 }) })
        .inner_margin(egui::Margin::same(4))
}

/// Paints a dashed rectangle outline (selection marquee).
pub fn dashed_rect(painter: &egui::Painter, rect: Rect, dark: Color32, light: Color32) {
    let pts = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
    painter.add(egui::Shape::line(pts.to_vec(), Stroke::new(1.0, light)));
    for w in pts.windows(2) {
        painter.extend(egui::Shape::dashed_line(&[w[0], w[1]], Stroke::new(1.0, dark), 4.0, 4.0));
    }
}

/// Size helper.
pub fn square(c: Pos2, s: f32) -> Rect {
    Rect::from_center_size(c, Vec2::splat(s))
}
