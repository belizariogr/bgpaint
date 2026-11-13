//! The Text tool: an editable text box (caret, selection, clipboard, IME) rendered with system
//! fonts, committed when the user clicks outside or switches tools: into the layer's pixels, or
//! in vector mode as a vector object that a later click opens for editing again.

use super::Tool;
use super::interaction::Active;
use super::shape::Lifted;
use super::transform::{BoxGeom, BoxHit};
use crate::app::PaintApp;
use crate::canvas::view::View;
use crate::theme::Theme;
use bgpaint_core::cosmic_text::{Action, Buffer, Cursor, Edit, Editor, Motion, Selection};
use bgpaint_core::document::ActivePreview;
use bgpaint_core::geom::{PixelRect, Point, RectF};
use bgpaint_core::history::Change;
use bgpaint_core::shapes::{RasterShape, ShapePaint};
pub use bgpaint_core::text::fonts;
use bgpaint_core::text::{self, Fonts, TextAlign, TextStyle};
use bgpaint_core::tiny_skia::Pixmap;
use bgpaint_core::vector::{ShapeGeom, VectorShape};
use egui::{Color32, Key, PointerButton, Pos2, Stroke};
use std::time::Instant;

/// Starts scanning system fonts in the background so the text tool is instant later.
pub fn preload_fonts() {
    std::thread::spawn(|| {
        drop(fonts());
    });
}

/// Text formatting chosen in the text toolbar.
#[derive(Clone, Debug)]
pub struct TextSettings {
    pub family: Option<String>,
    pub size_pt: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub align: TextAlign,
    pub background: bool,
    /// The side panel's opacity slider (0 to 1).
    pub opacity: f32,
}

impl Default for TextSettings {
    fn default() -> Self {
        Self {
            family: None,
            size_pt: 11.0,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            align: TextAlign::Left,
            background: false,
            opacity: 1.0,
        }
    }
}

#[derive(Clone)]
enum TextDrag {
    Create { start: Point },
    Box(BoxHit, Point, BoxGeom),
    Select,
}

pub struct TextOp {
    pub geom: BoxGeom,
    editor: Editor<'static>,
    style: TextStyle,
    /// The style the toolbar and colors gave last. A change only overrides the colors that
    /// changed, so restyling a reopened object keeps its own.
    applied: TextStyle,
    raster: Option<RasterShape>,
    drag: Option<TextDrag>,
    canvas: (u32, u32),
    blink: Instant,
    undo: Vec<String>,
    /// Time, position and count of the last click inside the box (double/triple click).
    last_click: Option<(Instant, Point, u8)>,
    /// Becomes a vector object when committed, instead of pixels.
    vector: bool,
    lifted: Option<Lifted>,
    /// The box fits itself to the text; a manual resize turns this off for good. A new box gets
    /// it when the click that creates it ends without dragging out a size.
    auto_size: bool,
}

impl ActivePreview for TextOp {
    fn bounds(&self) -> PixelRect {
        self.raster.as_ref().map_or(PixelRect::EMPTY, RasterShape::bounds)
    }

    fn apply(&self, region: &mut Pixmap, rect: PixelRect) {
        if let Some(r) = &self.raster {
            r.draw_into(region, (rect.x0, rect.y0));
        }
    }

    fn rendered(&self) -> Option<&RasterShape> {
        self.raster.as_ref()
    }

    fn above_objects(&self) -> bool {
        self.vector
    }
}

impl TextOp {
    pub fn text(&self) -> String {
        self.editor.with_buffer(|b| b.lines.iter().map(|l| l.text()).collect::<Vec<_>>().join("\n"))
    }

    /// Re-lays out and re-renders the box; returns the area to redraw.
    fn update(&mut self, fonts: &mut Fonts) -> PixelRect {
        let before = self.bounds();
        if self.auto_size {
            let (w, h) = self.editor.with_buffer_mut(|b| text::fit_buffer(fonts, b));
            self.geom = self.geom.resized(w, h, self.style.align.anchor());
        }
        let width = text::layout_width(self.geom.rect.width());
        self.editor.with_buffer_mut(|b| {
            b.set_size(Some(width), None);
            b.shape_until_scroll(&mut fonts.system, false);
        });
        let (w, h) = (self.geom.rect.width().ceil() as u32, self.geom.rect.height().ceil() as u32);
        let style = self.style.clone();
        let pixmap = self.editor.with_buffer_mut(|b| text::render(fonts, b, &style, w.max(1), h.max(1)));
        self.raster = text::place_box(&pixmap, self.geom.rect, self.geom.angle, self.canvas);
        before.union(&self.bounds())
    }

    /// Re-applies the style to the whole text, keeping the caret.
    fn restyle(&mut self, fonts: &mut Fonts, style: TextStyle) {
        let cursor = self.editor.cursor();
        let text = self.text();
        self.style = style;
        let attrs = self.style.attrs();
        let align = self.style.cosmic_align();
        let metrics = self.style.metrics();
        self.editor.with_buffer_mut(|b| {
            b.set_metrics(metrics);
            b.set_text(&text, &attrs, bgpaint_core::cosmic_text::Shaping::Advanced, Some(align));
        });
        self.editor.set_cursor(cursor);
        self.editor.shape_as_needed(&mut fonts.system, false);
    }

    /// Position inside the text layout. Points below the last line (or above the first)
    /// map onto that line, as in a regular text field.
    fn local(&self, p: Point) -> (i32, i32) {
        let l = self.geom.to_local(p);
        let content = self.editor.with_buffer(text::content_height);
        let y = (l.y - self.geom.rect.min.y).clamp(0.0, (content - 1.0).max(0.0));
        ((l.x - self.geom.rect.min.x - text::BOX_PADDING).round() as i32, y as i32)
    }

    fn snapshot(&mut self) {
        let t = self.text();
        if self.undo.last() != Some(&t) {
            self.undo.push(t);
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
        }
    }
}

impl PaintApp {
    pub fn text_style(&self, fonts: &Fonts) -> TextStyle {
        let s = &self.text_settings;
        TextStyle {
            family: s.family.clone().unwrap_or_else(|| fonts.default_family()),
            size_pt: s.size_pt,
            bold: s.bold,
            italic: s.italic,
            underline: s.underline,
            strikethrough: s.strikethrough,
            align: s.align,
            color: self.colors.primary,
            background: s.background.then_some(self.colors.secondary),
            opacity: s.opacity,
        }
    }

    /// Saves the current font and size as the ones the next start uses, when they changed.
    pub fn remember_text_font(&mut self) {
        let s = &self.text_settings;
        if self.settings.text_family != s.family || self.settings.text_size_pt != s.size_pt {
            self.settings.text_family = s.family.clone();
            self.settings.text_size_pt = s.size_pt;
            self.settings.save();
        }
    }

    pub fn text_pointer_down(&mut self, p: Point, pos: Pos2, button: PointerButton, modifiers: egui::Modifiers) {
        let tol = 6.0 / self.view.scale();
        if let Active::Text(op) = &mut self.active
            && button == PointerButton::Primary
        {
            let inner = RectF {
                min: Point::new(op.geom.rect.min.x + tol, op.geom.rect.min.y + tol),
                max: Point::new(op.geom.rect.max.x - tol, op.geom.rect.max.y - tol),
            };
            let local = op.geom.to_local(p);
            if inner.contains(local) {
                let (x, y) = op.local(p);
                let mut f = fonts();
                let near = 4.0 / self.view.scale();
                let count = match op.last_click {
                    Some((at, q, n)) if at.elapsed().as_millis() < 450 && (q.x - p.x).abs() < near && (q.y - p.y).abs() < near => n % 3 + 1,
                    _ => 1,
                };
                op.last_click = Some((Instant::now(), p, count));
                match count {
                    2 => op.editor.action(&mut f.system, Action::DoubleClick { x, y }),
                    3 => op.editor.action(&mut f.system, Action::TripleClick { x, y }),
                    _ if modifiers.shift => {
                        // Shift+click extends the selection from the caret.
                        if op.editor.selection() == Selection::None {
                            op.editor.set_selection(Selection::Normal(op.editor.cursor()));
                        }
                        op.editor.action(&mut f.system, Action::Drag { x, y });
                    }
                    _ => op.editor.action(&mut f.system, Action::Click { x, y }),
                }
                op.drag = Some(TextDrag::Select);
                op.blink = Instant::now();
                return;
            }
            if let Some(hit) = op.geom.hit(p, tol, 24.0 / self.view.scale()) {
                op.drag = Some(TextDrag::Box(hit, p, op.geom));
                return;
            }
        }
        // A click outside ends the editing: in vector mode the Pointer takes over from here.
        let editing = matches!(self.active, Active::Text(_));
        self.commit_interaction();
        if editing && self.doc.vector_mode() {
            self.hand_over_to_pointer();
            self.shape_pointer_down(p, pos, button, modifiers);
            return;
        }
        // In vector mode a click on a text object opens it again.
        let target =
            self.doc.object_at(p, tol).filter(|_| button == PointerButton::Primary && self.doc.vector_mode()).and_then(|(l, i)| {
                let o = &self.doc.layers()[l].objects[i];
                match &o.geom {
                    ShapeGeom::Text { rect, angle, text, style, auto_size } => {
                        Some((l, i, BoxGeom { rect: *rect, angle: *angle }, text.clone(), o.paint.text_style(style), *auto_size))
                    }
                    _ => None,
                }
            });
        if let Some((layer, index, geom, words, style, auto_size)) = target
            && let Some(lifted) = self.take_object_for_edit(layer, index)
        {
            self.open_text_object(geom, &words, style, auto_size, Some(lifted), p);
            return;
        }
        let mut f = fonts();
        let style = self.text_style(&f);
        let buffer = f.buffer("", &style, 200.0);
        let editor = Editor::new(buffer);
        let mut op = TextOp {
            geom: BoxGeom::new(RectF::from_xywh(p.x, p.y, 1.0, 1.0)),
            editor,
            applied: style.clone(),
            style,
            raster: None,
            drag: Some(TextDrag::Create { start: p }),
            canvas: (self.doc.width(), self.doc.height()),
            blink: Instant::now(),
            undo: Vec::new(),
            last_click: None,
            vector: self.doc.vector_mode(),
            lifted: None,
            auto_size: false,
        };
        op.update(&mut f);
        self.active = Active::Text(op);
    }

    /// Opens a text object for editing with the Text tool, the caret at `p`. The toolbar shows
    /// the object's formatting.
    pub(super) fn open_text_object(
        &mut self,
        geom: BoxGeom,
        words: &str,
        style: TextStyle,
        auto_size: bool,
        lifted: Option<Lifted>,
        p: Point,
    ) {
        self.text_settings = TextSettings {
            family: Some(style.family.clone()),
            size_pt: style.size_pt,
            bold: style.bold,
            italic: style.italic,
            underline: style.underline,
            strikethrough: style.strikethrough,
            align: style.align,
            background: style.background.is_some(),
            opacity: style.opacity,
        };
        self.set_tool(Tool::Text);
        let mut f = fonts();
        let applied = self.text_style(&f);
        let buffer = f.buffer(words, &style, text::layout_width(geom.rect.width()));
        let mut op = TextOp {
            geom,
            editor: Editor::new(buffer),
            style,
            applied,
            raster: None,
            drag: Some(TextDrag::Select),
            canvas: (self.doc.width(), self.doc.height()),
            blink: Instant::now(),
            undo: Vec::new(),
            last_click: Some((Instant::now(), p, 1)),
            vector: true,
            lifted,
            auto_size,
        };
        let d = op.update(&mut f);
        self.preview_dirty.add(d);
        let (x, y) = op.local(p);
        op.editor.action(&mut f.system, Action::Click { x, y });
        self.active = Active::Text(op);
    }

    /// Finished text hands over to the Pointer (only vector mode has one).
    fn hand_over_to_pointer(&mut self) {
        if self.doc.vector_mode() {
            self.set_tool(Tool::Shape);
            self.tools.vector_pointer = true;
        }
    }

    pub fn text_pointer_move(&mut self, op: &mut TextOp, p: Point) {
        let mut f = fonts();
        match op.drag.clone() {
            Some(TextDrag::Create { start }) => {
                op.geom.rect = RectF::from_points(start, p);
            }
            Some(TextDrag::Box(hit, start, orig)) => {
                // Sizing the box by hand ends its automatic sizing.
                if matches!(hit, BoxHit::Handle(_)) {
                    op.auto_size = false;
                }
                op.geom = orig.drag(hit, start, p, false);
                let d = op.update(&mut f);
                self.preview_dirty.add(d);
            }
            Some(TextDrag::Select) => {
                let (x, y) = op.local(p);
                op.editor.action(&mut f.system, Action::Drag { x, y });
            }
            None => {}
        }
    }

    pub fn text_pointer_up(&mut self, op: &mut TextOp, _p: Point) {
        if let Some(TextDrag::Create { .. }) = op.drag {
            let line = op.style.metrics().line_height;
            let r = op.geom.rect;
            // A click (or a tiny drag) creates a box that fits itself to the text; dragging out
            // a size creates one of that size.
            op.auto_size = r.width() < 16.0 && r.height() < line;
            let w = if op.auto_size { 1.0 } else { r.width().max(16.0) };
            let h = if r.height() < line { line + 4.0 } else { r.height() };
            op.geom.rect = RectF::from_xywh(r.min.x, r.min.y, w, h);
            let mut f = fonts();
            let d = op.update(&mut f);
            self.preview_dirty.add(d);
        }
        op.drag = None;
    }

    pub fn commit_text(&mut self, op: TextOp) {
        self.preview_dirty.add(op.bounds());
        if op.text().trim().is_empty() && op.style.background.is_none() {
            // A reopened object left with no words is deleted.
            if let Some(l) = op.lifted {
                self.history.push("delete text", vec![Change::objects(l.layer, l.before)]);
            }
            return;
        }
        if op.vector {
            let geom = ShapeGeom::Text {
                rect: op.geom.rect,
                angle: op.geom.angle,
                text: op.text(),
                style: op.style.clone(),
                auto_size: op.auto_size,
            };
            let shape = VectorShape::with_raster(geom, ShapePaint::for_text(&op.style), op.raster);
            self.place_object(shape, op.lifted, "text");
            return;
        }
        let Some(r) = &op.raster else { return };
        let change = self.doc.edit_active(r.bounds(), |layer| r.draw_into(layer, (0, 0)));
        self.history.push_one("text", change);
    }

    /// Drops the text being edited; a reopened object goes back unchanged.
    pub fn cancel_text(&mut self, op: TextOp) {
        if let Some(l) = op.lifted {
            self.place_object(l.original.clone(), Some(l), "text");
        }
    }

    /// Applies toolbar or color changes to the open text box.
    pub fn restyle_text(&mut self) {
        if !matches!(self.active, Active::Text(_)) {
            return;
        }
        let mut f = fonts();
        let new = self.text_style(&f);
        if let Active::Text(op) = &mut self.active {
            let mut style = new.clone();
            if new.color == op.applied.color {
                style.color = op.style.color;
            }
            if new.background == op.applied.background {
                style.background = op.style.background;
            }
            op.applied = new;
            op.restyle(&mut f, style);
            let d = op.update(&mut f);
            self.preview_dirty.add(d);
        }
    }

    pub fn text_cursor(&self, pos: Pos2) -> egui::CursorIcon {
        if let Active::Text(op) = &self.active {
            // A drag keeps the cursor of what it started on, wherever the pointer goes.
            if let Some(TextDrag::Box(hit, _, orig)) = &op.drag {
                return hit.cursor(orig.angle);
            }
            let p = self.view.to_canvas(pos);
            let tol = 6.0 / self.view.scale();
            let local = op.geom.to_local(p);
            let r = op.geom.rect;
            let inner = local.x > r.min.x + tol && local.x < r.max.x - tol && local.y > r.min.y + tol && local.y < r.max.y - tol;
            if !inner && let Some(h) = op.geom.hit(p, tol, 24.0 / self.view.scale()) {
                return h.cursor(op.geom.angle);
            }
        }
        egui::CursorIcon::Text
    }

    pub fn is_editing_text(&self) -> bool {
        matches!(self.active, Active::Text(_))
    }

    /// Routes keyboard input to the text box. Returns true if the text box consumed the frame's keys.
    pub fn text_keyboard(&mut self, ctx: &egui::Context) -> bool {
        if !self.is_editing_text() || ctx.egui_wants_keyboard_input() || self.ui.modal.is_some() || self.ui.open_menu.is_some() {
            return false;
        }
        let events = ctx.input_mut(|i| {
            let mut taken = Vec::new();
            i.events.retain(|e| {
                let ours = match e {
                    egui::Event::Text(_) | egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_) | egui::Event::Ime(_) => true,
                    egui::Event::Key { key, pressed: true, modifiers, .. } => {
                        use Key::*;
                        let editing = matches!(
                            key,
                            Backspace
                                | Delete
                                | Enter
                                | Tab
                                | Escape
                                | ArrowLeft
                                | ArrowRight
                                | ArrowUp
                                | ArrowDown
                                | Home
                                | End
                                | PageUp
                                | PageDown
                        );
                        let styled = modifiers.command && matches!(key, A | Z | B | I | U);
                        (editing && !(modifiers.command && matches!(key, PageUp | PageDown | Tab))) || styled
                    }
                    _ => false,
                };
                if ours {
                    taken.push(e.clone());
                }
                !ours
            });
            taken
        });
        let mut commit = false;
        let mut style_changed = false;
        {
            let Active::Text(op) = &mut self.active else { return false };
            let mut f = fonts();
            let mut changed = false;
            for e in events {
                match e {
                    egui::Event::Text(t) | egui::Event::Paste(t) | egui::Event::Ime(egui::ImeEvent::Commit(t)) => {
                        op.snapshot();
                        op.editor.delete_selection();
                        for c in t.chars().filter(|c| *c == '\n' || !c.is_control()) {
                            let action = if c == '\n' { Action::Enter } else { Action::Insert(c) };
                            op.editor.action(&mut f.system, action);
                        }
                        changed = true;
                    }
                    egui::Event::Copy | egui::Event::Cut => {
                        if let Some(s) = op.editor.copy_selection() {
                            ctx.copy_text(s);
                            if matches!(e, egui::Event::Cut) {
                                op.snapshot();
                                op.editor.delete_selection();
                                changed = true;
                            }
                        }
                    }
                    egui::Event::Key { key, modifiers, .. } => {
                        let cmd = modifiers.command;
                        let motion = |k: Key| -> Option<Motion> {
                            Some(match (k, cmd) {
                                (Key::ArrowLeft, true) => Motion::LeftWord,
                                (Key::ArrowRight, true) => Motion::RightWord,
                                (Key::ArrowLeft, false) => Motion::Left,
                                (Key::ArrowRight, false) => Motion::Right,
                                (Key::ArrowUp, _) => Motion::Up,
                                (Key::ArrowDown, _) => Motion::Down,
                                (Key::Home, true) => Motion::BufferStart,
                                (Key::End, true) => Motion::BufferEnd,
                                (Key::Home, false) => Motion::Home,
                                (Key::End, false) => Motion::End,
                                (Key::PageUp, _) => Motion::PageUp,
                                (Key::PageDown, _) => Motion::PageDown,
                                _ => return None,
                            })
                        };
                        if let Some(m) = motion(key) {
                            if modifiers.shift {
                                if op.editor.selection() == Selection::None {
                                    let c = op.editor.cursor();
                                    op.editor.set_selection(Selection::Normal(c));
                                }
                            } else {
                                op.editor.set_selection(Selection::None);
                            }
                            op.editor.action(&mut f.system, Action::Motion(m));
                        } else {
                            match key {
                                Key::Backspace => {
                                    op.snapshot();
                                    op.editor.action(&mut f.system, Action::Backspace);
                                    changed = true;
                                }
                                Key::Delete => {
                                    op.snapshot();
                                    op.editor.action(&mut f.system, Action::Delete);
                                    changed = true;
                                }
                                Key::Enter => {
                                    op.snapshot();
                                    op.editor.delete_selection();
                                    op.editor.action(&mut f.system, Action::Enter);
                                    changed = true;
                                }
                                Key::Tab => {
                                    op.snapshot();
                                    op.editor.action(&mut f.system, Action::Insert('\t'));
                                    changed = true;
                                }
                                Key::Escape => commit = true,
                                Key::A if cmd => {
                                    op.editor.set_cursor(Cursor::new(0, 0));
                                    op.editor.set_selection(Selection::Normal(Cursor::new(0, 0)));
                                    op.editor.action(&mut f.system, Action::Motion(Motion::BufferEnd));
                                }
                                Key::Z if cmd => {
                                    if let Some(prev) = op.undo.pop() {
                                        let style = op.style.clone();
                                        op.editor.with_buffer_mut(|b| {
                                            b.set_text(
                                                &prev,
                                                &style.attrs(),
                                                bgpaint_core::cosmic_text::Shaping::Advanced,
                                                Some(style.cosmic_align()),
                                            )
                                        });
                                        // The old selection may point past the restored text.
                                        op.editor.set_selection(Selection::None);
                                        op.editor.set_cursor(Cursor::new(0, 0));
                                        op.editor.action(&mut f.system, Action::Motion(Motion::BufferEnd));
                                        changed = true;
                                    }
                                }
                                Key::B if cmd => {
                                    self.text_settings.bold = !self.text_settings.bold;
                                    style_changed = true;
                                }
                                Key::I if cmd => {
                                    self.text_settings.italic = !self.text_settings.italic;
                                    style_changed = true;
                                }
                                Key::U if cmd => {
                                    self.text_settings.underline = !self.text_settings.underline;
                                    style_changed = true;
                                }
                                _ => {}
                            }
                        }
                        op.blink = Instant::now();
                    }
                    _ => {}
                }
            }
            if changed {
                op.blink = Instant::now();
                let d = op.update(&mut f);
                self.preview_dirty.add(d);
            }
        }
        if style_changed {
            self.restyle_text();
        }
        if commit {
            self.commit_interaction();
            self.hand_over_to_pointer();
        }
        // Tell the platform where the caret is, for input methods.
        if let Active::Text(op) = &self.active {
            let rect = egui::Rect::from_two_pos(self.view.to_screen(op.geom.rect.min), self.view.to_screen(op.geom.rect.max));
            let caret = caret_rect(op, &self.view).unwrap_or(rect);
            ctx.output_mut(|o| {
                o.ime = Some(egui::output::IMEOutput {
                    purpose: egui::IMEPurpose::Normal,
                    rect,
                    cursor_rect: caret,
                    should_interrupt_composition: false,
                });
            });
        }
        true
    }
}

fn caret_rect(op: &TextOp, view: &View) -> Option<egui::Rect> {
    let (x, y) = op.editor.cursor_position()?;
    let lh = op.style.metrics().line_height;
    let left = op.geom.rect.min.x + text::BOX_PADDING + x as f32;
    let top = op.geom.to_world(Point::new(left, op.geom.rect.min.y + y as f32));
    let bottom = op.geom.to_world(Point::new(left, op.geom.rect.min.y + y as f32 + lh));
    Some(egui::Rect::from_two_pos(view.to_screen(top), view.to_screen(bottom)))
}

/// Box frame, selection highlight and blinking caret.
pub fn paint_overlay(op: &TextOp, view: &View, painter: &egui::Painter, t: &Theme) {
    if matches!(op.drag, Some(TextDrag::Create { .. })) {
        let r = egui::Rect::from_two_pos(view.to_screen(op.geom.rect.min), view.to_screen(op.geom.rect.max));
        crate::ui::widgets::dashed_rect(painter, r, Color32::from_gray(20), Color32::WHITE);
        return;
    }
    op.geom.paint(view, painter, t, true);
    let origin = Point::new(op.geom.rect.min.x + text::BOX_PADDING, op.geom.rect.min.y);
    let lh = op.style.metrics().line_height;
    if let Some((a, b)) = op.editor.selection_bounds() {
        op.editor.with_buffer(|buf: &Buffer| {
            for run in buf.layout_runs() {
                for (x, w) in run.highlight(a, b) {
                    let pts = [(x, run.line_top), (x + w, run.line_top), (x + w, run.line_top + lh), (x, run.line_top + lh)]
                        .map(|(px, py)| view.to_screen(op.geom.to_world(Point::new(origin.x + px, origin.y + py))));
                    painter.add(egui::Shape::convex_polygon(pts.to_vec(), t.accent.gamma_multiply(0.35), Stroke::NONE));
                }
            }
        });
    }
    let on = (op.blink.elapsed().as_millis() / 530).is_multiple_of(2);
    if on && let Some(r) = caret_rect(op, view) {
        let c = op.style.color;
        painter.line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.5, Color32::from_rgb(c.r, c.g, c.b)));
    }
    painter.ctx().request_repaint_after(std::time::Duration::from_millis(530));
}
