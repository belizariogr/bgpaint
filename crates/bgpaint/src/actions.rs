//! Execution of [`Command`]s.

use crate::app::PaintApp;
use crate::commands::Command;
use crate::tools::{MAX_FONT_SIZE, MAX_SIZE, Tool};
use bgpaint_core::history::Change;
use bgpaint_core::io::SaveFormat;
use bgpaint_core::raster::{self, QuarterTurn};

impl PaintApp {
    pub fn execute(&mut self, ctx: &egui::Context, cmd: Command) {
        match cmd {
            Command::New => self.new_tab(),
            Command::Open => {
                self.commit_interaction();
                self.open_dialog(ctx);
            }
            Command::OpenRecent(p) => self.open_path(ctx, p),
            Command::SwitchTab(i) => self.switch_tab(i),
            Command::CycleTab(forward) => self.cycle_tab(forward),
            Command::CloseTab(i) => self.close_tab(if i == usize::MAX { self.current_tab } else { i }),
            Command::ImportFromFile => {
                self.commit_interaction();
                self.import_dialog(ctx);
            }
            Command::Save => self.save(ctx),
            Command::SaveAs(f) => self.save_as_dialog(ctx, f, None),
            Command::SaveProject => self.save_as_dialog(ctx, Some(SaveFormat::Ora), None),
            Command::Print | Command::PageSetup | Command::PrintPreview => self.open_print(cmd),
            Command::Share => self.share(ctx),
            Command::SetWallpaper(mode) => self.set_wallpaper(ctx, mode),
            Command::ImageProperties => self.open_properties(),
            Command::Exit => self.begin_exit(),
            Command::Undo => {
                if self.active.is_idle() {
                    let vector = self.doc.vector_mode();
                    self.history.undo(&mut self.doc);
                    self.after_history_step(ctx, vector);
                } else {
                    // Undo first discards the operation in progress, like Paint.
                    self.cancel_interaction();
                }
            }
            Command::Redo => {
                self.commit_interaction();
                let vector = self.doc.vector_mode();
                self.history.redo(&mut self.doc);
                self.after_history_step(ctx, vector);
            }
            Command::Cut => self.cut(),
            Command::Copy => self.copy(),
            Command::Paste => self.paste(),
            Command::CopyVisible => self.copy_visible(),
            Command::SelectAll => self.select_all(),
            Command::InvertSelection => self.invert_selection(),
            Command::DeleteSelection => self.delete_selection(),
            Command::ToggleTransparentSelection => self.set_transparent_selection(!self.tools.transparent_selection),
            Command::SetSelectMode(m) => {
                self.tools.select_mode = m;
                self.set_tool(Tool::Select);
            }
            Command::Crop => self.crop_to_selection(),
            Command::ResizeSkew => self.open_resize(),
            Command::Rotate(turn) => self.rotate(turn),
            Command::RotateCustom => self.open_rotate_custom(),
            Command::FlipHorizontal => self.flip(true),
            Command::FlipVertical => self.flip(false),
            Command::InvertColors => self.invert_colors(),
            Command::SetTool(tool) => self.set_tool(tool),
            Command::SetBrush(b) => {
                self.tools.brush = b;
                self.set_tool(Tool::Brush);
            }
            Command::SetShape(s) => {
                // Picking a tool (even another shape) finishes the shape being edited.
                self.commit_interaction();
                self.tools.shape = s;
                self.tools.vector_pointer = false;
                self.set_tool(Tool::Shape);
            }
            Command::Pointer => {
                if self.doc.vector_mode() {
                    self.execute(ctx, Command::SetVectorPointer);
                } else {
                    self.execute(ctx, Command::SetTool(Tool::Select));
                }
            }
            Command::SetVectorPointer => {
                self.commit_interaction();
                self.hold_vector_pointer();
            }
            Command::SetOutline(s) => {
                self.tools.outline = s;
                self.on_shape_style_changed();
            }
            Command::SetFill(s) => {
                self.tools.fill = s;
                self.on_shape_style_changed();
            }
            Command::GrowSize(up) => self.grow_size(up),
            Command::ZoomIn => {
                let z = self.view.next_preset(true);
                self.view.zoom_center(z);
            }
            Command::ZoomOut => {
                let z = self.view.next_preset(false);
                self.view.zoom_center(z);
            }
            Command::Zoom100 => self.view.zoom_center(1.0),
            Command::ZoomFit => self.view.request_fit(),
            Command::SetZoom(z) => self.view.zoom_center(z),
            Command::ToggleRulers => {
                self.settings.show_rulers = !self.settings.show_rulers;
                self.settings.save();
            }
            Command::ToggleGrid => {
                self.settings.show_grid = !self.settings.show_grid;
                self.settings.save();
            }
            Command::ToggleStatusBar => {
                self.settings.show_status_bar = !self.settings.show_status_bar;
                self.settings.save();
            }
            Command::SetAutoHideToolbar(v) => {
                self.settings.auto_hide_toolbar = v;
                self.ui.ribbon_collapsed = false;
                self.ui.ribbon_revealed = false;
                self.settings.save();
            }
            Command::ToggleFullscreen => {
                self.ui.fullscreen = !self.ui.fullscreen;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.ui.fullscreen));
            }
            Command::ToggleThumbnail => self.ui.thumbnail = !self.ui.thumbnail,
            Command::ToggleVectorMode => {
                self.layer_op("vector mode", |d| {
                    let on = !d.vector_mode();
                    d.set_vector_mode(on)
                });
                self.remember_vector_mode();
            }
            Command::ToggleLayersPanel => {
                self.settings.show_layers = !self.settings.show_layers;
                self.settings.save();
            }
            Command::OpenSettings => {
                self.commit_interaction();
                self.ui.open_menu = None;
                self.ui.modal = Some(crate::ui::dialogs::Modal::Settings);
            }
            Command::CheckForUpdates => {
                self.commit_interaction();
                self.ui.open_menu = None;
                self.updater.check(ctx, true);
                self.ui.modal = Some(crate::ui::dialogs::Modal::Settings);
            }
            Command::EditColors => self.open_color_dialog(crate::ui::color_target::ColorTarget::Active),
            Command::AddLayer => self.layer_op("add layer", |d| {
                d.add_layer();
                true
            }),
            Command::DuplicateLayer(i) => {
                let i = self.layer_index(i);
                self.layer_op("duplicate layer", |d| {
                    d.duplicate_layer(i);
                    true
                })
            }
            Command::MergeDown(i) => {
                let i = self.layer_index(i);
                self.layer_op("merge down", |d| d.merge_down(i))
            }
            Command::MoveLayer(i, up) => {
                let i = self.layer_index(i);
                self.layer_op("move layer", |d| d.move_layer(i, up))
            }
            Command::ReorderLayer(from, to) => self.layer_op("move layer", |d| d.move_layer_to(from, to)),
            Command::DeleteLayer(i) => {
                let i = self.layer_index(i);
                self.layer_op("delete layer", |d| d.delete_layer(i))
            }
            Command::ToggleLayerVisible(i) => {
                let i = self.layer_index(i);
                let visible = self.doc.layers().get(i).is_some_and(|l| l.visible);
                self.layer_op("layer visibility", |d| {
                    d.set_layer_visible(i, !visible);
                    true
                })
            }
            Command::SelectLayer(i) => {
                self.commit_interaction();
                self.doc.set_active(i);
            }
            Command::ToggleBackground => {
                let mut bg = self.doc.background();
                bg.visible = !bg.visible;
                self.layer_op("background visibility", |d| {
                    d.set_background(bg);
                    true
                })
            }
            Command::ChangeBackgroundColor => self.open_color_dialog(crate::ui::color_target::ColorTarget::Background),
            Command::Escape => self.escape(),
        }
    }

    fn layer_index(&self, i: usize) -> usize {
        if i == usize::MAX { self.doc.active_index() } else { i }
    }

    /// After an undo or redo: when it turned vector mode back on (undoing "Vector" off brings the
    /// objects back), the Pointer is picked so they can be grabbed right away.
    /// Only a step that changed vector mode is remembered: a document opened in vector mode from
    /// a file differs from the remembered mode without the user picking it.
    fn after_history_step(&mut self, ctx: &egui::Context, vector_before: bool) {
        if vector_before == self.doc.vector_mode() {
            return;
        }
        if self.doc.vector_mode() {
            self.execute(ctx, Command::SetVectorPointer);
        }
        self.remember_vector_mode();
    }

    /// Saves the document's vector mode as the one new documents and the next start use, when
    /// it changed (turned on or off, or by undoing or redoing that).
    fn remember_vector_mode(&mut self) {
        if self.settings.vector_mode != self.doc.vector_mode() {
            self.settings.vector_mode = self.doc.vector_mode();
            self.settings.save();
        }
    }

    /// Runs a structural layer operation with undo support.
    fn layer_op(&mut self, label: &'static str, f: impl FnOnce(&mut bgpaint_core::document::Document) -> bool) {
        self.commit_interaction();
        let before = self.doc.snapshot();
        if f(&mut self.doc) {
            self.history.push(label, vec![Change::document(before, &self.doc)]);
        }
    }

    pub fn set_tool(&mut self, tool: Tool) {
        if self.tool != tool {
            self.commit_interaction();
        }
        if tool == Tool::Picker && self.tool != Tool::Picker {
            self.previous_tool = self.tool;
        }
        self.tool = tool;
    }

    fn grow_size(&mut self, up: bool) {
        if let Some((size, opacity)) = self.picked_text() {
            let size = if up { (size + 1.0).min(MAX_FONT_SIZE) } else { (size - 1.0).max(1.0) };
            self.set_picked_text(size, opacity);
            return;
        }
        if self.tool == Tool::Text {
            let s = &mut self.text_settings;
            s.size_pt = if up { (s.size_pt + 1.0).min(MAX_FONT_SIZE) } else { (s.size_pt - 1.0).max(1.0) };
            self.restyle_text();
            self.remember_text_font();
            return;
        }
        if let Some(s) = self.tools.stroke_mut(self.tool) {
            s.size = if up { (s.size + 1.0).min(MAX_SIZE) } else { (s.size - 1.0).max(1.0) };
            if self.tool == Tool::Shape {
                self.on_shape_style_changed();
            }
        }
    }

    fn escape(&mut self) {
        if self.ui.open_menu.is_some() {
            self.ui.open_menu = None;
        } else if !self.active.is_idle() {
            // Escape finishes a shape, text box or selection (it deselects), as in Paint. A
            // finished shape hands over to the vector Pointer.
            let shape = matches!(self.active, crate::tools::interaction::Active::Shape(_));
            self.commit_interaction();
            if shape && self.doc.vector_mode() {
                self.tools.vector_pointer = true;
            }
        } else if self.ui.fullscreen {
            self.ui.fullscreen = false;
        }
    }

    /// Applies a whole-image transform to every layer (or the selection, when there is one).
    /// `t` gives the affine map `f` applies to a `w`×`h` image, which vector objects follow.
    pub fn transform_image(
        &mut self,
        label: &'static str,
        t: impl FnOnce(u32, u32) -> bgpaint_core::tiny_skia::Transform,
        f: impl Fn(&bgpaint_core::tiny_skia::Pixmap) -> bgpaint_core::tiny_skia::Pixmap,
    ) {
        if self.transform_selection(&f) {
            return;
        }
        self.commit_interaction();
        let before = self.doc.snapshot();
        let t = t(self.doc.width(), self.doc.height());
        self.doc.map_layers_affine(t, &f);
        self.history.push(label, vec![Change::document(before, &self.doc)]);
    }

    pub fn rotate(&mut self, turn: QuarterTurn) {
        self.reorient("rotate", raster::Reorient::Rotate(turn));
    }

    pub fn flip(&mut self, horizontal: bool) {
        let op = if horizontal { raster::Reorient::FlipHorizontal } else { raster::Reorient::FlipVertical };
        self.reorient("flip", op);
    }

    /// Flips or turns the selection, or else the whole image (undone by the inverse operation,
    /// with no copy of the pixels).
    fn reorient(&mut self, label: &'static str, op: raster::Reorient) {
        let f = |p: &bgpaint_core::tiny_skia::Pixmap| {
            let mut out = p.clone();
            op.apply(&mut out);
            out
        };
        if self.transform_selection(&f) {
            return;
        }
        self.commit_interaction();
        let objects = self.doc.reorient(op);
        self.history.push(label, vec![Change::reorient(op, objects)]);
    }

    pub fn invert_colors(&mut self) {
        let f = |p: &bgpaint_core::tiny_skia::Pixmap| {
            let mut out = p.clone();
            raster::invert_colors(&mut out);
            out
        };
        if self.transform_selection(&f) {
            return;
        }
        self.commit_interaction();
        let change = self.doc.edit_active(self.doc.bounds(), raster::invert_colors);
        self.history.push_one("invert colors", change);
    }

    /// Called when Color 1/Color 2 change so a shape or text being edited follows them.
    pub fn on_colors_changed(&mut self) {
        if let Some(crate::app::Swatch::Palette(i)) = self.colors.selected
            && bgpaint_core::color::Color::from_hex(crate::ui::ribbon::PALETTE[i]) != self.colors.active()
        {
            self.colors.selected = None;
        }
        self.on_shape_style_changed();
    }
}
