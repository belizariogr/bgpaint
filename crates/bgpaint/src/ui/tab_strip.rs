//! The document tabs next to the menu: click to switch, middle-click or × to close, drag to
//! reorder, + for a new image.

use super::icons::regular as ic;
use super::widgets::{self, IconButton};
use crate::app::PaintApp;
use crate::commands::{Command, shortcut_label};
use crate::theme::Theme;
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{CornerRadius, FontId, Id, Rect, Sense, Stroke, pos2, vec2};

const TAB_MIN: f32 = 96.0;
const TAB_MAX: f32 = 220.0;
const PLUS: f32 = 36.0;
/// Gap above the tabs, inside the row.
const TAB_TOP: f32 = if cfg!(target_os = "macos") { 3.0 } else { 6.0 };

/// Draws the tabs inside `area` and returns the command a click produced.
pub fn show(app: &mut PaintApp, ui: &mut egui::Ui, t: &Theme, area: Rect) -> Option<Command> {
    let ctx = ui.ctx().clone();
    let tabs = app.tab_infos();
    let n = tabs.len();
    let avail = (area.width() - PLUS - 4.0).max(TAB_MIN);
    let tab_w = (avail / n as f32).clamp(TAB_MIN, TAB_MAX);
    let total = tab_w * n as f32;
    let max_scroll = (total - avail).max(0.0);

    // Horizontal scroll when the tabs do not fit; follows the active tab when it changes.
    let scroll_id = Id::unique("tab-scroll");
    let (mut scroll, last_current): (f32, usize) = ctx.data(|d| d.get_temp(scroll_id)).unwrap_or((0.0, usize::MAX));
    if ui.rect_contains_pointer(area) {
        let d = ctx.input(|i| i.smooth_scroll_delta);
        scroll -= d.x + d.y;
    }
    if last_current != app.current_tab {
        let left = app.current_tab as f32 * tab_w;
        scroll = scroll.min(left).max(left + tab_w - avail);
    }
    scroll = scroll.clamp(0.0, max_scroll);
    ctx.data_mut(|d| d.insert_temp(scroll_id, (scroll, app.current_tab)));

    let clip = Rect::from_min_max(area.min, pos2(area.left() + avail.min(area.width()), area.bottom()));
    let painter = ui.painter_at(clip);
    let mut action = None;
    app.ui.active_tab_span = None;
    for (i, tab) in tabs.iter().enumerate() {
        let x = area.left() + i as f32 * tab_w - scroll;
        let rect = Rect::from_min_max(pos2(x, area.top() + TAB_TOP), pos2(x + tab_w, area.bottom()));
        if rect.right() < clip.left() || rect.left() > clip.right() {
            continue;
        }
        let visible = rect.intersect(clip);
        let selected = i == app.current_tab;
        let resp = ui.interact(visible, Id::unique(("tab", tab.id)), Sense::click_and_drag());
        let close = Rect::from_center_size(pos2(rect.right() - 18.0, rect.center().y), vec2(24.0, 24.0));
        let close_resp = ui.interact(close.intersect(clip), Id::unique(("tab-close", tab.id)), Sense::click());
        let hovered = resp.hovered() || close_resp.hovered();

        if selected {
            let shape = egui::epaint::RectShape::new(
                rect,
                CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 },
                t.ribbon_bg,
                Stroke::new(1.0, t.ribbon_border),
                egui::StrokeKind::Inside,
            );
            painter.add(shape);
            // Open the bottom edge so the tab flows into the toolbar below.
            painter.hline(rect.left() + 1.0..=rect.right() - 1.0, rect.bottom() - 0.5, Stroke::new(1.0, t.ribbon_bg));
            app.ui.active_tab_span = Some(visible.x_range());
        } else {
            if hovered {
                painter.rect_filled(rect.shrink2(vec2(2.0, 3.0)), widgets::RADIUS, t.hover);
            }
            // Thin divider between two inactive tabs.
            if i + 1 < n && i + 1 != app.current_tab && !hovered {
                painter.vline(rect.right(), rect.center().y - 8.0..=rect.center().y + 8.0, Stroke::new(1.0, t.separator));
            }
        }

        let text_left = rect.left() + 12.0;
        let text_w = (close.left() - 4.0 - text_left).max(8.0);
        let mut job = LayoutJob::single_section(
            tab.name.clone(),
            TextFormat { font_id: FontId::proportional(13.0), color: t.text, ..Default::default() },
        );
        job.wrap = TextWrapping { max_width: text_w, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
        let galley = painter.layout_job(job);
        painter.galley(pos2(text_left, rect.center().y - galley.size().y / 2.0), galley, t.text);

        if hovered || selected {
            if close_resp.hovered() {
                painter.rect_filled(close.shrink(2.0), widgets::RADIUS, if t.dark { t.hover } else { t.pressed });
            }
            widgets::paint_icon(&painter, close.center(), ic::DISMISS, 12.0, t.text);
        } else if tab.modified {
            painter.circle_filled(close.center(), 3.5, t.text_secondary);
        }
        if close_resp.on_hover_text(app.tr.close).clicked() || resp.middle_clicked() {
            action = Some(Command::CloseTab(i));
        } else if !selected && (resp.clicked() || resp.drag_started()) {
            action = Some(Command::SwitchTab(i));
        }
        if selected
            && resp.dragged()
            && let Some(p) = ctx.input(|i| i.pointer.interact_pos())
        {
            let target = ((p.x - area.left() + scroll) / tab_w).floor().clamp(0.0, (n - 1) as f32) as usize;
            if target != i {
                app.move_current_tab(target);
            }
        }
        let tip = tab.path.as_ref().map_or_else(|| tab.name.clone(), |p| p.display().to_string());
        let tip = if tab.modified { format!("{tip} •") } else { tip };
        resp.on_hover_text(tip);
    }

    // New tab.
    let px = (area.left() + total - scroll).min(clip.right()) + 4.0;
    let side = (area.height() - TAB_TOP).min(28.0);
    let plus = Rect::from_min_size(pos2(px, (area.top() + TAB_TOP + area.bottom() - side) / 2.0), vec2(side, side));
    let tip = match shortcut_label(&ctx, &Command::New) {
        Some(s) => format!("{} ({s})", app.tr.new),
        None => app.tr.new.to_owned(),
    };
    if IconButton::new(ic::ADD).size(16.0).tooltip(&tip).show(ui, t, plus, Id::unique("tab-new")).clicked() {
        action = Some(Command::New);
    }
    action
}
