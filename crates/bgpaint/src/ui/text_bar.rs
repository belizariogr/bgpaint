//! The floating text formatting bar shown while the Text tool is active.

use super::icons::regular as ic;
use super::widgets::{self, IconButton};
use crate::app::PaintApp;
use crate::tools::Tool;
use bgpaint_core::text::TextAlign;
use egui::{Align2, Id, Rect, Sense, Stroke, pos2, vec2};

const SIZES: [f32; 16] = [8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 36.0, 48.0, 72.0];

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Popup {
    #[default]
    None,
    Font,
    Size,
}

impl PaintApp {
    pub fn tool_bars(&mut self, ctx: &egui::Context) {
        if self.tool != Tool::Text {
            return;
        }
        let t = self.theme.clone();
        let tr = self.tr;
        let popup_id = Id::unique("text-bar-popup");
        let mut popup: Popup = ctx.data(|d| d.get_temp(popup_id)).unwrap_or_default();
        let top = crate::ui::menubar::menubar_height() + if self.ribbon_visible() { crate::ui::ribbon::RIBBON_HEIGHT } else { 10.0 } + 4.0;
        let width = 710.0;
        let screen = ctx.content_rect();
        let left = (screen.center().x - width / 2.0).max(screen.left() + 8.0);
        let mut changed = false;
        let mut font_rect = Rect::NOTHING;
        let mut size_rect = Rect::NOTHING;
        let family = self.text_settings.family.clone().unwrap_or_else(|| crate::tools::text::fonts().default_family());
        egui::Area::new(Id::unique("text-bar")).order(egui::Order::Middle).fixed_pos(pos2(left, top)).show(ctx, |ui| {
            widgets::panel_frame(&t).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                let (r, _) = ui.allocate_exact_size(vec2(width - 16.0, 32.0), Sense::hover());
                let cy = r.center().y;
                let mut x = r.left();
                // Font family
                font_rect = Rect::from_min_size(pos2(x, cy - 15.0), vec2(170.0, 30.0));
                let resp = ui.interact(font_rect, Id::unique("tb-font"), Sense::click());
                ui.painter().rect(
                    font_rect,
                    widgets::RADIUS,
                    if resp.hovered() { t.button_hover } else { t.input_bg },
                    Stroke::new(1.0, t.input_border),
                    egui::StrokeKind::Inside,
                );
                let name = truncate(&family, 20);
                widgets::label(ui.painter(), pos2(font_rect.left() + 10.0, cy), Align2::LEFT_CENTER, &name, 13.0, t.text);
                widgets::paint_chevron(ui.painter(), pos2(font_rect.right() - 14.0, cy), t.text, 12.0);
                if resp.on_hover_text(tr.font_family).clicked() {
                    popup = if popup == Popup::Font { Popup::None } else { Popup::Font };
                }
                x = font_rect.right() + 8.0;
                // Font size
                size_rect = Rect::from_min_size(pos2(x, cy - 15.0), vec2(64.0, 30.0));
                let resp = ui.interact(size_rect, Id::unique("tb-size"), Sense::click());
                ui.painter().rect(
                    size_rect,
                    widgets::RADIUS,
                    if resp.hovered() { t.button_hover } else { t.input_bg },
                    Stroke::new(1.0, t.input_border),
                    egui::StrokeKind::Inside,
                );
                widgets::label(
                    ui.painter(),
                    pos2(size_rect.left() + 10.0, cy),
                    Align2::LEFT_CENTER,
                    &format!("{}", self.text_settings.size_pt),
                    13.0,
                    t.text,
                );
                widgets::paint_chevron(ui.painter(), pos2(size_rect.right() - 14.0, cy), t.text, 12.0);
                if resp.on_hover_text(tr.font_size).clicked() {
                    popup = if popup == Popup::Size { Popup::None } else { Popup::Size };
                }
                x = size_rect.right() + 12.0;
                widgets::vseparator(ui.painter(), &t, x, cy - 12.0, cy + 12.0);
                x += 10.0;
                let s = &mut self.text_settings;
                for (glyph, tip, value) in [
                    (ic::BOLD, tr.bold, &mut s.bold),
                    (ic::ITALIC, tr.italic, &mut s.italic),
                    (ic::UNDERLINE, tr.underline, &mut s.underline),
                    (ic::STRIKETHROUGH, tr.strikethrough, &mut s.strikethrough),
                ] {
                    let rect = Rect::from_min_size(pos2(x, cy - 15.0), vec2(32.0, 30.0));
                    if IconButton::new(glyph).size(18.0).selected(*value).tooltip(tip).show(ui, &t, rect, Id::unique(("tb", tip))).clicked()
                    {
                        *value = !*value;
                        changed = true;
                    }
                    x += 36.0;
                }
                x += 4.0;
                widgets::vseparator(ui.painter(), &t, x, cy - 12.0, cy + 12.0);
                x += 10.0;
                for (glyph, tip, align) in [
                    (ic::ALIGN_LEFT, tr.align_left, TextAlign::Left),
                    (ic::ALIGN_CENTER, tr.align_center, TextAlign::Center),
                    (ic::ALIGN_RIGHT, tr.align_right, TextAlign::Right),
                ] {
                    let rect = Rect::from_min_size(pos2(x, cy - 15.0), vec2(32.0, 30.0));
                    if IconButton::new(glyph)
                        .size(18.0)
                        .selected(s.align == align)
                        .tooltip(tip)
                        .show(ui, &t, rect, Id::unique(("tb-align", tip)))
                        .clicked()
                    {
                        s.align = align;
                        changed = true;
                    }
                    x += 36.0;
                }
                x += 4.0;
                widgets::vseparator(ui.painter(), &t, x, cy - 12.0, cy + 12.0);
                x += 10.0;
                let check = Rect::from_min_max(pos2(x, cy - 16.0), pos2(r.right(), cy + 16.0));
                let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(check));
                let before = s.background;
                widgets::checkbox(&mut cui, &t, &mut s.background, tr.background_fill);
                changed |= before != s.background;
            });
        });
        match popup {
            Popup::Font => {
                if let Some(f) = self.font_popup(ctx, font_rect) {
                    self.text_settings.family = Some(f);
                    popup = Popup::None;
                    changed = true;
                }
            }
            Popup::Size => {
                let entries: Vec<super::menu::Entry<f32>> = SIZES
                    .iter()
                    .map(|&s| super::menu::Entry::item(None, &format!("{s}"), s).marked((s - self.text_settings.size_pt).abs() < 0.01))
                    .collect();
                let out =
                    super::menu::show(ctx, &t, Id::unique("tb-size-menu"), pos2(size_rect.left(), size_rect.bottom() + 6.0), &entries);
                if let Some(s) = out.action {
                    self.text_settings.size_pt = s;
                    popup = Popup::None;
                    changed = true;
                } else if clicked_outside(ctx, &out.rects, size_rect) {
                    popup = Popup::None;
                }
            }
            Popup::None => {}
        }
        if popup == Popup::Font
            && clicked_outside(ctx, &[Rect::from_min_size(pos2(font_rect.left(), font_rect.bottom() + 6.0), vec2(280.0, 360.0))], font_rect)
        {
            popup = Popup::None;
        }
        ctx.data_mut(|d| d.insert_temp(popup_id, popup));
        if changed {
            self.restyle_text();
        }
    }

    /// Searchable list of installed font families.
    fn font_popup(&mut self, ctx: &egui::Context, anchor: Rect) -> Option<String> {
        let t = self.theme.clone();
        let filter_id = Id::unique("font-filter");
        let mut filter: String = ctx.data(|d| d.get_temp(filter_id)).unwrap_or_default();
        let families: Vec<String> = {
            let f = crate::tools::text::fonts();
            let q = filter.to_lowercase();
            f.families().iter().filter(|n| q.is_empty() || n.to_lowercase().contains(&q)).cloned().collect()
        };
        let current = self.text_settings.family.clone();
        let mut picked = None;
        egui::Area::new(Id::unique("font-popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(anchor.left(), anchor.bottom() + 6.0))
            .show(ctx, |ui| {
                widgets::panel_frame(&t).show(ui, |ui| {
                    ui.set_width(272.0);
                    let resp = widgets::text_field(ui, &t, &mut filter, 272.0, Id::unique("font-filter-edit"));
                    if !resp.has_focus() && filter.is_empty() {
                        resp.request_focus();
                    }
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for name in &families {
                            let (rect, r) = ui.allocate_exact_size(vec2(264.0, 30.0), Sense::click());
                            let selected = current.as_deref() == Some(name.as_str());
                            if r.hovered() || selected {
                                ui.painter().rect_filled(rect, widgets::RADIUS, t.menu_hover);
                            }
                            widgets::label(
                                ui.painter(),
                                pos2(rect.left() + 10.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                name,
                                13.0,
                                t.text,
                            );
                            if r.clicked() {
                                picked = Some(name.clone());
                            }
                        }
                    });
                });
            });
        ctx.data_mut(|d| d.insert_temp(filter_id, if picked.is_some() { String::new() } else { filter }));
        picked
    }
}

fn clicked_outside(ctx: &egui::Context, rects: &[Rect], anchor: Rect) -> bool {
    ctx.input(|i| {
        i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !rects.iter().any(|r| r.contains(p)) && !anchor.contains(p))
    })
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_owned() } else { format!("{}…", s.chars().take(max - 1).collect::<String>()) }
}
