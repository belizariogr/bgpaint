//! Actions that hand the image to other programs: sharing and the desktop background.

use crate::app::PaintApp;
use crate::commands::WallpaperMode;
use crate::platform;
use bgpaint_core::io::{self, SaveFormat};

impl PaintApp {
    /// Writes the flattened image as PNG into the app data folder for other programs to use.
    fn export_png(&mut self, name: &str) -> Option<std::path::PathBuf> {
        self.commit_interaction();
        let path = platform::data_dir().join(name);
        let bytes = io::encode(&self.doc.flatten(), SaveFormat::Png).ok()?;
        io::write_atomic(&path, &bytes).ok()?;
        Some(path)
    }

    pub fn share(&mut self, ctx: &egui::Context) {
        let file = match &self.file.path {
            Some(p) if !self.modified() && p.exists() => Some(p.clone()),
            _ => {
                let stem = self
                    .file
                    .path
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| self.tr.untitled.to_owned());
                self.export_png(&format!("{stem}.png"))
            }
        };
        let failed = self.tr.share_failed;
        match file {
            // The mail client may take a while (or run until it is closed): do not block the UI.
            Some(f) => self.run_in_background(ctx, move || (!platform::share_file(&f)).then(|| failed.to_owned())),
            None => self.show_toast(failed),
        }
    }

    pub fn set_wallpaper(&mut self, ctx: &egui::Context, mode: WallpaperMode) {
        // A unique name makes desktops that cache by path pick up the new image; the previous
        // ones are no longer needed.
        if let Ok(entries) = std::fs::read_dir(platform::data_dir()) {
            for e in entries.flatten() {
                let name = e.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("wallpaper-") && name.ends_with(".png") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let Some(file) = self.export_png(&format!("wallpaper-{stamp}.png")) else {
            self.show_toast(self.tr.wallpaper_failed);
            return;
        };
        let (done, failed) = (self.tr.wallpaper_done, self.tr.wallpaper_failed);
        self.run_in_background(ctx, move || Some(if platform::set_wallpaper(&file, mode) { done } else { failed }.to_owned()));
    }
}
