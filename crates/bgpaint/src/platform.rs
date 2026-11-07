//! Integration with the desktop: URLs, printing (CUPS), sharing and the desktop background.

use crate::commands::WallpaperMode;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Opens a URL or file with the system's default handler.
pub fn open_url(url: &str) {
    let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let _ = Command::new(program).arg(url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

fn run(program: &str, args: &[&str]) -> bool {
    Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn has(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|d| d.join(program).is_file()))
}

/// A directory for files handed to other programs (print jobs, shared images, wallpapers).
pub fn data_dir() -> PathBuf {
    let dir = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("bgpaint");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

// ----- Printing -----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paper {
    A4,
    Letter,
    Legal,
    A3,
    A5,
}

impl Paper {
    pub const ALL: [Paper; 5] = [Paper::A4, Paper::Letter, Paper::Legal, Paper::A3, Paper::A5];

    /// Size in millimetres (portrait).
    pub fn size_mm(self) -> (f32, f32) {
        match self {
            Paper::A4 => (210.0, 297.0),
            Paper::Letter => (215.9, 279.4),
            Paper::Legal => (215.9, 355.6),
            Paper::A3 => (297.0, 420.0),
            Paper::A5 => (148.0, 210.0),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Paper::A4 => "A4",
            Paper::Letter => "Letter",
            Paper::Legal => "Legal",
            Paper::A3 => "A3",
            Paper::A5 => "A5",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrintSettings {
    pub printer: Option<String>,
    pub copies: u32,
    pub orientation: Orientation,
    pub paper: Paper,
    pub fit: bool,
    pub center: bool,
    pub margin_mm: f32,
}

impl Default for PrintSettings {
    fn default() -> Self {
        Self { printer: None, copies: 1, orientation: Orientation::Portrait, paper: Paper::A4, fit: true, center: true, margin_mm: 19.0 }
    }
}

/// Printers known to CUPS and the default one.
pub fn printers() -> (Vec<String>, Option<String>) {
    let list =
        output("lpstat", &["-e"]).map(|s| s.lines().map(|l| l.trim().to_owned()).filter(|l| !l.is_empty()).collect()).unwrap_or_default();
    let default = output("lpstat", &["-d"]).and_then(|s| s.split(':').nth(1).map(|d| d.trim().to_owned())).filter(|d| !d.is_empty());
    (list, default)
}

/// The `lp` arguments for a job.
pub fn lp_args(s: &PrintSettings, file: &Path) -> Vec<String> {
    let mut a = Vec::new();
    if let Some(p) = &s.printer {
        a.extend(["-d".to_owned(), p.clone()]);
    }
    a.extend(["-n".to_owned(), s.copies.max(1).to_string()]);
    a.extend(["-o".to_owned(), format!("media={}", s.paper.name())]);
    if s.orientation == Orientation::Landscape {
        a.extend(["-o".to_owned(), "landscape".to_owned()]);
    }
    a.extend(["-o".to_owned(), if s.fit { "fit-to-page".to_owned() } else { "ppi=96".to_owned() }]);
    if s.center {
        a.extend(["-o".to_owned(), "position=center".to_owned()]);
    }
    let pts = (s.margin_mm / 25.4 * 72.0).round() as i32;
    for side in ["page-left", "page-right", "page-top", "page-bottom"] {
        a.extend(["-o".to_owned(), format!("{side}={pts}")]);
    }
    a.extend(["-t".to_owned(), "BGPaint".to_owned(), file.display().to_string()]);
    a
}

/// Sends a PNG to the printer; returns an error message on failure.
pub fn print_png(png: &[u8], s: &PrintSettings) -> Result<(), String> {
    if !has("lp") {
        return Err("lp (CUPS) is not installed".into());
    }
    // A file per job, so a quick second print does not overwrite one CUPS may still be reading;
    // jobs older than an hour are cleaned up.
    let dir = data_dir();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let old =
            |e: &std::fs::DirEntry| e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|d| d.as_secs() > 3600));
        for e in entries.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("print-job-") && old(e)) {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let file = dir.join(format!("print-job-{stamp}.png"));
    std::fs::write(&file, png).map_err(|e| e.to_string())?;
    let args = lp_args(s, &file);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = Command::new("lp").args(&refs).stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_owned()) }
}

// ----- Sharing -----

/// Hands an image file to the e-mail client (Linux `xdg-email`, macOS Mail).
pub fn share_file(file: &Path) -> bool {
    let f = file.display().to_string();
    if cfg!(target_os = "macos") {
        run("open", &["-a", "Mail", &f])
    } else if has("xdg-email") {
        run("xdg-email", &["--attach", &f])
    } else {
        false
    }
}

// ----- Desktop background -----

/// A `file://` URI with the path percent-encoded (spaces, non-ASCII characters…).
fn file_uri(path: &str) -> String {
    let mut uri = String::from("file://");
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri
}

/// Sets the desktop background on the current desktop environment.
pub fn set_wallpaper(file: &Path, mode: WallpaperMode) -> bool {
    let path = file.display().to_string();
    if cfg!(target_os = "macos") {
        let script = format!(
            "tell application \"System Events\" to tell every desktop to set picture to POSIX file \"{}\"",
            path.replace('"', "\\\"")
        );
        return run("osascript", &["-e", &script]);
    }
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    let uri = file_uri(&path);
    if desktop.contains("kde") && has("plasma-apply-wallpaperimage") {
        let fill = match mode {
            WallpaperMode::Fill => "preserveAspectCrop",
            WallpaperMode::Tile => "tile",
            WallpaperMode::Center => "pad",
        };
        return run("plasma-apply-wallpaperimage", &["--fill-mode", fill, &path]) || run("plasma-apply-wallpaperimage", &[&path]);
    }
    if desktop.contains("xfce") && has("xfconf-query") {
        let style = match mode {
            WallpaperMode::Fill => "5",
            WallpaperMode::Tile => "2",
            WallpaperMode::Center => "1",
        };
        let props = output("xfconf-query", &["-c", "xfce4-desktop", "-l"]).unwrap_or_default();
        let mut ok = false;
        for p in props.lines().filter(|p| p.ends_with("/last-image")) {
            ok |= run("xfconf-query", &["-c", "xfce4-desktop", "-p", p, "-s", &path]);
            let style_prop = p.replace("/last-image", "/image-style");
            run("xfconf-query", &["-c", "xfce4-desktop", "-p", &style_prop, "-s", style]);
        }
        return ok;
    }
    if desktop.contains("hyprland") || std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        if has("swww") && run("swww", &["img", &path]) {
            return true;
        }
        if has("hyprctl")
            && run("hyprctl", &["hyprpaper", "preload", &path])
            && run("hyprctl", &["hyprpaper", "wallpaper", &format!(",{path}")])
        {
            // Free the images of earlier wallpapers.
            run("hyprctl", &["hyprpaper", "unload", "unused"]);
            return true;
        }
        if has("swaybg") {
            let m = match mode {
                WallpaperMode::Fill => "fill",
                WallpaperMode::Tile => "tile",
                WallpaperMode::Center => "center",
            };
            let _ = Command::new("pkill").arg("-x").arg("swaybg").status();
            return Command::new("setsid")
                .args(["swaybg", "-i", &path, "-m", m])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .is_ok();
        }
    }
    if desktop.contains("sway") && has("swaymsg") {
        let m = match mode {
            WallpaperMode::Fill => "fill",
            WallpaperMode::Tile => "tile",
            WallpaperMode::Center => "center",
        };
        return run("swaymsg", &["output", "*", "bg", &path, m]);
    }
    if has("gsettings") {
        let schema = if desktop.contains("cinnamon") { "org.cinnamon.desktop.background" } else { "org.gnome.desktop.background" };
        let option = match mode {
            WallpaperMode::Fill => "zoom",
            WallpaperMode::Tile => "wallpaper",
            WallpaperMode::Center => "centered",
        };
        let ok = run("gsettings", &["set", schema, "picture-uri", &uri]);
        run("gsettings", &["set", schema, "picture-uri-dark", &uri]);
        run("gsettings", &["set", schema, "picture-options", option]);
        if ok {
            return true;
        }
    }
    if has("feh") {
        let m = match mode {
            WallpaperMode::Fill => "--bg-fill",
            WallpaperMode::Tile => "--bg-tile",
            WallpaperMode::Center => "--bg-center",
        };
        return run("feh", &[m, &path]);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lp_arguments() {
        let s =
            PrintSettings { printer: Some("Office".into()), copies: 2, orientation: Orientation::Landscape, ..PrintSettings::default() };
        let a = lp_args(&s, Path::new("/tmp/x.png"));
        let joined = a.join(" ");
        assert!(joined.starts_with("-d Office -n 2 -o media=A4 -o landscape -o fit-to-page -o position=center"));
        assert!(joined.contains("page-left=54"));
        assert!(joined.ends_with("/tmp/x.png"));
    }

    #[test]
    fn file_uris_are_encoded() {
        assert_eq!(file_uri("/home/a b/ç.png"), "file:///home/a%20b/%C3%A7.png");
    }

    #[test]
    fn paper_sizes() {
        assert_eq!(Paper::A4.size_mm(), (210.0, 297.0));
        assert_eq!(Paper::ALL.len(), 5);
    }
}
