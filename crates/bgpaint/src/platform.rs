//! Integration with the desktop: opening URLs/files with the default application.

use std::process::Command;

/// Opens a URL or file with the system's default handler.
pub fn open_url(url: &str) {
    let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let _ = Command::new(program).arg(url).spawn();
}
