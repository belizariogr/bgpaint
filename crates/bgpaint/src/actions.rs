//! Execution of [`Command`]s.

use crate::app::PaintApp;
use crate::commands::Command;
use crate::files::Pending;
use crate::tools::{MAX_SIZE, Tool};
use bgpaint_core::history::Change;
use bgpaint_core::io::SaveFormat;
use bgpaint_core::raster::{self, QuarterTurn};

impl PaintApp {
    pub fn execute(&mut self, ctx: &egui::Context, cmd: Command) {
        match cmd {
            Command::New => self.confirm_unsaved(Pending::New),
            Command::Open => self.confirm_unsaved(Pending::Open),
            Command::OpenRecent(p) => self.confirm_unsaved(Pending::OpenPath(p)),
            Command::ImportFromFile => {
                self.commit_interaction();
                self.import_dialog(ctx);
            }
            Command::Save => self.save(ctx),
            Command::SaveAs(f) => self.save_as_dialog(ctx, f, None),
            Command::SaveProject => self.save_as_dialog(ctx, Some(SaveFormat::Ora), None),
            Command::Print | Command::PageSetup | Command::PrintPreview => self.open_print(cmd),
            Command::Share => self.share(),
            Command::SetWallpaper(mode) => self.set_wallpaper(mode),
            Command::ImageProperties => self.open_properties(),
            Command::Exit => self.confirm_unsaved(Pending::Exit),
            Command::Undo => {
                if self.active.is_idle() {
                    self.history.undo(&mut self.doc);
                } else {
                    // Undo first discards the operation in progress, like Paint.
                    self.cancel_interaction();
                }
            }
            Command::Redo => {
                self.commit_interaction();
                self.history.redo(&mut self.doc);
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
                self.tools.shape = s;
                self.set_tool(Tool::Shape);
            }
            Command::SetOutline(s) => {
                self.tools.outline = s;
                self.on_shape_style_changed();
            }
            Command::SetFill(s) => {
                self.tools.fill = s;
                self.on_shape_style_changed();
            }
            Command::SetSize(w) => {
                self.tools.shape_width = w;
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
            Command::ToggleLayersPanel => self.ui.show_layers = !self.ui.show_layers,
            Command::OpenSettings => {
                self.commit_interaction();
                self.ui.settings_open = true;
            }
            Command::CloseSettings => self.ui.settings_open = false,
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

    /// Runs a structural layer operation with undo support.
    fn layer_op(&mut self, label: &'static str, f: impl FnOnce(&mut bgpaint_core::document::Document) -> bool) {
        self.commit_interaction();
        let before = self.doc.snapshot();
        if f(&mut self.doc) {
            self.history.push(label, vec![Change::document(before)]);
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
        if self.tool == Tool::Shape {
            let widths = crate::tools::SHAPE_WIDTHS;
            let i = widths.iter().position(|w| (*w - self.tools.shape_width).abs() < 0.1).unwrap_or(2);
            let j = if up { (i + 1).min(widths.len() - 1) } else { i.saturating_sub(1) };
            self.tools.shape_width = widths[j];
            self.on_shape_style_changed();
        } else if let Some(s) = self.tools.stroke_mut(self.tool) {
            s.size = if up { (s.size + 1.0).min(MAX_SIZE) } else { (s.size - 1.0).max(1.0) };
        }
    }

    fn escape(&mut self) {
        if self.ui.open_menu.is_some() {
            self.ui.open_menu = None;
        } else if !self.active.is_idle() {
            self.cancel_interaction();
        } else if self.ui.fullscreen {
            self.ui.fullscreen = false;
        }
    }

    /// Applies a whole-image transform to every layer (or the selection, when there is one).
    fn transform_image(&mut self, label: &'static str, f: impl Fn(&bgpaint_core::tiny_skia::Pixmap) -> bgpaint_core::tiny_skia::Pixmap) {
        if self.transform_selection(&f) {
            return;
        }
        self.commit_interaction();
        let before = self.doc.snapshot();
        self.doc.map_layers(&f);
        self.history.push(label, vec![Change::document(before)]);
    }

    pub fn rotate(&mut self, turn: QuarterTurn) {
        self.transform_image("rotate", |p| raster::rotate_quarter(p, turn));
    }

    pub fn flip(&mut self, horizontal: bool) {
        self.transform_image("flip", |p| {
            let mut out = p.clone();
            if horizontal {
                raster::flip_horizontal(&mut out);
            } else {
                raster::flip_vertical(&mut out);
            }
            out
        });
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
        self.on_shape_style_changed();
    }
}
