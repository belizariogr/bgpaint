//! The toolbar ("ribbon") with Paint's groups: Selection, Image, Tools, Brushes, Shapes, Colors
//! and Layers. Positions are measured from Paint at 100% scale.

use super::icons::{filled as icf, regular as ic};
use super::menu::{Entry, Preview};
use super::menubar::MenuKind;
use super::widgets::{self, IconButton};
use crate::app::{PaintApp, Swatch};
use crate::commands::Command;
use crate::settings::{FlipDefault, RotateDefault};
use crate::theme::Theme;
use crate::tools::Tool;
use bgpaint_core::brush::{BlendMode, BrushKind, Stroke, Texture, apply_mask};
use bgpaint_core::color::Color;
use bgpaint_core::geom::{PixelRect, Point, RectF};
use bgpaint_core::raster::{self, QuarterTurn};
use bgpaint_core::shapes::{self, ShapeKind};
use egui::{Align2, Color32, Id, Pos2, Rect, Sense, Stroke as EStroke, pos2, vec2};

pub const RIBBON_HEIGHT: f32 = 106.0;
/// x, y, outline glyph, filled glyph, tool and tooltip of a Tools-group button.
type ToolButton<'a> = (f32, f32, char, Option<char>, Tool, &'a str);

/// Extra width of the Image group for the Undo/Redo column.
const IMAGE_EXTRA: f32 = 36.0;
/// Extra width of the Shapes group in vector mode, for the Pointer button.
const POINTER_WIDTH: f32 = 40.0;
/// Room kept at the right of the toolbar for the collapse chevron.
const COLLAPSE_SPACE: f32 = 24.0;
/// Width of the More button shown when some groups do not fit.
const MORE_WIDTH: f32 = 44.0;

/// The toolbar groups, left to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RibbonGroup {
    Selection,
    Image,
    Tools,
    Brushes,
    Shapes,
    Colors,
    Layers,
    Vector,
}

impl RibbonGroup {
    const ALL: [RibbonGroup; 8] = [
        RibbonGroup::Selection,
        RibbonGroup::Image,
        RibbonGroup::Tools,
        RibbonGroup::Brushes,
        RibbonGroup::Shapes,
        RibbonGroup::Colors,
        RibbonGroup::Layers,
        RibbonGroup::Vector,
    ];

    /// Where the group starts and ends in its drawing coordinates (the groups after Image were
    /// drawn shifted by [`IMAGE_EXTRA`], which their coordinates keep).
    fn span(self) -> (f32, f32) {
        match self {
            RibbonGroup::Selection => (0.0, 66.0),
            RibbonGroup::Image => (66.0, 224.0 + IMAGE_EXTRA),
            RibbonGroup::Tools => (224.0, 365.0),
            RibbonGroup::Brushes => (365.0, 427.0),
            RibbonGroup::Shapes => (427.0, 668.0),
            RibbonGroup::Colors => (668.0, 1021.0),
            RibbonGroup::Layers => (1021.0, 1099.0),
            RibbonGroup::Vector => (1099.0, 1166.0),
        }
    }

    fn start(self) -> f32 {
        self.span().0
    }

    fn width(self, vector_mode: bool) -> f32 {
        let (a, b) = self.span();
        b - a + if self == RibbonGroup::Shapes && vector_mode { POINTER_WIDTH } else { 0.0 }
    }
}

/// The 20 colors of Paint's palette (two rows).
pub const PALETTE: [u32; 20] = [
    0x000000, 0x7F7F7F, 0x880015, 0xED1C24, 0xFF7F27, 0xFFF200, 0x22B14C, 0x00A2E8, 0x3F48CC, 0xA349A4, 0xFFFFFF, 0xC3C3C3, 0xB97A57,
    0xFFAEC9, 0xFFC90E, 0xEFE4B0, 0xB5E61D, 0x99D9EA, 0x7092BE, 0xC8BFE7,
];

pub fn to_egui(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

impl PaintApp {
    /// A choice from the ribbon's Rotate or Flip menu becomes what its button does from now on.
    /// "Custom rotation" keeps the previous default.
    pub fn remember_ribbon_default(&mut self, menu: MenuKind, cmd: &Command) {
        let (rotate, flip) = match (menu, cmd) {
            (MenuKind::Rotate, Command::Rotate(QuarterTurn::Cw90)) => (Some(RotateDefault::Right), None),
            (MenuKind::Rotate, Command::Rotate(QuarterTurn::Ccw90)) => (Some(RotateDefault::Left), None),
            (MenuKind::Rotate, Command::Rotate(QuarterTurn::Half)) => (Some(RotateDefault::Half), None),
            (MenuKind::Flip, Command::FlipHorizontal) => (None, Some(FlipDefault::Horizontal)),
            (MenuKind::Flip, Command::FlipVertical) => (None, Some(FlipDefault::Vertical)),
            _ => return,
        };
        let changed = rotate.is_some_and(|r| r != self.settings.rotate_default) || flip.is_some_and(|f| f != self.settings.flip_default);
        if changed {
            self.settings.rotate_default = rotate.unwrap_or(self.settings.rotate_default);
            self.settings.flip_default = flip.unwrap_or(self.settings.flip_default);
            self.settings.save();
        }
    }

    pub fn ribbon_visible(&self) -> bool {
        !(self.settings.auto_hide_toolbar || self.ui.ribbon_collapsed) || self.ui.ribbon_revealed
    }

    pub fn ribbon_panel(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        if !self.ribbon_visible() {
            // A slim strip with the chevron to bring the toolbar back.
            egui::Panel::top("ribbon-collapsed")
                .exact_size(10.0)
                .show_separator_line(false)
                .frame(egui::Frame::NONE.fill(t.window_bg))
                .show(ui, |_| {});
            return;
        }
        egui::Panel::top("ribbon").exact_size(RIBBON_HEIGHT).show_separator_line(false).frame(egui::Frame::NONE.fill(t.ribbon_bg)).show(
            ui,
            |ui| {
                let r = ui.max_rect();
                ui.painter().hline(r.x_range(), r.bottom() - 0.5, EStroke::new(1.0, t.ribbon_border));
                let border = EStroke::new(1.0, t.ribbon_border);
                match self.ui.active_tab_span {
                    Some(span) => {
                        ui.painter().hline(r.left()..=span.min + 0.5, r.top() + 0.5, border);
                        ui.painter().hline(span.max - 0.5..=r.right(), r.top() + 0.5, border);
                    }
                    None => {
                        ui.painter().hline(r.x_range(), r.top() + 0.5, border);
                    }
                }
                self.ribbon_contents(ui, &t, r);
            },
        );
    }

    fn ribbon_contents(&mut self, ui: &mut egui::Ui, t: &Theme, r: Rect) {
        // Scripted tests can pretend the window is narrower (a tiling window manager may not
        // let them resize it).
        #[cfg(feature = "automation")]
        let r = match std::env::var("BGPAINT_RIBBON_WIDTH").ok().and_then(|v| v.parse::<f32>().ok()) {
            Some(w) => Rect::from_min_size(r.min, vec2(w.min(r.width()), r.height())),
            None => r,
        };
        let ctx = ui.ctx().clone();
        let top = r.top();
        let label_y = top + 91.0;
        let tr = self.tr;
        let vector = self.doc.vector_mode();
        let groups = RibbonGroup::ALL;
        // Groups go left to right; when the window is too narrow, the ones that no longer fit
        // move into a block that the More button opens below the toolbar.
        let right = r.right() - COLLAPSE_SPACE;
        let total: f32 = groups.iter().map(|g| g.width(vector)).sum();
        let limit = if r.left() + total <= right { right } else { right - MORE_WIDTH };
        let mut x = r.left();
        let mut shown = 0;
        for g in groups {
            if x + g.width(vector) > limit {
                break;
            }
            self.draw_ribbon_group(g, ui, t, x, top);
            x += g.width(vector);
            if shown + 1 < groups.len() {
                widgets::vseparator(ui.painter(), t, x, top + 10.0, top + 98.0);
            }
            shown += 1;
        }
        let hidden = &groups[shown..];
        if hidden.is_empty() {
            self.ui.ribbon_overflow = false;
        } else {
            let more = Rect::from_min_size(pos2(x + 4.0, top + 8.0), vec2(MORE_WIDTH - 8.0, 76.0));
            let resp = ui.interact(more, Id::unique("rb-more"), Sense::click());
            let fill = if self.ui.ribbon_overflow { t.hover } else { widgets::state_fill(t, &resp, true) };
            ui.painter().rect_filled(more, widgets::RADIUS, fill);
            widgets::paint_icon(ui.painter(), more.center() - vec2(0.0, 6.0), ic::MORE, 20.0, t.text);
            widgets::paint_chevron(ui.painter(), more.center() + vec2(0.0, 16.0), t.text, 12.0);
            if resp.on_hover_text(tr.more_tools).clicked() {
                self.ui.ribbon_overflow = !self.ui.ribbon_overflow;
            }
            if self.ui.ribbon_overflow {
                self.ribbon_overflow_block(&ctx, t, hidden, r, more);
            }
        }

        // Collapse chevron at the bottom right.
        let cr = Rect::from_center_size(pos2(r.right() - 16.0, label_y), vec2(24.0, 20.0));
        let resp = ui.interact(cr, Id::unique("rb-collapse"), Sense::click());
        let fill = widgets::state_fill(t, &resp, true);
        ui.painter().rect_filled(cr, widgets::RADIUS, fill);
        let collapsed = self.ui.ribbon_collapsed || self.settings.auto_hide_toolbar;
        widgets::paint_icon(ui.painter(), cr.center(), if collapsed { ic::CHEVRON_DOWN } else { ic::CHEVRON_UP }, 12.0, t.text);
        if resp.on_hover_text(if collapsed { tr.expand_toolbar } else { tr.collapse_toolbar }).clicked() {
            if self.settings.auto_hide_toolbar {
                self.settings.auto_hide_toolbar = false;
                self.ui.ribbon_collapsed = false;
            } else {
                self.ui.ribbon_collapsed = !self.ui.ribbon_collapsed;
            }
            self.ui.ribbon_revealed = false;
        }
    }

    /// Draws one toolbar group whose left edge is at `x` (screen coordinates).
    fn draw_ribbon_group(&mut self, g: RibbonGroup, ui: &mut egui::Ui, t: &Theme, x: f32, top: f32) {
        // The groups are laid out in their own coordinates, starting at `g.start()`.
        let x0 = x - g.start();
        match g {
            RibbonGroup::Selection => self.rb_selection(ui, t, x0, top),
            RibbonGroup::Image => self.rb_image(ui, t, x0, top),
            RibbonGroup::Tools => self.rb_tools(ui, t, x0, top),
            RibbonGroup::Brushes => self.rb_brushes(ui, t, x0, top),
            RibbonGroup::Shapes => self.rb_shapes(ui, t, x0, top),
            RibbonGroup::Colors => self.rb_colors(ui, t, x0, top),
            RibbonGroup::Layers => self.rb_layers(ui, t, x0, top),
            RibbonGroup::Vector => self.rb_vector(ui, t, x0, top),
        }
    }

    /// The groups that do not fit in the toolbar, in a block below it (wrapping into more rows
    /// when the window is narrower still). A click anywhere else closes it.
    fn ribbon_overflow_block(&mut self, ctx: &egui::Context, t: &Theme, hidden: &[RibbonGroup], ribbon: Rect, more: Rect) {
        let vector = self.doc.vector_mode();
        let pad = 6.0;
        let max_row = (ribbon.width() - 16.0).max(200.0);
        // Rows of groups, each no wider than the window.
        let mut rows: Vec<Vec<RibbonGroup>> = vec![Vec::new()];
        let mut row_w = 0.0;
        for &g in hidden {
            let w = g.width(vector);
            if row_w > 0.0 && row_w + w > max_row {
                rows.push(Vec::new());
                row_w = 0.0;
            }
            rows.last_mut().unwrap().push(g);
            row_w += w;
        }
        let width = rows.iter().map(|r| r.iter().map(|g| g.width(vector)).sum::<f32>()).fold(0.0, f32::max) + 2.0 * pad;
        let height = rows.len() as f32 * RIBBON_HEIGHT + 2.0 * pad;
        let left = (more.right() - width).clamp(ribbon.left() + 8.0, (ribbon.right() - 8.0 - width).max(ribbon.left() + 8.0));
        let block = Rect::from_min_size(pos2(left, ribbon.bottom() + 2.0), vec2(width, height));
        egui::Area::new(Id::unique("ribbon-overflow")).order(egui::Order::Middle).fixed_pos(block.min).show(ctx, |ui| {
            ui.allocate_rect(Rect::from_min_size(block.min, block.size()), Sense::hover());
            ui.painter().add(egui::Shadow { offset: [0, 4], blur: 12, spread: 0, color: t.canvas_shadow }.as_shape(block, 8));
            ui.painter().rect(block, 8, t.ribbon_bg, EStroke::new(1.0, t.ribbon_border), egui::StrokeKind::Inside);
            for (i, row) in rows.iter().enumerate() {
                let top = block.top() + pad + i as f32 * RIBBON_HEIGHT;
                let mut x = block.left() + pad;
                for (j, &g) in row.iter().enumerate() {
                    self.draw_ribbon_group(g, ui, t, x, top);
                    x += g.width(vector);
                    if j + 1 < row.len() {
                        widgets::vseparator(ui.painter(), t, x, top + 10.0, top + 98.0);
                    }
                }
            }
        });
        // Close on a click outside (a menu opened from the block takes its own clicks).
        let clicked_outside =
            ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !block.contains(p) && !more.contains(p)));
        if clicked_outside && self.ui.open_menu.is_none() {
            self.ui.ribbon_overflow = false;
        }
    }

    fn rb_selection(&mut self, ui: &mut egui::Ui, t: &Theme, x0: f32, top: f32) {
        let ctx = ui.ctx().clone();
        let label_y = top + 91.0;
        let at = |x: f32, y: f32| pos2(x0 + x, y);
        let group_label = |ui: &egui::Ui, cx: f32, text: &str| {
            widgets::label(ui.painter(), pos2(x0 + cx, label_y), Align2::CENTER_CENTER, text, 12.0, t.text_secondary);
        };
        let tr = self.tr;
        // --- Selection ---
        let sel_rect = Rect::from_center_size(at(32.0, top + 42.0), vec2(40.0, 64.0));
        let resp = ui.interact(sel_rect, Id::unique("rb-select"), Sense::click());
        let selected = self.tool == Tool::Select;
        let open = self.ui.open_menu == Some(MenuKind::Selection);
        let fill = if open { t.hover } else { widgets::state_fill(t, &resp, true) };
        if selected {
            ui.painter().rect(sel_rect, widgets::RADIUS, t.hover, EStroke::new(1.0, t.selected_border), egui::StrokeKind::Inside);
        } else {
            ui.painter().rect_filled(sel_rect, widgets::RADIUS, fill);
        }
        let glyph = if self.tools.select_mode == crate::tools::SelectMode::FreeForm { ic::LASSO } else { ic::SELECT };
        widgets::paint_icon(ui.painter(), at(32.0, top + 32.0), glyph, 24.0, t.text);
        widgets::paint_chevron(ui.painter(), at(32.0, top + 60.0), t.text, 12.0);
        // Outside vector mode the V key picks the Select tool.
        let vector = self.doc.vector_mode();
        if !vector {
            widgets::paint_hotkey_hint(ui.painter(), sel_rect.shrink(2.0), "V", t.text_secondary);
        }
        let tip = if vector { tr.group_selection.to_owned() } else { self.tool_tooltip(&ctx, tr.group_selection, Some(&Command::Pointer)) };
        let resp = resp.on_hover_text(tip);
        if resp.clicked() {
            if resp.interact_pointer_pos().is_some_and(|p| p.y > top + 48.0) || selected {
                self.toggle_menu(MenuKind::Selection, sel_rect);
            } else {
                self.execute(&ctx, Command::SetTool(Tool::Select));
            }
        }
        group_label(ui, 32.0, tr.group_selection);
    }

    fn rb_image(&mut self, ui: &mut egui::Ui, t: &Theme, x0: f32, top: f32) {
        let ctx = ui.ctx().clone();
        let row1 = top + 23.0;
        let row2 = top + 61.0;
        let label_y = top + 91.0;
        let at = |x: f32, y: f32| pos2(x0 + x, y);
        let btn = |x: f32, y: f32| widgets::square(pos2(x0 + x, y), 32.0);
        let group_label = |ui: &egui::Ui, cx: f32, text: &str| {
            widgets::label(ui.painter(), pos2(x0 + cx, label_y), Align2::CENTER_CENTER, text, 12.0, t.text_secondary);
        };
        let tr = self.tr;
        // --- Image ---
        let has_sel = self.has_selection();
        if IconButton::new(ic::CROP)
            .enabled(has_sel)
            .tooltip(&self.tool_tooltip(&ctx, tr.crop, Some(&Command::Crop)))
            .show(ui, t, btn(94.0, row1), Id::unique("rb-crop"))
            .clicked()
        {
            self.execute(&ctx, Command::Crop);
        }
        // Rotate and Flip run their last choice from the menu right away; the chevron opens the menu.
        let rot = btn(128.0, row1);
        let (glyph, tip, turn) = match self.settings.rotate_default {
            RotateDefault::Right => (ic::ROTATE_RIGHT, tr.rotate_right, QuarterTurn::Cw90),
            RotateDefault::Left => (ic::ROTATE_LEFT, tr.rotate_left, QuarterTurn::Ccw90),
            RotateDefault::Half => (ic::ROTATE_CUSTOM, tr.rotate_180, QuarterTurn::Half),
        };
        if IconButton::new(glyph).tooltip(tip).show(ui, t, rot, Id::unique("rb-rot")).clicked() {
            self.execute(&ctx, Command::Rotate(turn));
        }
        let ch = Rect::from_center_size(at(155.0, row1), vec2(16.0, 32.0));
        if widgets::chevron_button(ui, t, ch, Id::unique("rb-rot-ch"), true, self.ui.open_menu == Some(MenuKind::Rotate)).clicked() {
            self.toggle_menu(MenuKind::Rotate, rot.union(ch));
        }
        if IconButton::new(ic::INVERT_COLOR)
            .tooltip(&self.tool_tooltip(&ctx, tr.invert_color, Some(&Command::InvertColors)))
            .show(ui, t, btn(94.0, row2), Id::unique("rb-invert"))
            .clicked()
        {
            self.execute(&ctx, Command::InvertColors);
        }
        let flip = btn(128.0, row2);
        let (glyph, tip, cmd) = match self.settings.flip_default {
            FlipDefault::Horizontal => (ic::FLIP_H, tr.flip_horizontal, Command::FlipHorizontal),
            FlipDefault::Vertical => (ic::FLIP_V, tr.flip_vertical, Command::FlipVertical),
        };
        if IconButton::new(glyph).tooltip(tip).show(ui, t, flip, Id::unique("rb-flip")).clicked() {
            self.execute(&ctx, cmd);
        }
        let ch = Rect::from_center_size(at(155.0, row2), vec2(16.0, 32.0));
        if widgets::chevron_button(ui, t, ch, Id::unique("rb-flip-ch"), true, self.ui.open_menu == Some(MenuKind::Flip)).clicked() {
            self.toggle_menu(MenuKind::Flip, flip.union(ch));
        }
        if IconButton::new(ic::RESIZE)
            .size(24.0)
            .tooltip(&self.tool_tooltip(&ctx, tr.resize_and_skew, Some(&Command::ResizeSkew)))
            .show(ui, t, Rect::from_center_size(at(195.0, top + 42.0), vec2(36.0, 40.0)), Id::unique("rb-resize"))
            .clicked()
        {
            self.execute(&ctx, Command::ResizeSkew);
        }
        // Undo / Redo, stacked.
        for (glyph, cmd, tip, enabled, y) in [
            (ic::UNDO, Command::Undo, tr.undo, self.history.can_undo() || !self.active.is_idle(), row1),
            (ic::REDO, Command::Redo, tr.redo, self.history.can_redo(), row2),
        ] {
            if IconButton::new(glyph)
                .enabled(enabled)
                .tooltip(&self.tool_tooltip(&ctx, tip, Some(&cmd)))
                .show(ui, t, btn(241.0, y), Id::unique(("rb-history", tip)))
                .clicked()
            {
                self.execute(&ctx, cmd);
            }
        }
        group_label(ui, 163.0, tr.group_image);
    }

    fn rb_tools(&mut self, ui: &mut egui::Ui, t: &Theme, x0: f32, top: f32) {
        let ctx = ui.ctx().clone();
        let row1 = top + 23.0;
        let row2 = top + 61.0;
        let label_y = top + 91.0;
        let btn = |x: f32, y: f32| widgets::square(pos2(x0 + x, y), 32.0);
        let group_label = |ui: &egui::Ui, cx: f32, text: &str| {
            widgets::label(ui.painter(), pos2(x0 + cx, label_y), Align2::CENTER_CENTER, text, 12.0, t.text_secondary);
        };
        let tr = self.tr;
        // --- Tools ---
        let accent = t.tool_icon_fill;
        let tools: [ToolButton; 6] = [
            (252.0, row1, ic::PENCIL, Some(icf::PENCIL), Tool::Pencil, tr.pencil),
            (293.0, row1, ic::FILL, Some(icf::FILL), Tool::Fill, tr.fill),
            (333.0, row1, ic::TEXT, None, Tool::Text, tr.text),
            (252.0, row2, ic::ERASER, Some(icf::ERASER), Tool::Eraser, tr.eraser),
            (293.0, row2, ic::PICKER, Some(icf::PICKER), Tool::Picker, tr.color_picker),
            (333.0, row2, ic::ZOOM_IN, None, Tool::Magnifier, tr.magnifier),
        ];
        for (x, y, glyph, filled, tool, tip) in tools {
            let cmd = Command::SetTool(tool);
            let tip = self.tool_tooltip(&ctx, tip, Some(&cmd));
            let mut b = IconButton::new(glyph).selected(self.tool == tool).tooltip(&tip);
            if let Some(f) = filled {
                let c = match tool {
                    Tool::Pencil => Color32::from_rgb(0xF2, 0xB0, 0x3A),
                    Tool::Eraser => Color32::from_rgb(0xE8, 0x6A, 0x8A),
                    _ => accent,
                };
                b = b.filled(f, c);
            }
            let rect = btn(x, y);
            if b.show(ui, t, rect, Id::unique(("rb-tool", x as i32, y as i32))).clicked() {
                self.execute(&ctx, cmd.clone());
            }
            if let Some(key) = crate::commands::tool_hotkey(&cmd) {
                widgets::paint_hotkey_hint(ui.painter(), rect, key, t.text_secondary);
            }
        }
        group_label(ui, 293.0, tr.group_tools);
    }

    fn rb_brushes(&mut self, ui: &mut egui::Ui, t: &Theme, x0: f32, top: f32) {
        let ctx = ui.ctx().clone();
        let label_y = top + 91.0;
        let at = |x: f32, y: f32| pos2(x0 + x, y);
        let group_label = |ui: &egui::Ui, cx: f32, text: &str| {
            widgets::label(ui.painter(), pos2(x0 + cx, label_y), Align2::CENTER_CENTER, text, 12.0, t.text_secondary);
        };
        let tr = self.tr;
        // --- Brushes ---
        let br = Rect::from_center_size(at(396.0, top + 42.0), vec2(40.0, 56.0));
        let resp = ui.interact(br, Id::unique("rb-brushes"), Sense::click());
        let selected = self.tool == Tool::Brush;
        let open = self.ui.open_menu == Some(MenuKind::Brushes);
        if selected {
            ui.painter().rect(
                br,
                widgets::RADIUS,
                if resp.hovered() || open { t.hover } else { Color32::TRANSPARENT },
                EStroke::new(1.0, t.selected_border),
                egui::StrokeKind::Inside,
            );
        } else {
            let fill = if open { t.hover } else { widgets::state_fill(t, &resp, true) };
            ui.painter().rect_filled(br, widgets::RADIUS, fill);
        }
        widgets::paint_icon_two_tone(
            ui.painter(),
            at(396.0, top + 32.0),
            ic::BRUSH,
            Some((icf::BRUSH, to_egui(self.colors.primary).gamma_multiply(1.6))),
            24.0,
            t.text,
        );
        widgets::paint_chevron(ui.painter(), at(396.0, top + 58.0), t.text, 12.0);
        let resp = resp.on_hover_text(self.brush_name(self.tools.brush));
        if resp.clicked() {
            if resp.interact_pointer_pos().is_some_and(|p| p.y > top + 48.0) || selected {
                self.toggle_menu(MenuKind::Brushes, br);
            } else {
                self.execute(&ctx, Command::SetTool(Tool::Brush));
            }
        }
        group_label(ui, 396.0, tr.group_brushes);
    }

    fn rb_shapes(&mut self, ui: &mut egui::Ui, t: &Theme, x0: f32, top: f32) {
        let ctx = ui.ctx().clone();
        let row1 = top + 23.0;
        let row2 = top + 61.0;
        let label_y = top + 91.0;
        let at = |x: f32, y: f32| pos2(x0 + x, y);
        let tr = self.tr;
        let shape_tool = self.tool == Tool::Shape;
        // --- Shapes ---
        // In vector mode the Pointer comes first: it only picks up and moves objects.
        let x0 = if self.doc.vector_mode() {
            let pointer = self.vector_pointer();
            // Same size as the Brushes button next to it.
            let pr = Rect::from_center_size(at(449.0, top + 42.0), vec2(40.0, 56.0));
            let tip = self.tool_tooltip(&ctx, tr.vector_pointer_tip, Some(&Command::Pointer));
            if IconButton::new(' ').selected(pointer).tooltip(&tip).show(ui, t, pr, Id::unique("rb-pointer")).clicked() {
                self.execute(&ctx, Command::SetVectorPointer);
            }
            // The arrow's own box in the 24 x 24 drawing is centered on the button.
            let icon = Rect::from_min_size(pr.center() - vec2(8.7, 11.9), vec2(24.0, 24.0));
            super::svg_icon::paint(&ctx, ui.painter(), icon, "pointer", super::svg_icon::POINTER);
            widgets::paint_hotkey_hint(ui.painter(), pr.shrink(2.0), "V", t.text_secondary);
            x0 + POINTER_WIDTH
        } else {
            x0
        };
        let at = |x: f32, y: f32| pos2(x0 + x, y);
        let btn = |x: f32, y: f32| widgets::square(pos2(x0 + x, y), 32.0);
        let group_label = |ui: &egui::Ui, cx: f32, text: &str| {
            widgets::label(ui.painter(), pos2(x0 + cx, label_y), Align2::CENTER_CENTER, text, 12.0, t.text_secondary);
        };
        self.shape_gallery(ui, t, Rect::from_min_size(at(436.0, top + 4.0), vec2(171.0, 80.0)));
        let outline = btn(622.0, row1);
        if IconButton::new(ic::OUTLINE).enabled(shape_tool).tooltip(tr.outline).show(ui, t, outline, Id::unique("rb-outline")).clicked() {
            self.toggle_menu(MenuKind::Outline, outline);
        }
        let ch = Rect::from_center_size(at(649.0, row1), vec2(16.0, 32.0));
        if widgets::chevron_button(ui, t, ch, Id::unique("rb-outline-ch"), shape_tool, self.ui.open_menu == Some(MenuKind::Outline))
            .clicked()
        {
            self.toggle_menu(MenuKind::Outline, outline.union(ch));
        }
        let fill = btn(622.0, row2);
        if IconButton::new(ic::SHAPE_FILL).enabled(shape_tool).tooltip(tr.shape_fill).show(ui, t, fill, Id::unique("rb-fill")).clicked() {
            self.toggle_menu(MenuKind::Fill, fill);
        }
        let ch = Rect::from_center_size(at(649.0, row2), vec2(16.0, 32.0));
        if widgets::chevron_button(ui, t, ch, Id::unique("rb-fill-ch"), shape_tool, self.ui.open_menu == Some(MenuKind::Fill)).clicked() {
            self.toggle_menu(MenuKind::Fill, fill.union(ch));
        }
        // The outline width comes from the size slider beside the canvas.
        group_label(ui, 547.0, tr.group_shapes);
    }

    fn rb_colors(&mut self, ui: &mut egui::Ui, t: &Theme, x0: f32, top: f32) {
        let label_y = top + 91.0;
        let group_label = |ui: &egui::Ui, cx: f32, text: &str| {
            widgets::label(ui.painter(), pos2(x0 + cx, label_y), Align2::CENTER_CENTER, text, 12.0, t.text_secondary);
        };
        let tr = self.tr;
        // --- Colors ---
        self.color_group(ui, t, x0 + 668.0, top);
        group_label(ui, 830.0, tr.group_colors);
    }

    fn rb_layers(&mut self, ui: &mut egui::Ui, t: &Theme, x0: f32, top: f32) {
        let ctx = ui.ctx().clone();
        let label_y = top + 91.0;
        let at = |x: f32, y: f32| pos2(x0 + x, y);
        let group_label = |ui: &egui::Ui, cx: f32, text: &str| {
            widgets::label(ui.painter(), pos2(x0 + cx, label_y), Align2::CENTER_CENTER, text, 12.0, t.text_secondary);
        };
        let tr = self.tr;
        // --- Layers ---
        let lr = Rect::from_center_size(at(1060.0, top + 42.0), vec2(40.0, 40.0));
        if IconButton::new(ic::LAYERS)
            .size(22.0)
            .selected(self.settings.show_layers)
            .tooltip(tr.layers)
            .show(ui, t, lr, Id::unique("rb-layers"))
            .clicked()
        {
            self.execute(&ctx, Command::ToggleLayersPanel);
        }
        group_label(ui, 1060.0, tr.group_layers);
    }

    fn rb_vector(&mut self, ui: &mut egui::Ui, t: &Theme, x0: f32, top: f32) {
        let ctx = ui.ctx().clone();
        let label_y = top + 91.0;
        let at = |x: f32, y: f32| pos2(x0 + x, y);
        let group_label = |ui: &egui::Ui, cx: f32, text: &str| {
            widgets::label(ui.painter(), pos2(x0 + cx, label_y), Align2::CENTER_CENTER, text, 12.0, t.text_secondary);
        };
        let tr = self.tr;
        // --- Vector ---
        let vr = Rect::from_center_size(at(1138.0, top + 42.0), vec2(40.0, 40.0));
        if IconButton::new(' ')
            .selected(self.doc.vector_mode())
            .tooltip(tr.vector_mode_tip)
            .show(ui, t, vr, Id::unique("rb-vector"))
            .clicked()
        {
            self.execute(&ctx, Command::ToggleVectorMode);
        }
        paint_vector_icon(ui.painter(), vr.center(), t.text);
        group_label(ui, 1138.0, tr.group_vector);
    }

    fn color_group(&mut self, ui: &mut egui::Ui, t: &Theme, x0: f32, top: f32) {
        let ctx = ui.ctx().clone();
        let tr = self.tr;
        // Color 1 / Color 2
        for (secondary, y, radius) in [(false, top + 23.0, 14.0), (true, top + 61.0, 12.0)] {
            let c = pos2(x0 + 26.0, y);
            let hit = Rect::from_center_size(c, vec2(36.0, 36.0));
            let resp = ui.interact(hit, Id::unique(("color-slot", secondary)), Sense::click());
            let color = if secondary { self.colors.secondary } else { self.colors.primary };
            let active = self.colors.editing_secondary == secondary;
            if active {
                ui.painter().circle_stroke(c, radius + 3.5, EStroke::new(1.5, t.accent));
            } else if resp.hovered() {
                ui.painter().circle_filled(c, radius + 4.0, t.hover);
            }
            paint_swatch(ui.painter(), t, c, radius, color);
            let resp = resp.on_hover_text(if secondary { tr.color2 } else { tr.color1 });
            if resp.clicked() && self.colors.editing_secondary != secondary {
                self.colors.editing_secondary = secondary;
                self.colors.selected = None;
            }
            if resp.double_clicked() {
                self.execute(&ctx, Command::EditColors);
            }
        }
        // Palette
        for (i, hex) in PALETTE.iter().enumerate() {
            let (col, row) = (i % 10, i / 10);
            let c = pos2(x0 + 62.0 + col as f32 * 24.0, top + 18.0 + row as f32 * 23.5);
            let color = Color::from_hex(*hex);
            self.palette_swatch(ui, t, c, Some(color), Swatch::Palette(i));
        }
        for i in 0..10 {
            let c = pos2(x0 + 62.0 + i as f32 * 24.0, top + 65.0);
            let color = self.colors.custom[i];
            self.palette_swatch(ui, t, c, color, Swatch::Custom(i));
        }
        // Edit colors (color wheel)
        let c = pos2(x0 + 317.0, top + 42.0);
        let hit = Rect::from_center_size(c, vec2(36.0, 36.0));
        let resp = ui.interact(hit, Id::unique("edit-colors"), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(hit, widgets::RADIUS, t.hover);
        }
        paint_color_wheel(ui.painter(), c, 12.0);
        ui.painter().circle(c + vec2(9.0, -9.0), 5.0, t.ribbon_bg, EStroke::new(1.0, t.text_secondary));
        widgets::paint_icon(ui.painter(), c + vec2(9.0, -9.0), ic::ADD, 8.0, t.text);
        if resp.on_hover_text(tr.edit_colors).clicked() {
            self.execute(&ctx, Command::EditColors);
        }
    }

    fn palette_swatch(&mut self, ui: &mut egui::Ui, t: &Theme, c: Pos2, color: Option<Color>, swatch: Swatch) {
        let hit = Rect::from_center_size(c, vec2(22.0, 22.0));
        let resp = ui.interact(hit, Id::unique(("swatch", swatch)), Sense::click());
        if self.colors.selected == Some(swatch) {
            ui.painter().circle_stroke(c, 10.5, EStroke::new(1.5, t.accent));
        } else if resp.hovered() {
            ui.painter().circle_stroke(c, 10.5, EStroke::new(1.5, t.text_secondary));
        }
        match color {
            Some(col) => {
                paint_swatch(ui.painter(), t, c, 8.0, col);
                let resp = resp.on_hover_text(col.to_hex_string());
                if resp.clicked() {
                    self.colors.selected = Some(swatch);
                    self.set_active_color(col);
                }
                if resp.secondary_clicked() {
                    self.colors.secondary = col;
                    self.on_colors_changed();
                }
            }
            None => {
                ui.painter().circle_stroke(c, 7.5, EStroke::new(1.0, t.swatch_ring));
                // An empty custom slot can be selected and then defined with the color tools.
                if resp.on_hover_text(self.tr.edit_colors).clicked() {
                    self.colors.selected = Some(swatch);
                }
            }
        }
    }

    /// Sets the color being edited (Color 1 or 2). A selected custom slot takes the color too.
    pub fn set_active_color(&mut self, c: Color) {
        if self.colors.editing_secondary {
            self.colors.secondary = c;
        } else {
            self.colors.primary = c;
        }
        if let Some(Swatch::Custom(i)) = self.colors.selected {
            self.colors.custom[i] = Some(c);
        }
        self.on_colors_changed();
    }

    /// Shape gallery with its own scroll (3 of 4 rows visible, as in Paint).
    fn shape_gallery(&mut self, ui: &mut egui::Ui, t: &Theme, rect: Rect) {
        let ctx = ui.ctx().clone();
        ui.painter().rect(rect, widgets::RADIUS, Color32::TRANSPARENT, EStroke::new(1.0, t.separator), egui::StrokeKind::Inside);
        let scroll_id = Id::unique("shape-gallery-scroll");
        let mut first_row: usize = ctx.data(|d| d.get_temp(scroll_id)).unwrap_or(0);
        let hovered = ui.rect_contains_pointer(rect);
        if hovered {
            let dy = ctx.input(|i| i.smooth_scroll_delta.y);
            if dy < -0.5 {
                first_row = 1;
            } else if dy > 0.5 {
                first_row = 0;
            }
        }
        ctx.data_mut(|d| d.insert_temp(scroll_id, first_row));
        let cell = 22.0;
        let origin = rect.min + vec2(8.0, 7.0);
        let clip = rect.shrink(2.0);
        let painter = ui.painter().with_clip_rect(clip);
        for (i, kind) in ShapeKind::ALL.iter().enumerate() {
            let (col, row) = (i % 7, i / 7);
            if row < first_row || row >= first_row + 3 {
                continue;
            }
            let c = origin + vec2(col as f32 * cell + cell / 2.0, (row - first_row) as f32 * cell + cell / 2.0);
            let r = Rect::from_center_size(c, vec2(cell, cell));
            let resp = ui.interact(r, Id::unique(("shape", i)), Sense::click());
            let selected = self.tool == Tool::Shape && self.tools.shape == *kind && !self.vector_pointer();
            if selected {
                painter.rect(r.shrink(0.5), widgets::RADIUS, t.hover, EStroke::new(1.0, t.selected_border), egui::StrokeKind::Inside);
            } else if resp.hovered() {
                painter.rect_filled(r, widgets::RADIUS, t.hover);
            }
            paint_shape_icon(&painter, *kind, r.shrink(5.0), t.text);
            let cmd = Command::SetShape(*kind);
            if let Some(key) = crate::commands::tool_hotkey(&cmd) {
                painter.text(
                    r.right_bottom() + vec2(0.5, 1.0),
                    Align2::RIGHT_BOTTOM,
                    key,
                    egui::FontId::proportional(7.0),
                    t.text_secondary,
                );
            }
            if resp.on_hover_text(self.tool_tooltip(&ctx, self.shape_name(*kind), Some(&cmd))).clicked() {
                self.execute(&ctx, Command::SetShape(*kind));
            }
        }
        // Scrollbar
        let track = Rect::from_min_max(pos2(rect.right() - 6.0, rect.top() + 6.0), pos2(rect.right() - 3.0, rect.bottom() - 6.0));
        let h = track.height() * 0.75;
        let y = track.top() + if first_row == 1 { track.height() - h } else { 0.0 };
        ui.painter().rect_filled(Rect::from_min_size(pos2(track.left(), y), vec2(2.0, h)), 1, t.text_secondary.gamma_multiply(0.7));
        let sb = ui.interact(track.expand2(vec2(3.0, 0.0)), Id::unique("shape-sb"), Sense::click_and_drag());
        if (sb.clicked() || sb.dragged())
            && let Some(p) = sb.interact_pointer_pos()
        {
            first_row = usize::from(p.y > track.center().y);
            ctx.data_mut(|d| d.insert_temp(scroll_id, first_row));
        }
    }

    pub fn brush_menu_entries(&self, ctx: &egui::Context) -> Vec<Entry<Command>> {
        BrushKind::BRUSHES
            .iter()
            .map(|&k| {
                Entry::item(None, self.brush_name(k), Command::SetBrush(k))
                    .marked(self.tools.brush == k)
                    .preview(self.brush_preview(ctx, k))
            })
            .collect()
    }

    pub fn brush_name(&self, k: BrushKind) -> &'static str {
        let tr = self.tr;
        match k {
            BrushKind::Brush => tr.brush,
            BrushKind::CalligraphyBrush => tr.calligraphy_brush,
            BrushKind::CalligraphyPen => tr.calligraphy_pen,
            BrushKind::Airbrush => tr.airbrush,
            BrushKind::Oil => tr.oil_brush,
            BrushKind::Crayon => tr.crayon,
            BrushKind::Marker => tr.marker,
            BrushKind::NaturalPencil => tr.natural_pencil,
            BrushKind::Watercolor => tr.watercolor,
            BrushKind::Pencil => tr.pencil,
            BrushKind::Eraser => tr.eraser,
        }
    }

    pub fn texture_name(&self, tex: Texture) -> &'static str {
        let tr = self.tr;
        match tex {
            Texture::Crayon => tr.crayon,
            Texture::Marker => tr.marker,
            Texture::Oil => tr.oil_brush,
            Texture::NaturalPencil => tr.natural_pencil,
            Texture::Watercolor => tr.watercolor,
            Texture::Solid => tr.solid_fill,
        }
    }

    pub fn shape_name(&self, k: ShapeKind) -> &'static str {
        let tr = self.tr;
        match k {
            ShapeKind::Line => tr.shape_line,
            ShapeKind::Curve => tr.shape_curve,
            ShapeKind::Oval => tr.shape_oval,
            ShapeKind::Rectangle => tr.shape_rectangle,
            ShapeKind::RoundedRectangle => tr.shape_rounded_rectangle,
            ShapeKind::Polygon => tr.shape_polygon,
            ShapeKind::Triangle => tr.shape_triangle,
            ShapeKind::RightTriangle => tr.shape_right_triangle,
            ShapeKind::Diamond => tr.shape_diamond,
            ShapeKind::Pentagon => tr.shape_pentagon,
            ShapeKind::Hexagon => tr.shape_hexagon,
            ShapeKind::RightArrow => tr.shape_right_arrow,
            ShapeKind::LeftArrow => tr.shape_left_arrow,
            ShapeKind::UpArrow => tr.shape_up_arrow,
            ShapeKind::DownArrow => tr.shape_down_arrow,
            ShapeKind::Star4 => tr.shape_star4,
            ShapeKind::Star5 => tr.shape_star5,
            ShapeKind::Star6 => tr.shape_star6,
            ShapeKind::RoundedRectCallout => tr.shape_rounded_callout,
            ShapeKind::OvalCallout => tr.shape_oval_callout,
            ShapeKind::CloudCallout => tr.shape_cloud_callout,
            ShapeKind::Heart => tr.shape_heart,
            ShapeKind::Lightning => tr.shape_lightning,
            ShapeKind::CurvedArrow => tr.shape_curved_arrow,
            ShapeKind::Moon => tr.shape_moon,
        }
    }

    fn brush_preview(&self, ctx: &egui::Context, kind: BrushKind) -> Preview {
        let color = preview_color(&self.theme);
        cached_preview(ctx, ("brush", format!("{kind:?}"), color), |k| {
            // 92 x 26 points, drawn at the display's scale.
            let (w, h) = ((92.0 * k).round() as u32, (26.0 * k).round() as u32);
            let size = k * match kind {
                BrushKind::CalligraphyPen | BrushKind::CalligraphyBrush => 9.0,
                BrushKind::NaturalPencil => 5.0,
                BrushKind::Airbrush => 7.0,
                _ => 8.0,
            };
            let mut s = Stroke::new(kind, size, w, h, 7);
            for i in 0..=60 {
                let x = 6.0 + i as f32 * 80.0 / 60.0;
                let y = 13.0 - (x / 92.0 * std::f32::consts::TAU).sin() * 6.0;
                s.add_point(Point::new(x * k, y * k));
                if kind == BrushKind::Airbrush {
                    s.hold();
                }
            }
            let mask = s.finish();
            let mut p = raster::pixmap(w, h);
            apply_mask(&mut p, PixelRect::from_size(w, h), &mask, color, 1.0, BlendMode::Over);
            p
        })
    }

    pub fn texture_preview(&self, ctx: &egui::Context, tex: Texture) -> Preview {
        let color = preview_color(&self.theme);
        cached_preview(ctx, ("tex", format!("{tex:?}"), color), |k| {
            // 72 x 22 points, drawn at the display's scale.
            let (w, h) = ((72.0 * k).round() as u32, (22.0 * k).round() as u32);
            let paint = shapes::ShapePaint {
                outline: shapes::ShapeStyle::Textured(tex),
                fill: shapes::ShapeStyle::None,
                width: 8.0 * k,
                outline_color: color,
                fill_color: color,
                opacity: 1.0,
            };
            let mut pb = bgpaint_core::tiny_skia::PathBuilder::new();
            pb.move_to(4.0 * k, 15.0 * k);
            pb.cubic_to(24.0 * k, 2.0 * k, 44.0 * k, 22.0 * k, 68.0 * k, 8.0 * k);
            let mut p = raster::pixmap(w, h);
            if let Some(path) = pb.finish()
                && let Some(s) = shapes::rasterize(&path, &paint, false, w, h)
            {
                raster::draw_over(&mut p, &s.pixmap, s.x, s.y, 1.0);
            }
            p
        })
    }
}

fn preview_color(t: &Theme) -> Color {
    if t.dark { Color::rgb(0xE6, 0xE6, 0xE6) } else { Color::rgb(0x40, 0x40, 0x40) }
}

/// A menu preview, rendered by `make(scale)` at the display's scale (`scale` physical pixels
/// per point) so it stays sharp at any DPI, and cached per scale.
fn cached_preview(
    ctx: &egui::Context,
    key: (&'static str, String, Color),
    make: impl FnOnce(f32) -> bgpaint_core::tiny_skia::Pixmap,
) -> Preview {
    let ppp = ctx.pixels_per_point();
    let id = Id::unique(("preview", key.0, key.1, key.2.r, key.2.g, key.2.b, ppp.to_bits()));
    let tex: Option<egui::TextureHandle> = ctx.data(|d| d.get_temp(id));
    let tex = tex.unwrap_or_else(|| {
        let p = make(ppp);
        let img = egui::ColorImage::from_rgba_premultiplied([p.width() as usize, p.height() as usize], p.data());
        let tex = ctx.load_texture(format!("preview-{id:?}"), img, egui::TextureOptions::LINEAR);
        ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
        tex
    });
    // Shown at its size in points.
    let size = tex.size_vec2() / ppp;
    Preview::Texture(tex.id(), size)
}

/// A round color swatch; transparent colors show a checkerboard.
pub fn paint_swatch(p: &egui::Painter, t: &Theme, c: Pos2, r: f32, color: Color) {
    if color.a < 255 {
        p.circle_filled(c, r, Color32::WHITE);
        let s = r / 2.0;
        for (dx, dy) in [(-1.0, -1.0), (0.0, 0.0)] {
            let q = Rect::from_min_size(c + vec2(dx * s, dy * s), vec2(s, s));
            p.rect_filled(q, 0, Color32::from_gray(200));
        }
    }
    p.circle_filled(c, r, to_egui(color));
    let ring = if (color.r as u32 + color.g as u32 + color.b as u32) > 700 || (t.dark && color.r < 50 && color.g < 50 && color.b < 50) {
        EStroke::new(1.0, t.swatch_ring)
    } else {
        EStroke::new(1.0, Color32::from_black_alpha(30))
    };
    p.circle_stroke(c, r, ring);
}

/// The rainbow "Edit colors" button.
pub fn paint_color_wheel(p: &egui::Painter, c: Pos2, r: f32) {
    let n = 48;
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(c, Color32::WHITE);
    for i in 0..=n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        let col = Color::from_hsv(a.to_degrees(), 1.0, 1.0);
        mesh.colored_vertex(c + vec2(a.cos(), a.sin()) * r, to_egui(col));
        if i > 0 {
            mesh.add_triangle(0, i, i + 1);
        }
    }
    p.add(egui::Shape::mesh(mesh));
}

/// Draws a shape outline as an icon inside `r`.
pub fn paint_shape_icon(p: &egui::Painter, kind: ShapeKind, r: Rect, color: Color32) {
    let stroke = EStroke::new(1.1, color);
    let pt = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.top() + y * r.height());
    match kind {
        ShapeKind::Line => {
            p.line_segment([pt(0.0, 0.0), pt(1.0, 1.0)], stroke);
        }
        ShapeKind::Curve => {
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let u = i as f32 / 16.0;
                    let x = u;
                    let y = 0.85 - (u * std::f32::consts::PI * 1.5).sin() * 0.6 - u * 0.1;
                    pt(x, y.clamp(0.0, 1.0))
                })
                .collect();
            p.add(egui::Shape::line(pts, stroke));
        }
        ShapeKind::CurvedArrow => {
            // An arch ending in a small arrowhead that points down.
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let u = i as f32 / 16.0;
                    pt(0.05 + 0.75 * u, 0.95 - (u * std::f32::consts::PI).sin() * 0.9)
                })
                .collect();
            let (tip, before) = (pts[16], pts[15]);
            let dir = (tip - before).normalized();
            let side = egui::vec2(-dir.y, dir.x);
            let back = tip - dir * r.width() * 0.32;
            p.add(egui::Shape::line(pts, stroke));
            p.add(egui::Shape::line(vec![back + side * r.width() * 0.2, tip, back - side * r.width() * 0.2], stroke));
        }
        ShapeKind::Polygon => {
            let pts = vec![pt(0.15, 1.0), pt(0.35, 0.0), pt(0.95, 0.0), pt(0.75, 0.55), pt(1.0, 1.0)];
            closed_outline(p, pts, stroke);
        }
        k => {
            let rect = RectF::from_xywh(r.left(), r.top(), r.width(), r.height());
            if let Some(path) = shapes::box_shape_path(k, &rect) {
                for sub in flatten_path(&path) {
                    if k.is_open() {
                        p.add(egui::Shape::line(sub, stroke));
                    } else {
                        closed_outline(p, sub, stroke);
                    }
                }
            }
        }
    }
}

/// The Vector button's icon, drawn because the icon font has none: a curve between two nodes,
/// one showing its tangent handle (the usual sign for vector drawing).
fn paint_vector_icon(p: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = EStroke::new(1.3, color);
    let (a, b) = (c + vec2(-7.0, 6.0), c + vec2(7.0, -6.0));
    let (c1, c2) = (c + vec2(-6.0, -8.0), c + vec2(6.0, 8.0));
    p.add(egui::epaint::CubicBezierShape::from_points_stroke([a, c1, c2, b], false, Color32::TRANSPARENT, stroke));
    p.line_segment([a, c1], EStroke::new(1.0, color.gamma_multiply(0.7)));
    p.circle_filled(c1, 1.8, color);
    for n in [a, b] {
        p.rect_filled(Rect::from_center_size(n, vec2(4.5, 4.5)), 0, color);
    }
}

/// Turn (between a segment and the next) past which a corner counts as sharp: about 127°,
/// i.e. an inner angle under ~53°.
const SHARP_TURN_COS: f32 = -0.6;

/// Strokes a closed polyline. egui joins segments with unlimited miters, which turn into long
/// spikes at very sharp corners (the heart's tip and notch, the lightning bolt's points), so the
/// outline is cut there into open pieces that meet at the corner.
fn closed_outline(p: &egui::Painter, pts: Vec<Pos2>, stroke: EStroke) {
    match split_at_sharp_corners(&pts) {
        None => {
            p.add(egui::Shape::closed_line(pts, stroke));
        }
        Some(pieces) => {
            for piece in pieces {
                p.add(egui::Shape::line(piece, stroke));
            }
        }
    }
}

/// The open pieces of the closed polyline `pts` between its sharp corners (each piece starts
/// and ends on one), or `None` when it has no sharp corner.
fn split_at_sharp_corners(pts: &[Pos2]) -> Option<Vec<Vec<Pos2>>> {
    let mut pts = pts.to_vec();
    pts.dedup();
    while pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    let n = pts.len();
    if n < 3 {
        return None;
    }
    let sharp = |i: usize| {
        let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
        (b - a).normalized().dot((c - b).normalized()) < SHARP_TURN_COS
    };
    let cuts: Vec<usize> = (0..n).filter(|&i| sharp(i)).collect();
    if cuts.is_empty() {
        return None;
    }
    let pieces = cuts
        .iter()
        .enumerate()
        .map(|(k, &start)| {
            let end = cuts[(k + 1) % cuts.len()];
            let len = (end + n - start - 1) % n + 1;
            (0..=len).map(|j| pts[(start + j) % n]).collect()
        })
        .collect();
    Some(pieces)
}

/// Flattens a tiny-skia path into polylines (one per subpath).
pub fn flatten_path(path: &bgpaint_core::tiny_skia::Path) -> Vec<Vec<Pos2>> {
    use bgpaint_core::tiny_skia::PathSegment;
    let mut out: Vec<Vec<Pos2>> = Vec::new();
    let mut cur: Vec<Pos2> = Vec::new();
    let mut last = Pos2::ZERO;
    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                if cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
                last = pos2(p.x, p.y);
                cur.push(last);
            }
            PathSegment::LineTo(p) => {
                last = pos2(p.x, p.y);
                cur.push(last);
            }
            PathSegment::QuadTo(c, p) => {
                for i in 1..=8 {
                    let t = i as f32 / 8.0;
                    let a = last.lerp(pos2(c.x, c.y), t);
                    let b = pos2(c.x, c.y).lerp(pos2(p.x, p.y), t);
                    cur.push(a.lerp(b, t));
                }
                last = pos2(p.x, p.y);
            }
            PathSegment::CubicTo(c1, c2, p) => {
                for i in 1..=12 {
                    let t = i as f32 / 12.0;
                    let u = 1.0 - t;
                    let x = u * u * u * last.x + 3.0 * u * u * t * c1.x + 3.0 * u * t * t * c2.x + t * t * t * p.x;
                    let y = u * u * u * last.y + 3.0 * u * u * t * c1.y + 3.0 * u * t * t * c2.y + t * t * t * p.y;
                    cur.push(pos2(x, y));
                }
                last = pos2(p.x, p.y);
            }
            PathSegment::Close => {}
        }
    }
    if cur.len() > 1 {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn icon(kind: ShapeKind) -> Vec<Pos2> {
        let path = shapes::box_shape_path(kind, &RectF::from_xywh(0.0, 0.0, 20.0, 20.0)).unwrap();
        flatten_path(&path).remove(0)
    }

    #[test]
    fn smooth_and_square_outlines_stay_closed() {
        assert!(split_at_sharp_corners(&icon(ShapeKind::Rectangle)).is_none());
        assert!(split_at_sharp_corners(&icon(ShapeKind::Oval)).is_none());
    }

    #[test]
    fn heart_is_cut_at_its_tip_and_notch() {
        let pts = icon(ShapeKind::Heart);
        let pieces = split_at_sharp_corners(&pts).unwrap();
        assert_eq!(pieces.len(), 2, "{pieces:?}");
        // The pieces cover the whole outline: every segment once, sharing the cut points.
        let segments: usize = pieces.iter().map(|p| p.len() - 1).sum();
        assert_eq!(segments, pts.len());
        let (top, bottom) = (pieces[0][0].y.min(pieces[1][0].y), pieces[0][0].y.max(pieces[1][0].y));
        assert!(top < 10.0 && bottom > 19.0, "cuts at y = {top}, {bottom}");
    }

    #[test]
    fn a_single_sharp_corner_gives_one_piece_around() {
        // A thin triangle: only the corner at the origin is sharp.
        let pts = [pos2(0.0, 0.0), pos2(10.0, 1.0), pos2(10.0, -1.0)];
        let pieces = split_at_sharp_corners(&pts).unwrap();
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0], vec![pts[0], pts[1], pts[2], pts[0]]);
        let tri = [pos2(0.0, 0.0), pos2(100.0, 3.0), pos2(100.0, 6.0), pos2(50.0, 50.0)];
        let pieces = split_at_sharp_corners(&tri).unwrap();
        assert_eq!(pieces.len(), 1, "{pieces:?}");
        assert_eq!(pieces[0].first(), pieces[0].last());
        assert_eq!(pieces[0].len(), tri.len() + 1);
    }
}
