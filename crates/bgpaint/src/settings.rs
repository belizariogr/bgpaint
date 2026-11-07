//! Persistent user preferences, stored as JSON in the platform config directory.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeMode {
    Light,
    Dark,
    #[default]
    System,
}

/// What the ribbon's Rotate button does: the last quarter turn picked from its menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RotateDefault {
    #[default]
    Right,
    Left,
    Half,
}

/// What the ribbon's Flip button does: the last flip picked from its menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FlipDefault {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeMode,
    pub recent_files: Vec<PathBuf>,
    /// The ten "custom colors" slots of the palette, as `#RRGGBB`.
    pub custom_colors: Vec<String>,
    /// The 24 "custom colors" of the Edit colors dialog.
    pub dialog_custom_colors: Vec<String>,
    pub show_rulers: bool,
    pub show_grid: bool,
    pub show_status_bar: bool,
    /// The Layers panel stays as the user last left it.
    pub show_layers: bool,
    /// Vector mode as last picked: new and opened documents start with it.
    pub vector_mode: bool,
    pub auto_hide_toolbar: bool,
    pub window_size: Option<(f32, f32)>,
    pub maximized: bool,
    pub last_dir: Option<PathBuf>,
    /// Look for a new release at startup (and install it, see [`crate::update`]).
    pub check_updates: bool,
    /// The interface language picked in Settings (a code from [`crate::i18n::LANGUAGES`]);
    /// `None` follows the system.
    pub language: Option<String>,
    pub rotate_default: RotateDefault,
    pub flip_default: FlipDefault,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            recent_files: Vec::new(),
            custom_colors: Vec::new(),
            dialog_custom_colors: Vec::new(),
            show_rulers: false,
            show_grid: false,
            show_status_bar: true,
            show_layers: false,
            vector_mode: false,
            auto_hide_toolbar: false,
            window_size: None,
            maximized: false,
            last_dir: None,
            check_updates: true,
            language: None,
            rotate_default: RotateDefault::default(),
            flip_default: FlipDefault::default(),
        }
    }
}

pub const MAX_RECENT: usize = 10;

impl Settings {
    fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("bgpaint").join("settings.json"))
    }

    pub fn load() -> Self {
        Self::path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = bgpaint_core::io::write_atomic(&path, &json);
        }
    }

    pub fn add_recent(&mut self, path: PathBuf) {
        self.recent_files.retain(|p| p != &path);
        self.recent_files.insert(0, path);
        self.recent_files.truncate(MAX_RECENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_files_are_unique_and_bounded() {
        let mut s = Settings::default();
        for i in 0..15 {
            s.add_recent(PathBuf::from(format!("/tmp/{i}.png")));
        }
        s.add_recent(PathBuf::from("/tmp/10.png"));
        assert_eq!(s.recent_files.len(), MAX_RECENT);
        assert_eq!(s.recent_files[0], PathBuf::from("/tmp/10.png"));
        assert_eq!(s.recent_files.iter().filter(|p| p.ends_with("10.png")).count(), 1);
    }

    #[test]
    fn unknown_fields_fall_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"show_grid": true, "bogus": 1}"#).unwrap();
        assert!(s.show_grid);
        assert!(s.show_status_bar);
    }
}
