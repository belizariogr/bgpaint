//! Temporary entry points for features of later stages.

use crate::app::PaintApp;
use crate::commands::{Command, WallpaperMode};
use crate::ui::color_target::ColorTarget;

impl PaintApp {
    pub fn open_print(&mut self, _cmd: Command) {}
    pub fn share(&mut self) {}
    pub fn set_wallpaper(&mut self, _mode: WallpaperMode) {}
    pub fn open_properties(&mut self) {}
    pub fn open_resize(&mut self) {}
    pub fn open_rotate_custom(&mut self) {}
    pub fn open_color_dialog(&mut self, _target: ColorTarget) {}
    pub fn tool_bars(&mut self, _ctx: &egui::Context) {}
}
