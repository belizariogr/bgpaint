//! The toolbar ("ribbon") with Paint's groups: Selection, Image, Tools, Brushes, Shapes, Colors
//! and Layers. Positions are measured from Paint at 100% scale.

use super::icons::{filled as icf, regular as ic};
use super::menu::{Entry, Preview};
use super::menubar::MenuKind;
use super::widgets::{self, IconButton};
use crate::app::PaintApp;
use crate::commands::Command;
use crate::theme::Theme;
use crate::tools::Tool;
use bgpaint_core::brush::{BlendMode, BrushKind, Stroke, Texture, apply_mask};
use bgpaint_core::color::Color;
use bgpaint_core::geom::{PixelRect, Point, RectF};
use bgpaint_core::raster;
use bgpaint_core::shapes::{self, ShapeKind};
use egui::{Align2, Color32, Id, Pos2, Rect, Sense, Stroke as EStroke, pos2, vec2};

pub const RIBBON_HEIGHT: f32 = 106.0;

/// The 20 colors of Paint's palette (two rows).
pub const PALETTE: [u32; 20] = [
    0x000000, 0x7F7F7F, 0x880015, 0xED1C24, 0xFF7F27, 0xFFF200, 0x22B14C, 0x00A2E8, 0x3F48CC, 0xA349A4, 0xFFFFFF, 0xC3C3C3, 0xB97A57,
    0xFFAEC9, 0xFFC90E, 0xEFE4B0, 0xB5E61D, 0x99D9EA, 0x7092BE, 0xC8BFE7,
];

pub fn to_egui(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

impl PaintApp {
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
                ui.painter().hline(r.x_range(), r.top() + 0.5, EStroke::new(1.0, t.ribbon_border));
                self.ribbon_contents(ui, &t, r);
            },
        );
    }

    fn ribbon_contents(&mut self, ui: &mut egui::Ui, t: &Theme, r: Rect) {
        let ctx = ui.ctx().clone();
        let top = r.top();
        let row1 = top + 23.0;
        let row2 = top + 61.0;
        let label_y = top + 91.0;
        let x0 = r.left();
        let at = |x: f32, y: f32| pos2(x0 + x, y);
        let btn = |x: f32, y: f32| widgets::square(pos2(x0 + x, y), 32.0);
        let group_label = |ui: &egui::Ui, cx: f32, text: &str| {
            widgets::label(ui.painter(), pos2(x0 + cx, label_y), Align2::CENTER_CENTER, text, 12.0, t.text_secondary);
        };
        let sep = |ui: &egui::Ui, x: f32| widgets::vseparator(ui.painter(), t, x0 + x, top + 10.0, top + 98.0);
        let tr = self.tr;
        let shape_tool = self.tool == Tool::Shape;

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
        let resp = resp.on_hover_text(tr.group_selection);
        if resp.clicked() {
            if resp.interact_pointer_pos().is_some_and(|p| p.y > top + 48.0) || selected {
                self.toggle_menu(MenuKind::Selection, sel_rect);
            } else {
                self.execute(&ctx, Command::SetTool(Tool::Select));
            }
        }
        group_label(ui, 32.0, tr.group_selection);
        sep(ui, 66.0);

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
        let rot = btn(128.0, row1);
        if IconButton::new(ic::ROTATE_RIGHT).tooltip(tr.rotate).show(ui, t, rot, Id::unique("rb-rot")).clicked() {
            self.toggle_menu(MenuKind::Rotate, rot.union(btn(150.0, row1)));
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
        if IconButton::new(ic::FLIP_H).tooltip(tr.flip).show(ui, t, flip, Id::unique("rb-flip")).clicked() {
            self.toggle_menu(MenuKind::Flip, flip);
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
        group_label(ui, 145.0, tr.group_image);
        sep(ui, 224.0);

        // --- Tools ---
        let accent = t.tool_icon_fill;
        let tools: [(f32, f32, char, Option<char>, Tool, &str); 6] = [
            (252.0, row1, ic::PENCIL, Some(icf::PENCIL), Tool::Pencil, tr.pencil),
            (293.0, row1, ic::FILL, Some(icf::FILL), Tool::Fill, tr.fill),
            (333.0, row1, ic::TEXT, None, Tool::Text, tr.text),
            (252.0, row2, ic::ERASER, Some(icf::ERASER), Tool::Eraser, tr.eraser),
            (293.0, row2, ic::PICKER, Some(icf::PICKER), Tool::Picker, tr.color_picker),
            (333.0, row2, ic::ZOOM_IN, None, Tool::Magnifier, tr.magnifier),
        ];
        for (x, y, glyph, filled, tool, tip) in tools {
            let mut b = IconButton::new(glyph).selected(self.tool == tool).tooltip(tip);
            if let Some(f) = filled {
                let c = match tool {
                    Tool::Pencil => Color32::from_rgb(0xF2, 0xB0, 0x3A),
                    Tool::Eraser => Color32::from_rgb(0xE8, 0x6A, 0x8A),
                    _ => accent,
                };
                b = b.filled(f, c);
            }
            if b.show(ui, t, btn(x, y), Id::unique(("rb-tool", tip))).clicked() {
                self.execute(&ctx, Command::SetTool(tool));
            }
        }
        group_label(ui, 293.0, tr.group_tools);
        sep(ui, 365.0);

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
        sep(ui, 427.0);

        // --- Shapes ---
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
        let size = Rect::from_center_size(at(686.0, top + 42.0), vec2(32.0, 56.0));
        let resp = ui.interact(size, Id::unique("rb-size"), if shape_tool { Sense::click() } else { Sense::hover() });
        let open = self.ui.open_menu == Some(MenuKind::Size);
        let fill = if open { t.hover } else { widgets::state_fill(t, &resp, shape_tool) };
        ui.painter().rect_filled(size, widgets::RADIUS, fill);
        let c = if shape_tool { t.text } else { t.text_disabled };
        widgets::paint_icon(ui.painter(), at(686.0, top + 34.0), ic::SIZE, 22.0, c);
        widgets::paint_chevron(ui.painter(), at(686.0, top + 58.0), c, 12.0);
        if resp.on_hover_text(tr.size).clicked() {
            self.toggle_menu(MenuKind::Size, size);
        }
        group_label(ui, 562.0, tr.group_shapes);
        sep(ui, 718.0);

        // --- Colors ---
        self.color_group(ui, t, x0 + 718.0, top);
        group_label(ui, 880.0, tr.group_colors);
        sep(ui, 1071.0);

        // --- Layers ---
        let lr = Rect::from_center_size(at(1110.0, top + 42.0), vec2(40.0, 40.0));
        if IconButton::new(ic::LAYERS)
            .size(22.0)
            .selected(self.ui.show_layers)
            .tooltip(tr.layers)
            .show(ui, t, lr, Id::unique("rb-layers"))
            .clicked()
        {
            self.execute(&ctx, Command::ToggleLayersPanel);
        }
        group_label(ui, 1110.0, tr.group_layers);

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
            if resp.clicked() {
                self.colors.editing_secondary = secondary;
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
            self.palette_swatch(ui, t, c, Some(color), Id::unique(("pal", i)));
        }
        for i in 0..10 {
            let c = pos2(x0 + 62.0 + i as f32 * 24.0, top + 65.0);
            let color = self.colors.custom[i];
            self.palette_swatch(ui, t, c, color, Id::unique(("custom", i)));
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

    fn palette_swatch(&mut self, ui: &mut egui::Ui, t: &Theme, c: Pos2, color: Option<Color>, id: Id) {
        let hit = Rect::from_center_size(c, vec2(22.0, 22.0));
        let resp = ui.interact(hit, id, if color.is_some() { Sense::click() } else { Sense::hover() });
        match color {
            Some(col) => {
                if resp.hovered() {
                    ui.painter().circle_stroke(c, 10.5, EStroke::new(1.5, t.text_secondary));
                }
                paint_swatch(ui.painter(), t, c, 8.0, col);
                let resp = resp.on_hover_text(col.to_hex_string());
                if resp.clicked() {
                    self.set_active_color(col);
                }
                if resp.secondary_clicked() {
                    self.colors.secondary = col;
                }
            }
            None => {
                ui.painter().circle_stroke(c, 7.5, EStroke::new(1.0, t.swatch_ring));
            }
        }
    }

    pub fn set_active_color(&mut self, c: Color) {
        if self.colors.editing_secondary {
            self.colors.secondary = c;
        } else {
            self.colors.primary = c;
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
            let selected = self.tool == Tool::Shape && self.tools.shape == *kind;
            if selected {
                painter.rect(r.shrink(0.5), widgets::RADIUS, t.hover, EStroke::new(1.0, t.selected_border), egui::StrokeKind::Inside);
            } else if resp.hovered() {
                painter.rect_filled(r, widgets::RADIUS, t.hover);
            }
            paint_shape_icon(&painter, *kind, r.shrink(5.0), t.text);
            if resp.on_hover_text(self.shape_name(*kind)).clicked() {
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
        }
    }

    fn brush_preview(&self, ctx: &egui::Context, kind: BrushKind) -> Preview {
        let color = preview_color(&self.theme);
        cached_preview(ctx, ("brush", format!("{kind:?}"), color), || {
            let (w, h) = (92u32, 26u32);
            let size = match kind {
                BrushKind::CalligraphyPen | BrushKind::CalligraphyBrush => 9.0,
                BrushKind::NaturalPencil => 5.0,
                BrushKind::Airbrush => 7.0,
                _ => 8.0,
            };
            let mut s = Stroke::new(kind, size, w, h, 7);
            for i in 0..=60 {
                let x = 6.0 + i as f32 * (w as f32 - 12.0) / 60.0;
                let y = h as f32 / 2.0 - (x / w as f32 * std::f32::consts::TAU).sin() * 6.0;
                s.add_point(Point::new(x, y));
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
        cached_preview(ctx, ("tex", format!("{tex:?}"), color), || {
            let (w, h) = (72u32, 22u32);
            let paint = shapes::ShapePaint {
                outline: shapes::ShapeStyle::Textured(tex),
                fill: shapes::ShapeStyle::None,
                width: 8.0,
                outline_color: color,
                fill_color: color,
            };
            let mut pb = bgpaint_core::tiny_skia::PathBuilder::new();
            pb.move_to(4.0, 15.0);
            pb.cubic_to(24.0, 2.0, 44.0, 22.0, 68.0, 8.0);
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

fn cached_preview(
    ctx: &egui::Context,
    key: (&'static str, String, Color),
    make: impl FnOnce() -> bgpaint_core::tiny_skia::Pixmap,
) -> Preview {
    let id = Id::unique(("preview", key.0, key.1, key.2.r, key.2.g, key.2.b));
    let tex: Option<egui::TextureHandle> = ctx.data(|d| d.get_temp(id));
    let tex = tex.unwrap_or_else(|| {
        let p = make();
        let img = egui::ColorImage::from_rgba_premultiplied([p.width() as usize, p.height() as usize], p.data());
        let tex = ctx.load_texture(format!("preview-{id:?}"), img, egui::TextureOptions::LINEAR);
        ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
        tex
    });
    let size = tex.size_vec2();
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
        ShapeKind::Polygon => {
            let pts = vec![pt(0.15, 1.0), pt(0.35, 0.0), pt(0.95, 0.0), pt(0.75, 0.55), pt(1.0, 1.0)];
            p.add(egui::Shape::closed_line(pts, stroke));
        }
        k => {
            let rect = RectF::from_xywh(r.left(), r.top(), r.width(), r.height());
            if let Some(path) = shapes::box_shape_path(k, &rect) {
                for sub in flatten_path(&path) {
                    p.add(egui::Shape::closed_line(sub, stroke));
                }
            }
        }
    }
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
