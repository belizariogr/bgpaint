//! Fluent context menus and flyouts with icons, shortcuts, check marks and nested submenus.

use super::widgets::{self, paint_icon};
use crate::theme::Theme;
use crate::ui::icons::regular as ic;
use egui::{Align2, Color32, FontId, Id, Order, Pos2, Rect, Sense, Stroke, TextureId, Vec2, pos2, vec2};

/// A picture shown at the right of an item (brush stroke samples, line widths...).
#[derive(Clone, Copy)]
pub enum Preview {
    Texture(TextureId, Vec2),
    /// A horizontal line of the given thickness.
    Line(f32),
    /// A swatch "none" box (crossed rectangle).
    None,
    /// A solid bar.
    Solid,
}

pub struct Item<A> {
    pub icon: Option<char>,
    pub label: String,
    pub shortcut: Option<String>,
    pub enabled: bool,
    pub checked: bool,
    /// Accent marker for the current choice (Paint shows a small bar at the left).
    pub marked: bool,
    pub preview: Option<Preview>,
    pub action: A,
}

pub enum Entry<A> {
    Item(Item<A>),
    Submenu { icon: Option<char>, label: String, enabled: bool, entries: Vec<Entry<A>> },
    Separator,
}

impl<A> Entry<A> {
    pub fn item(icon: Option<char>, label: &str, action: A) -> Self {
        Entry::Item(Item {
            icon,
            label: label.to_owned(),
            shortcut: None,
            enabled: true,
            checked: false,
            marked: false,
            preview: None,
            action,
        })
    }

    pub fn shortcut(mut self, s: &str) -> Self {
        if let Entry::Item(i) = &mut self {
            i.shortcut = Some(s.to_owned());
        }
        self
    }

    pub fn enabled(mut self, e: bool) -> Self {
        match &mut self {
            Entry::Item(i) => i.enabled = e,
            Entry::Submenu { enabled, .. } => *enabled = e,
            Entry::Separator => {}
        }
        self
    }

    pub fn checked(mut self, c: bool) -> Self {
        if let Entry::Item(i) = &mut self {
            i.checked = c;
        }
        self
    }

    pub fn marked(mut self, m: bool) -> Self {
        if let Entry::Item(i) = &mut self {
            i.marked = m;
        }
        self
    }

    pub fn preview(mut self, p: Preview) -> Self {
        if let Entry::Item(i) = &mut self {
            i.preview = Some(p);
        }
        self
    }

    pub fn submenu(icon: Option<char>, label: &str, entries: Vec<Entry<A>>) -> Self {
        Entry::Submenu { icon, label: label.to_owned(), enabled: true, entries }
    }
}

pub struct MenuOutput<A> {
    pub action: Option<A>,
    /// Screen areas covered by the menu and its open submenus (to detect clicks outside).
    pub rects: Vec<Rect>,
}

const ROW_H: f32 = 34.0;

/// Shows a menu whose top-left corner is at `pos`.
pub fn show<A: Clone>(ctx: &egui::Context, t: &Theme, id: Id, pos: Pos2, entries: &[Entry<A>]) -> MenuOutput<A> {
    let mut out = MenuOutput { action: None, rects: Vec::new() };
    show_level(ctx, t, id, pos, entries, &mut out);
    out
}

fn measure<A>(ctx: &egui::Context, entries: &[Entry<A>]) -> (f32, bool, bool) {
    let font = FontId::proportional(14.0);
    let small = FontId::proportional(12.0);
    let mut has_icon = false;
    let mut has_check = false;
    let mut width: f32 = 0.0;
    ctx.fonts_mut(|f| {
        for e in entries {
            match e {
                Entry::Item(i) => {
                    has_icon |= i.icon.is_some();
                    has_check |= i.checked;
                    let mut w = f.layout_no_wrap(i.label.clone(), font.clone(), Color32::WHITE).size().x;
                    if let Some(s) = &i.shortcut {
                        w += 48.0 + f.layout_no_wrap(s.clone(), small.clone(), Color32::WHITE).size().x;
                    }
                    if let Some(p) = &i.preview {
                        w += 24.0 + preview_width(p);
                    }
                    width = width.max(w);
                }
                Entry::Submenu { icon, label, .. } => {
                    has_icon |= icon.is_some();
                    width = width.max(f.layout_no_wrap(label.clone(), font.clone(), Color32::WHITE).size().x + 40.0);
                }
                Entry::Separator => {}
            }
        }
    });
    (width, has_icon, has_check)
}

fn preview_width(p: &Preview) -> f32 {
    match p {
        Preview::Texture(_, s) => s.x,
        _ => 72.0,
    }
}

fn show_level<A: Clone>(ctx: &egui::Context, t: &Theme, id: Id, pos: Pos2, entries: &[Entry<A>], out: &mut MenuOutput<A>) {
    let (text_w, has_icon, has_check) = measure(ctx, entries);
    let lead = 12.0 + if has_check { 28.0 } else { 0.0 } + if has_icon { 32.0 } else { 0.0 };
    let width = (lead + text_w + 28.0).max(150.0);
    let height: f32 = entries.iter().map(|e| if matches!(e, Entry::Separator) { 9.0 } else { ROW_H }).sum::<f32>() + 8.0;
    let screen = ctx.content_rect();
    let mut pos = pos;
    if pos.x + width > screen.right() - 4.0 {
        pos.x = (screen.right() - width - 4.0).max(screen.left());
    }
    if pos.y + height > screen.bottom() - 4.0 {
        pos.y = (screen.bottom() - height - 4.0).max(screen.top());
    }
    let sub_key = id.with("open-sub");
    let mut open_sub: Option<(usize, Pos2)> = ctx.data(|d| d.get_temp(sub_key));
    let mut rect_out = Rect::NOTHING;
    egui::Area::new(id).order(Order::Foreground).fixed_pos(pos).constrain(false).show(ctx, |ui| {
        widgets::panel_frame(t).show(ui, |ui| {
            ui.set_width(width - 8.0);
            let mut y = ui.cursor().top();
            let left = ui.cursor().left();
            for (i, e) in entries.iter().enumerate() {
                match e {
                    Entry::Separator => {
                        ui.painter().hline(left - 4.0..=left + width - 4.0, y + 4.5, Stroke::new(1.0, t.menu_separator));
                        y += 9.0;
                    }
                    Entry::Item(item) => {
                        let row = Rect::from_min_size(pos2(left, y), vec2(width - 8.0, ROW_H));
                        let resp = ui.interact(row, id.with(i), if item.enabled { Sense::click() } else { Sense::hover() });
                        if resp.hovered() {
                            open_sub = None;
                        }
                        paint_row(ui, t, row, &resp, item.enabled, item.marked);
                        let color = if item.enabled { t.text } else { t.text_disabled };
                        let mut x = row.left() + 12.0;
                        if has_check {
                            if item.checked {
                                paint_icon(ui.painter(), pos2(x + 8.0, row.center().y), ic::CHECKMARK, 16.0, color);
                            }
                            x += 28.0;
                        }
                        if has_icon {
                            if let Some(g) = item.icon {
                                paint_icon(ui.painter(), pos2(x + 9.0, row.center().y), g, 18.0, color);
                            }
                            x += 32.0;
                        }
                        widgets::label(ui.painter(), pos2(x, row.center().y), Align2::LEFT_CENTER, &item.label, 14.0, color);
                        if let Some(s) = &item.shortcut {
                            let sc = if item.enabled { t.text_secondary } else { t.text_disabled };
                            widgets::label(ui.painter(), pos2(row.right() - 12.0, row.center().y), Align2::RIGHT_CENTER, s, 12.0, sc);
                        }
                        if let Some(p) = &item.preview {
                            paint_preview(ui, t, *p, pos2(row.right() - 12.0, row.center().y));
                        }
                        if resp.clicked() && item.enabled {
                            out.action = Some(item.action.clone());
                        }
                        y += ROW_H;
                    }
                    Entry::Submenu { icon, label, enabled, .. } => {
                        let row = Rect::from_min_size(pos2(left, y), vec2(width - 8.0, ROW_H));
                        let resp = ui.interact(row, id.with(i), Sense::hover());
                        let is_open = open_sub.is_some_and(|(o, _)| o == i);
                        if resp.hovered() && *enabled {
                            open_sub = Some((i, pos2(pos.x + width - 4.0, y - 4.0)));
                        }
                        let p = ui.painter();
                        if *enabled && (resp.hovered() || is_open) {
                            p.rect_filled(row, widgets::RADIUS, t.menu_hover);
                        }
                        let color = if *enabled { t.text } else { t.text_disabled };
                        let mut x = row.left() + 12.0 + if has_check { 28.0 } else { 0.0 };
                        if has_icon {
                            if let Some(g) = icon {
                                paint_icon(p, pos2(x + 9.0, row.center().y), *g, 18.0, color);
                            }
                            x += 32.0;
                        }
                        widgets::label(p, pos2(x, row.center().y), Align2::LEFT_CENTER, label, 14.0, color);
                        paint_icon(p, pos2(row.right() - 16.0, row.center().y), ic::CHEVRON_RIGHT, 12.0, color);
                        y += ROW_H;
                    }
                }
            }
            ui.allocate_space(vec2(width - 8.0, y - ui.cursor().top()));
        });
        rect_out = ui.min_rect();
    });
    out.rects.push(rect_out);
    ctx.data_mut(|d| {
        if let Some(v) = open_sub {
            d.insert_temp(sub_key, v);
        } else {
            d.remove::<(usize, Pos2)>(sub_key);
        }
    });
    if let Some((i, at)) = open_sub
        && let Some(Entry::Submenu { entries: sub, .. }) = entries.get(i)
    {
        show_level(ctx, t, id.with(("sub", i)), at, sub, out);
    }
}

fn paint_row(ui: &egui::Ui, t: &Theme, row: Rect, resp: &egui::Response, enabled: bool, marked: bool) {
    let p = ui.painter();
    if enabled && resp.hovered() {
        p.rect_filled(row, widgets::RADIUS, t.menu_hover);
    }
    if marked {
        if !resp.hovered() {
            p.rect_filled(row, widgets::RADIUS, t.menu_hover);
        }
        let bar = Rect::from_center_size(pos2(row.left() + 1.5, row.center().y), vec2(3.0, 16.0));
        p.rect_filled(bar, 2, t.accent);
    }
}

fn paint_preview(ui: &egui::Ui, t: &Theme, p: Preview, right_center: Pos2) {
    let painter = ui.painter();
    match p {
        Preview::Texture(id, size) => {
            let r = Rect::from_min_size(pos2(right_center.x - size.x, right_center.y - size.y / 2.0), size);
            painter.image(id, r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        Preview::Line(w) => {
            painter.line_segment([pos2(right_center.x - 92.0, right_center.y), right_center], Stroke::new(w, t.text_secondary));
        }
        Preview::Solid => {
            let r = Rect::from_min_size(pos2(right_center.x - 72.0, right_center.y - 8.0), vec2(72.0, 16.0));
            painter.rect_filled(r, 1, t.text_secondary);
        }
        Preview::None => {
            let r = Rect::from_min_size(pos2(right_center.x - 72.0, right_center.y - 11.0), vec2(72.0, 22.0));
            painter.rect_stroke(r, 3, Stroke::new(1.0, t.text_secondary), egui::StrokeKind::Inside);
            painter.line_segment(
                [r.left_bottom() + vec2(2.0, -2.0), r.right_top() + vec2(-2.0, 2.0)],
                Stroke::new(1.0, Color32::from_rgb(0xE8, 0x80, 0x80)),
            );
        }
    }
}

/// Clears the remembered submenu state of a menu (call when it closes).
pub fn reset(ctx: &egui::Context, id: Id) {
    ctx.data_mut(|d| d.remove::<(usize, Pos2)>(id.with("open-sub")));
}
