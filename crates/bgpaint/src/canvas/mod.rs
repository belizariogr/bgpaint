//! The central canvas area: image tiles, overlays, rulers, grid, scrollbars, the size/opacity panel
//! and pointer input.

mod cursors;
pub mod tiles;
pub mod view;

use crate::app::PaintApp;
use crate::theme::Theme;
use crate::tools::interaction::{Active, Handle};
use crate::tools::{MAX_SIZE, Tool};
use crate::ui::icons::regular as ic;
use crate::ui::widgets;
use bgpaint_core::geom::Point;
use cursors::ToolCursor;
use egui::{Align2, Color32, CursorIcon, Id, Rect, Sense, Stroke, Vec2, pos2, vec2};

const RULER: f32 = 20.0;
const SCROLLBAR: f32 = 12.0;
/// Right edge of the floating size/opacity bar, measured from the left of the canvas area.
const SIZE_BAR_RIGHT: f32 = 12.0 + 38.0;
/// Smallest on-screen diameter of the brush/eraser outline cursor, in points.
const MIN_OUTLINE: f32 = 6.0;

impl PaintApp {
    pub fn canvas_panel(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.canvas_area)).show(ui, |ui| {
            let full = ui.max_rect();
            let mut viewport = full;
            viewport.max -= vec2(SCROLLBAR, SCROLLBAR);
            if self.settings.show_rulers {
                viewport.min += vec2(RULER, RULER);
            }
            let ctx = ui.ctx().clone();
            let ppp = ctx.pixels_per_point();
            // Keep the canvas clear of the floating size bar (see `size_panel`).
            self.view.left_inset = (SIZE_BAR_RIGHT - if self.settings.show_rulers { RULER } else { 0.0 }).max(0.0);
            self.view.layout(viewport, (self.doc.width(), self.doc.height()), ppp);

            let resp = ui.interact(viewport, Id::unique("canvas"), Sense::click_and_drag());
            self.canvas_input(&ctx, &resp, viewport);
            self.view.layout(viewport, (self.doc.width(), self.doc.height()), ppp);

            // Upload changed pixels.
            let mut dirty = self.doc.take_dirty();
            if dirty.is_some() {
                self.thumbs.stale = true;
            }
            if let Some(d) = self.preview_dirty.take() {
                dirty = Some(dirty.map_or(d, |x| x.union(&d)));
            }
            let smooth = self.view.zoom < 0.999;
            self.tiles.update(&ctx, &self.doc, self.active.preview(), dirty, smooth);

            let painter = ui.painter_at(viewport);
            let canvas = self.view.canvas_rect();
            // Soft shadow under the canvas.
            painter.add(egui::Shadow { offset: [0, 2], blur: 10, spread: 0, color: t.canvas_shadow }.as_shape(canvas, 0));
            self.paint_transparency(&ctx, &painter, canvas, &t);
            self.tiles.paint(&painter, canvas.min, self.view.scale(), ppp);
            if self.settings.show_grid {
                self.paint_grid(&painter, &t, viewport);
            }
            self.paint_overlays(&painter, &t);
            self.paint_handles(&painter, &t);
            self.paint_cursor(&ctx, &painter, &resp);
            if self.settings.show_rulers {
                self.paint_rulers(ui, &t, full, viewport);
            }
            self.scrollbars(ui, &t, viewport);
            // Tools with a size (pencil, brushes, eraser, shapes) get the side sliders.
            self.size_panel(ui, &t, viewport);
            if self.ui.thumbnail {
                self.thumbnail_window(&ctx, &t, viewport);
            }
            self.update_status(&resp);
        });
    }

    fn canvas_input(&mut self, ctx: &egui::Context, resp: &egui::Response, viewport: Rect) {
        let (events, modifiers, space) = ctx.input(|i| (i.events.clone(), i.modifiers, i.key_down(egui::Key::Space)));
        let hover_ok = resp.contains_pointer() && self.ui.open_menu.is_none() && self.ui.modal.is_none();
        for e in events {
            match e {
                egui::Event::PointerButton { pos, button, pressed: true, modifiers } => {
                    if hover_ok && viewport.contains(pos) && self.active_button().is_none() {
                        if self.ui.ribbon_revealed {
                            self.ui.ribbon_revealed = false;
                        }
                        self.pointer_down(pos, button, modifiers, space);
                    }
                }
                egui::Event::PointerMoved(pos) => {
                    if !self.active.is_idle() {
                        self.pointer_move(pos, modifiers);
                    }
                }
                egui::Event::PointerButton { pos, button, pressed: false, .. } => self.pointer_up(pos, button),
                egui::Event::PointerGone => {}
                egui::Event::MouseWheel { delta, unit, modifiers, .. } if resp.contains_pointer() => {
                    let pixels = match unit {
                        egui::MouseWheelUnit::Point => delta,
                        egui::MouseWheelUnit::Line => delta * 40.0,
                        egui::MouseWheelUnit::Page => delta * viewport.height(),
                    };
                    if modifiers.command {
                        let anchor = resp.hover_pos().unwrap_or(viewport.center());
                        let factor = (pixels.y / 200.0).exp();
                        self.view.zoom_at(self.view.zoom * factor, anchor);
                    } else if modifiers.shift {
                        self.view.scroll(vec2(pixels.y + pixels.x, 0.0));
                    } else {
                        self.view.scroll(pixels);
                    }
                }
                egui::Event::Zoom(f) if resp.contains_pointer() => {
                    let anchor = resp.hover_pos().unwrap_or(viewport.center());
                    self.view.zoom_at(self.view.zoom * f, anchor);
                }
                _ => {}
            }
        }
        self.pointer_hold(ctx);
        if let Active::Stroke { .. } = self.active
            && let Some(p) = resp.hover_pos().or(ctx.input(|i| i.pointer.latest_pos()))
        {
            // Keep the stroke visible while drawing near the edges.
            if !viewport.contains(p) {
                self.view.ensure_visible(self.view.to_canvas(p));
            }
        }
    }

    /// The checkerboard behind transparent pixels, shown while the background is hidden.
    fn paint_transparency(&mut self, ctx: &egui::Context, painter: &egui::Painter, rect: Rect, t: &Theme) {
        if !self.doc.background().visible {
            self.checker.paint(ctx, painter, rect, [t.checker_a, t.checker_b]);
        }
    }

    fn active_button(&self) -> Option<egui::PointerButton> {
        match &self.active {
            Active::Stroke { button, .. } | Active::Pan { button, .. } => Some(*button),
            Active::Resize(_) => Some(egui::PointerButton::Primary),
            _ => None,
        }
    }

    fn paint_handles(&self, painter: &egui::Painter, t: &Theme) {
        if let Active::Resize(op) = &self.active {
            let a = self.view.to_screen(Point::new(op.rect.x0 as f32, op.rect.y0 as f32));
            let b = self.view.to_screen(Point::new(op.rect.x1 as f32, op.rect.y1 as f32));
            widgets::dashed_rect(painter, Rect::from_two_pos(a, b), Color32::BLACK, Color32::WHITE);
            return;
        }
        let border = if t.dark { Color32::from_gray(30) } else { Color32::from_gray(60) };
        for (_, r) in self.handle_rects() {
            painter.rect(r, 0, Color32::WHITE, Stroke::new(1.0, border), egui::StrokeKind::Inside);
        }
    }

    fn paint_cursor(&self, ctx: &egui::Context, painter: &egui::Painter, resp: &egui::Response) {
        let Some(pos) = resp.hover_pos() else { return };
        if self.ui.open_menu.is_some() || self.ui.modal.is_some() {
            return;
        }
        if let Active::Resize(op) = &self.active {
            ctx.set_cursor_icon(op.handle.cursor());
            return;
        }
        if matches!(self.active, Active::Pan { .. }) || ctx.input(|i| i.key_down(egui::Key::Space)) {
            ctx.set_cursor_icon(CursorIcon::Grabbing);
            return;
        }
        if self.active.is_idle()
            && let Some(h) = self.handle_at(pos)
        {
            ctx.set_cursor_icon(h.cursor());
            return;
        }
        let cursor = match self.tool {
            Tool::Pencil | Tool::Brush | Tool::Eraser => {
                let s = self.tools.stroke(self.tool).map_or(1.0, |s| s.size) * self.view.scale();
                // The outline always shows, even for tiny tips, so it never turns into a crosshair.
                let big = s >= 6.0;
                let s = s.max(MIN_OUTLINE);
                let r = s / 2.0;
                match self.tool {
                    Tool::Eraser => {
                        let rect = Rect::from_center_size(pos, Vec2::splat(s));
                        painter.rect_stroke(rect, 0, Stroke::new(1.0, Color32::from_white_alpha(200)), egui::StrokeKind::Outside);
                        painter.rect_stroke(rect, 0, Stroke::new(1.0, Color32::from_black_alpha(200)), egui::StrokeKind::Inside);
                    }
                    Tool::Brush => {
                        painter.circle_stroke(pos, r + 0.5, Stroke::new(1.0, Color32::from_white_alpha(200)));
                        painter.circle_stroke(pos, r - 0.5, Stroke::new(1.0, Color32::from_black_alpha(200)));
                    }
                    _ => {
                        // Pencil: the tip outline only when it is big enough to tell apart from the icon.
                        if big {
                            painter.circle_stroke(pos, r + 0.5, Stroke::new(1.0, Color32::from_white_alpha(200)));
                            painter.circle_stroke(pos, r - 0.5, Stroke::new(1.0, Color32::from_black_alpha(200)));
                        }
                        cursors::paint(ctx, painter, pos, ToolCursor::Pencil, self.colors.primary);
                    }
                }
                CursorIcon::None
            }
            Tool::Fill => {
                cursors::paint(ctx, painter, pos, ToolCursor::Fill, self.colors.primary);
                CursorIcon::None
            }
            Tool::Picker => {
                cursors::paint(ctx, painter, pos, ToolCursor::Picker, self.colors.primary);
                CursorIcon::None
            }
            Tool::Shape => self.shape_cursor(pos).unwrap_or(CursorIcon::Crosshair),
            Tool::Text => self.text_cursor(pos),
            Tool::Magnifier => CursorIcon::ZoomIn,
            Tool::Select => self.select_cursor(pos),
        };
        ctx.set_cursor_icon(cursor);
    }

    fn paint_grid(&self, painter: &egui::Painter, t: &Theme, viewport: Rect) {
        let scale = self.view.scale();
        let step = [1u32, 2, 5, 10, 20, 50, 100, 200, 500].into_iter().find(|&s| s as f32 * scale >= 8.0).unwrap_or(1000);
        let canvas = self.view.canvas_rect().intersect(viewport);
        if canvas.width() <= 0.0 {
            return;
        }
        let color =
            if t.dark { Color32::from_rgba_unmultiplied(128, 128, 128, 90) } else { Color32::from_rgba_unmultiplied(100, 100, 100, 70) };
        let stroke = Stroke::new(1.0, color);
        let (a, b) = (self.view.to_canvas(canvas.min), self.view.to_canvas(canvas.max));
        let first = |v: f32| ((v.max(0.0) / step as f32).ceil() as u32) * step;
        let ppp = self.view.ppp;
        let snap = |v: f32| (v * ppp).round() / ppp + 0.5 / ppp;
        let mut x = first(a.x);
        while (x as f32) < b.x {
            let sx = snap(self.view.to_screen(Point::new(x as f32, 0.0)).x);
            painter.vline(sx, canvas.y_range(), stroke);
            x += step;
        }
        let mut y = first(a.y);
        while (y as f32) < b.y {
            let sy = snap(self.view.to_screen(Point::new(0.0, y as f32)).y);
            painter.hline(canvas.x_range(), sy, stroke);
            y += step;
        }
    }

    fn paint_rulers(&self, ui: &egui::Ui, t: &Theme, full: Rect, viewport: Rect) {
        let painter = ui.painter_at(full);
        let top = Rect::from_min_max(pos2(viewport.left(), full.top()), pos2(viewport.right(), viewport.top()));
        let left = Rect::from_min_max(pos2(full.left(), viewport.top()), pos2(viewport.left(), viewport.bottom()));
        for r in [top, left] {
            painter.rect_filled(r, 0, t.ribbon_bg);
        }
        painter.rect_filled(Rect::from_min_max(full.min, viewport.min), 0, t.ribbon_bg);
        painter.hline(top.x_range(), top.bottom() - 0.5, Stroke::new(1.0, t.separator));
        painter.vline(left.right() - 0.5, left.y_range(), Stroke::new(1.0, t.separator));
        let scale = self.view.scale();
        let major = [10u32, 20, 50, 100, 200, 500, 1000, 2000, 5000].into_iter().find(|&s| s as f32 * scale >= 60.0).unwrap_or(10000);
        let minor = major / 10;
        let (a, b) = self.view.visible_canvas();
        let tick = Stroke::new(1.0, t.text_secondary);
        let font = egui::FontId::proportional(10.0);
        let start = |v: f32| ((v / minor as f32).floor() as i64) * minor as i64;
        let mut x = start(a.x);
        while (x as f32) <= b.x {
            let sx = self.view.to_screen(Point::new(x as f32, 0.0)).x.round() + 0.5;
            let len = if x % major as i64 == 0 {
                10.0
            } else if x % (major as i64 / 2) == 0 {
                6.0
            } else {
                3.0
            };
            painter.vline(sx, top.bottom() - len..=top.bottom(), tick);
            if x % major as i64 == 0 {
                painter.text(pos2(sx + 3.0, top.top() + 1.0), Align2::LEFT_TOP, x.to_string(), font.clone(), t.text_secondary);
            }
            x += minor as i64;
        }
        let mut y = start(a.y);
        while (y as f32) <= b.y {
            let sy = self.view.to_screen(Point::new(0.0, y as f32)).y.round() + 0.5;
            let len = if y % major as i64 == 0 {
                10.0
            } else if y % (major as i64 / 2) == 0 {
                6.0
            } else {
                3.0
            };
            painter.hline(left.right() - len..=left.right(), sy, tick);
            if y % major as i64 == 0 {
                for (i, ch) in y.to_string().chars().enumerate() {
                    painter.text(pos2(left.left() + 4.0, sy + 2.0 + i as f32 * 10.0), Align2::LEFT_TOP, ch, font.clone(), t.text_secondary);
                }
            }
            y += minor as i64;
        }
        // Cursor markers.
        if let Some(p) = ui.ctx().pointer_hover_pos().filter(|p| viewport.contains(*p)) {
            painter.vline(p.x, top.y_range(), Stroke::new(1.0, t.accent));
            painter.hline(left.x_range(), p.y, Stroke::new(1.0, t.accent));
        }
    }

    fn scrollbars(&mut self, ui: &mut egui::Ui, t: &Theme, viewport: Rect) {
        let content = self.view.canvas_size_points() + Vec2::splat(view::MARGIN * 2.0) + vec2(self.view.left_inset, 0.0);
        for vertical in [false, true] {
            let axis = usize::from(vertical);
            let (vsize, csize) = (viewport.size()[axis], content[axis]);
            if self.view.fits(axis) {
                continue;
            }
            let track = if vertical {
                Rect::from_min_max(pos2(viewport.right(), viewport.top()), pos2(viewport.right() + SCROLLBAR, viewport.bottom()))
            } else {
                Rect::from_min_max(pos2(viewport.left(), viewport.bottom()), pos2(viewport.right(), viewport.bottom() + SCROLLBAR))
            };
            let id = Id::unique(("scrollbar", vertical));
            let resp = ui.interact(track, id, Sense::click_and_drag());
            let hot = resp.hovered() || resp.dragged();
            let len = track.size()[axis];
            let thumb_len = (len * vsize / csize).max(24.0);
            let scrolled = view::MARGIN + self.view.lead(axis) - self.view.offset[axis];
            let max_scroll = csize - vsize;
            let pos = (scrolled / max_scroll).clamp(0.0, 1.0) * (len - thumb_len);
            if resp.dragged() {
                let d = resp.drag_delta()[axis] * max_scroll / (len - thumb_len).max(1.0);
                let mut delta = Vec2::ZERO;
                delta[axis] = -d;
                self.view.scroll(delta);
            } else if resp.clicked()
                && let Some(p) = resp.interact_pointer_pos()
            {
                let page = if p[axis] - track.min[axis] < pos { vsize } else { -vsize };
                let mut delta = Vec2::ZERO;
                delta[axis] = page * 0.9;
                self.view.scroll(delta);
            }
            let thickness = if hot { 6.0 } else { 2.0 };
            let thumb = if vertical {
                Rect::from_min_size(pos2(track.center().x - thickness / 2.0, track.top() + pos), vec2(thickness, thumb_len))
            } else {
                Rect::from_min_size(pos2(track.left() + pos, track.center().y - thickness / 2.0), vec2(thumb_len, thickness))
            };
            ui.painter().rect_filled(thumb, 3, t.text_secondary.gamma_multiply(if hot { 0.9 } else { 0.6 }));
        }
    }

    /// The floating panel with the size and opacity sliders (left of the canvas).
    fn size_panel(&mut self, ui: &mut egui::Ui, t: &Theme, viewport: Rect) {
        let tool = self.tool;
        let Some(mut stroke) = self.tools.stroke(tool) else { return };
        let h = (viewport.height() - 120.0).clamp(160.0, 720.0);
        let panel_h = (h - 12.0) / 2.0;
        let top = viewport.center().y - h / 2.0;
        let x = viewport.left() - if self.settings.show_rulers { RULER } else { 0.0 } + 12.0;
        let rects = [
            Rect::from_min_size(pos2(x, top), vec2(38.0, panel_h)),
            Rect::from_min_size(pos2(x, top + panel_h + 12.0), vec2(38.0, panel_h)),
        ];
        let mut changed = false;
        for (i, r) in rects.iter().enumerate() {
            // Each panel lives in its own area so the canvas below never sees its clicks.
            egui::Area::new(Id::unique(("size-panel-area", i))).order(egui::Order::Middle).fixed_pos(r.min).show(ui.ctx(), |ui| {
                let (r, _) = ui.allocate_exact_size(r.size(), Sense::hover());
                ui.painter().rect(r, 6, t.panel_bg, Stroke::new(1.0, t.panel_border), egui::StrokeKind::Inside);
                let glyph = if i == 0 { ic::SIZE } else { ic::DROP };
                widgets::paint_icon(ui.painter(), pos2(r.center().x, r.top() + 18.0), glyph, 16.0, t.text);
                let slider_rect = Rect::from_min_max(pos2(r.left(), r.top() + 32.0), pos2(r.right(), r.bottom() - 6.0));
                let mut v = if i == 0 { ((stroke.size - 1.0) / (MAX_SIZE - 1.0)).sqrt() } else { stroke.opacity };
                let resp = widgets::slider(ui, t, slider_rect, Id::unique(("size-panel", i)), &mut v, true);
                if resp.dragged() || resp.clicked() || resp.is_pointer_button_down_on() {
                    if i == 0 {
                        stroke.size = (1.0 + v * v * (MAX_SIZE - 1.0)).round();
                    } else {
                        stroke.opacity = (v * 100.0).round() / 100.0;
                    }
                    changed = true;
                }
                if resp.hovered() || resp.dragged() {
                    let text =
                        if i == 0 { format!("{} px", stroke.size as i32) } else { format!("{}%", (stroke.opacity * 100.0).round() as i32) };
                    resp.on_hover_text_at_pointer(text);
                }
            });
        }
        if changed && let Some(s) = self.tools.stroke_mut(tool) {
            *s = stroke;
            if tool == Tool::Shape {
                // Redraw a shape still being edited with the new width or opacity.
                self.on_shape_style_changed();
            }
        }
    }

    fn thumbnail_window(&mut self, ctx: &egui::Context, t: &Theme, viewport: Rect) {
        let max = 220.0;
        let (w, h) = (self.doc.width() as f32, self.doc.height() as f32);
        let s = max / w.max(h);
        let size = vec2(w * s, h * s);
        egui::Area::new(Id::unique("thumbnail"))
            .order(egui::Order::Middle)
            .default_pos(pos2(viewport.right() - size.x - 24.0, viewport.top() + 12.0))
            .constrain_to(viewport)
            .movable(true)
            .show(ctx, |ui| {
                widgets::panel_frame(t).show(ui, |ui| {
                    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
                    self.paint_transparency(ctx, ui.painter(), rect, t);
                    let ppp = ctx.pixels_per_point();
                    self.tiles.paint(&ui.painter_at(rect), rect.min, s, ppp);
                    // Visible area.
                    let (a, b) = self.view.visible_canvas();
                    let vis = Rect::from_min_max(rect.min + vec2(a.x, a.y) * s, rect.min + vec2(b.x, b.y) * s).intersect(rect);
                    ui.painter().rect_stroke(vis, 0, Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
                    if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
                        let target = Point::new((p.x - rect.left()) / s, (p.y - rect.top()) / s);
                        let center = self.view.viewport.center();
                        let cur = self.view.to_screen(target);
                        self.view.scroll(center - cur);
                    }
                });
            });
    }

    fn update_status(&mut self, resp: &egui::Response) {
        self.status.cursor = resp
            .hover_pos()
            .map(|p| self.view.to_canvas(p))
            .filter(|p| self.doc.bounds().contains(p.x.floor() as i32, p.y.floor() as i32))
            .map(|p| p.floor_i());
        self.status.selection = self.selection_size();
        if let Active::Resize(op) = &self.active {
            self.status.selection = Some((op.rect.width(), op.rect.height()));
        }
    }

    pub fn paint_overlays(&self, painter: &egui::Painter, t: &Theme) {
        self.paint_tool_overlays(painter, t);
    }
}

impl Handle {
    pub fn is_corner(self) -> bool {
        matches!(self, Handle::NE | Handle::NW | Handle::SE | Handle::SW)
    }
}
