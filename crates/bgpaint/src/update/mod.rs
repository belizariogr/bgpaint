//! Updates from GitHub Releases.
//!
//! A worker thread asks the GitHub API for the latest release (at startup, and from Settings).
//! When it is newer than this build, the package that matches how this copy was installed is
//! downloaded, checked against the SHA-256 digest GitHub publishes for it, and installed by
//! `install.sh` (embedded):
//!
//! | Install                                   | Package                    | Privileges            |
//! |-------------------------------------------|----------------------------|-----------------------|
//! | `~/.local/bgpaint.app` (`install.sh`)     | `bgpaint-linux-<arch>.tar.gz` | none               |
//! | `/usr`, `/usr/local` (`install-linux-system`) | same tarball           | root                  |
//! | AppImage                                  | `.AppImage`                | only if not writable  |
//! | `.deb`                                    | `.deb` (apt/dpkg)          | root                  |
//! | `.rpm`                                    | `.rpm` (dnf/zypper/yum/rpm) | root                 |
//! | pacman `.pkg.tar.zst`                     | `.pkg.tar.zst` (pacman)    | root                  |
//! | macOS `BGPaint.app`                       | `.dmg` for the CPU         | only if not writable  |
//!
//! Installs that need no privileges are updated silently; the others ask first, then request
//! the password through polkit (`pkexec`), falling back to `sudo` with a password typed into
//! BGPaint when no polkit agent is running, or through the macOS administrator prompt.
//! Copies owned by a package other than this project's `bgpaint` (an AUR build under another
//! name, say) or built from source only report the new version. The new version runs after BGPaint restarts.

use crate::platform::{data_dir, has, output};
use serde::Deserialize;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{self, AtomicU64};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

const REPO: &str = "belizariogr/bgpaint";
/// The page of the release being offered (release notes, manual downloads).
pub const RELEASES_URL: &str = "https://github.com/belizariogr/bgpaint/releases/latest";
const SCRIPT: &str = include_str!("install.sh");
/// Printed by `install.sh` once it runs: authorization succeeded.
const STARTED: &str = "bgpaint-update: start";

// ----- Versions -----

/// A semantic version (`1.2.3`, `1.2.3-beta.1`); build metadata is ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    core: [u64; 3],
    pre: Vec<String>,
}

impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
        let s = s.split('+').next()?;
        let (core, pre) = match s.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (s, None),
        };
        let mut parts = core.split('.');
        let mut n = [0; 3];
        for slot in &mut n {
            *slot = parts.next()?.parse().ok()?;
        }
        if parts.next().is_some() {
            return None;
        }
        let pre = match pre {
            None => Vec::new(),
            Some(p) if !p.is_empty() && p.split('.').all(|id| !id.is_empty()) => p.split('.').map(str::to_owned).collect(),
            Some(_) => return None,
        };
        Some(Self { core: n, pre })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            // A prerelease comes before its release.
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (a, b) in self.pre.iter().zip(&other.pre) {
                    let c = match (a.parse::<u64>(), b.parse::<u64>()) {
                        (Ok(x), Ok(y)) => x.cmp(&y),
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        _ => a.cmp(b),
                    };
                    if c != Ordering::Equal {
                        return c;
                    }
                }
                self.pre.len().cmp(&other.pre.len())
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// ----- Releases -----

#[derive(Clone, Debug, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Clone, Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    /// `sha256:<hex>`, computed by GitHub when the file was uploaded.
    #[serde(default)]
    digest: Option<String>,
}

fn user_agent() -> String {
    format!("BGPaint/{} (update check)", env!("CARGO_PKG_VERSION"))
}

fn latest_release() -> Result<Release, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let ua = user_agent();
    let accept = "Accept: application/vnd.github+json";
    let mut cmd = if has("curl") {
        let mut c = Command::new("curl");
        c.args(["-fsSL", "--connect-timeout", "15", "--max-time", "60", "-A", &ua, "-H", accept, &url]);
        c
    } else if has("wget") {
        let mut c = Command::new("wget");
        c.args(["-q", "-T", "30", "-O", "-", "-U", &ua, &format!("--header={accept}"), &url]);
        c
    } else {
        return Err("curl or wget is needed to check for updates".into());
    };
    let out = cmd.stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        return Err(if err.is_empty() { "the release could not be fetched".into() } else { err });
    }
    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
}

// ----- How this copy was installed -----

/// How the running copy was installed, which decides what replaces it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// A macOS app bundle, replaced with the one in the release's `.dmg`.
    MacApp(PathBuf),
    /// An AppImage file.
    AppImage(PathBuf),
    /// The `bgpaint.app` folder that `script/install.sh` unpacks into ~/.local.
    Bundle(PathBuf),
    /// `<prefix>/bin/bgpaint` and friends, from `script/install-linux-system`.
    Prefix(PathBuf),
    /// Installed from the release's `.deb` (the binary's path).
    Deb(PathBuf),
    /// Installed from the release's `.rpm` (the binary's path).
    Rpm(PathBuf),
    /// Installed from the release's pacman `.pkg.tar.zst` (the binary's path).
    Pacman(PathBuf),
    /// Owned by another package manager, which has to update it.
    Managed(&'static str),
    /// Built from source, or a layout this module does not know.
    Unsupported,
}

/// Works out the install from the executable's path alone (`appimage`: the AppImage this
/// executable runs from).
fn classify(exe: &Path, appimage: Option<&Path>, macos: bool) -> Install {
    let name = |p: Option<&Path>| p.and_then(Path::file_name).and_then(|n| n.to_str()).unwrap_or("").to_owned();
    if macos {
        // …/BGPaint.app/Contents/MacOS/bgpaint
        let macos_dir = exe.parent();
        let contents = macos_dir.and_then(Path::parent);
        let Some(app) = contents.and_then(Path::parent) else { return Install::Unsupported };
        let readonly = app.starts_with("/Volumes") || app.components().any(|c| c.as_os_str() == "AppTranslocation");
        return if name(macos_dir) == "MacOS" && name(contents) == "Contents" && name(Some(app)).ends_with(".app") && !readonly {
            Install::MacApp(app.to_path_buf())
        } else {
            Install::Unsupported
        };
    }
    if let Some(file) = appimage {
        return Install::AppImage(file.to_path_buf());
    }
    let bin = exe.parent();
    let Some(root) = bin.and_then(Path::parent) else { return Install::Unsupported };
    if name(bin) != "bin" || name(Some(exe)) != "bgpaint" {
        return Install::Unsupported;
    }
    if name(Some(root)) == "bgpaint.app" {
        Install::Bundle(root.to_path_buf())
    } else if root == Path::new("/usr") || root == Path::new("/usr/local") {
        Install::Prefix(root.to_path_buf())
    } else {
        Install::Unsupported
    }
}

fn succeeds(program: &str, args: &[&str]) -> bool {
    has(program) && output(program, args).is_some()
}

/// The package that owns a file, as `program args…` prints it (`None`: not owned, or no such
/// package manager).
fn owner(program: &str, args: &[&str]) -> Option<String> {
    if !has(program) {
        return None;
    }
    output(program, args).map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

/// The install of the running copy. Asks the package managers about system paths, so it
/// runs on a worker thread.
pub fn detect() -> Install {
    let Ok(exe) = std::env::current_exe() else { return Install::Unsupported };
    let exe = exe.canonicalize().unwrap_or(exe);
    // The AppImage runtime sets these for everything started from the AppImage too, so they
    // count only when this executable lives inside the mounted image.
    let appdir = std::env::var_os("APPDIR").map(PathBuf::from);
    let appimage = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|_| appdir.as_ref().is_some_and(|d| !d.as_os_str().is_empty() && exe.starts_with(d)));
    match classify(&exe, appimage.as_deref(), cfg!(target_os = "macos")) {
        Install::Prefix(prefix) => {
            let path = exe.to_string_lossy();
            if succeeds("dpkg-query", &["-S", &path]) {
                Install::Deb(exe)
            } else if let Some(owner) = owner("pacman", &["-Qqo", &path]) {
                // Only this project's package is replaced by the release's.
                if owner == "bgpaint" { Install::Pacman(exe) } else { Install::Managed("pacman") }
            } else if let Some(owner) = owner("rpm", &["-qf", "--queryformat", "%{NAME}", &path]) {
                if owner == "bgpaint" { Install::Rpm(exe) } else { Install::Managed("rpm") }
            } else {
                Install::Prefix(prefix)
            }
        }
        other => other,
    }
}

/// The release file that updates `install` on this CPU (`arch` as in `std::env::consts::ARCH`).
fn asset_for<'a>(install: &Install, assets: &'a [Asset], arch: &str) -> Option<&'a Asset> {
    let deb_arch = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };
    let wanted = |name: &str| match install {
        Install::MacApp(_) => name.starts_with("BGPaint_") && name.ends_with(&format!("_{arch}.dmg")),
        Install::AppImage(_) => name.ends_with(&format!("_{arch}.AppImage")),
        Install::Bundle(_) | Install::Prefix(_) => name == format!("bgpaint-linux-{arch}.tar.gz"),
        Install::Deb(_) => name.starts_with("bgpaint_") && name.ends_with(&format!("_{deb_arch}.deb")),
        // bgpaint-<version>-<release>.<arch>.rpm
        Install::Rpm(_) => name.starts_with("bgpaint-") && name.ends_with(&format!(".{arch}.rpm")),
        // bgpaint-<version>-<release>-<arch>.pkg.tar.zst
        Install::Pacman(_) => name.starts_with("bgpaint-") && name.ends_with(&format!("-{arch}.pkg.tar.zst")),
        Install::Managed(_) | Install::Unsupported => false,
    };
    assets.iter().find(|a| wanted(&a.name))
}

fn is_root() -> bool {
    output("id", &["-u"]).is_some_and(|s| s.trim() == "0")
}

/// Whether this user can create files in `dir`.
fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".bgpaint-write-test-{}", std::process::id()));
    let ok = std::fs::File::create(&probe).is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Whether installing over `install` needs root.
fn needs_root(install: &Install) -> bool {
    match install {
        Install::Prefix(_) | Install::Deb(_) | Install::Rpm(_) | Install::Pacman(_) => !is_root(),
        // The folder is renamed and later deleted, so its contents must be ours too.
        Install::MacApp(p) | Install::Bundle(p) => !(p.parent().is_some_and(writable) && writable(p)),
        Install::AppImage(p) => !p.parent().is_some_and(writable),
        Install::Managed(_) | Install::Unsupported => false,
    }
}

// ----- Download -----

fn updates_dir() -> PathBuf {
    let dir = data_dir().join("updates");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn sha256(file: &Path) -> Option<String> {
    let path = file.to_str()?;
    let out = if has("sha256sum") { output("sha256sum", &[path]) } else { output("shasum", &["-a", "256", path]) }?;
    out.split_whitespace().next().map(str::to_ascii_lowercase)
}

/// Whether `file` matches the digest GitHub lists (true when it lists none).
fn digest_matches(file: &Path, digest: Option<&str>) -> Result<bool, String> {
    let Some(expected) = digest.and_then(|d| d.strip_prefix("sha256:")) else { return Ok(true) };
    let actual = sha256(file).ok_or("sha256sum or shasum is needed to verify the download")?;
    Ok(actual.eq_ignore_ascii_case(expected))
}

/// Downloads `url` into `dest`, storing the bytes received so far in `progress`.
fn download(url: &str, dest: &Path, progress: &AtomicU64, tick: &dyn Fn()) -> Result<(), String> {
    let part = PathBuf::from(format!("{}.part", dest.display()));
    let ua = user_agent();
    let mut cmd = if has("curl") {
        let mut c = Command::new("curl");
        c.args(["-fsSL", "--retry", "2", "--connect-timeout", "20", "-A", &ua, "-o"]).arg(&part).arg(url);
        c
    } else if has("wget") {
        let mut c = Command::new("wget");
        c.args(["-q", "-T", "30", "-U", &ua, "-O"]).arg(&part).arg(url);
        c
    } else {
        return Err("curl or wget is needed to download the update".into());
    };
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        progress.store(std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0), atomic::Ordering::Relaxed);
        tick();
        std::thread::sleep(Duration::from_millis(200));
    };
    if !status.success() {
        let mut err = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = std::io::Read::read_to_string(&mut stderr, &mut err);
        }
        let _ = std::fs::remove_file(&part);
        let err = err.trim();
        return Err(if err.is_empty() { format!("could not download {url}") } else { err.to_owned() });
    }
    std::fs::rename(&part, dest).map_err(|e| e.to_string())
}

// ----- Installing -----

/// A downloaded update, ready to install.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub version: String,
    install: Install,
    file: PathBuf,
}

/// How `install.sh` gets its privileges.
enum Elevation {
    None,
    /// `pkexec`: the desktop's polkit agent asks for the password.
    Polkit,
    /// `sudo`, with the password typed into BGPaint.
    Sudo(String),
    /// macOS's administrator prompt (`do shell script … with administrator privileges`).
    MacAdmin(&'static str),
}

enum Outcome {
    Done,
    /// The user dismissed the password prompt.
    Cancelled,
    /// polkit could not ask (no `pkexec`, or no agent running).
    NoAgent,
    WrongPassword,
    Failed(String),
}

fn script_args(p: &Prepared) -> Option<Vec<String>> {
    let s = |p: &Path| p.to_string_lossy().into_owned();
    let package = s(&p.file);
    Some(match &p.install {
        Install::MacApp(app) => vec!["macapp".into(), package, s(app)],
        Install::AppImage(file) => vec!["appimage".into(), package, s(file)],
        Install::Bundle(app) => vec!["bundle".into(), package, s(app)],
        Install::Prefix(prefix) => vec!["prefix".into(), package, s(prefix)],
        Install::Deb(_) => vec!["deb".into(), package],
        Install::Rpm(_) => vec!["rpm".into(), package],
        Install::Pacman(_) => vec!["pacman".into(), package],
        Install::Managed(_) | Install::Unsupported => return None,
    })
}

/// `s` quoted for a POSIX shell.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// `s` as an AppleScript string literal.
fn applescript_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', r"\\").replace('"', "\\\""))
}

fn run_with_input(cmd: &mut Command, input: Option<&str>) -> std::io::Result<std::process::Output> {
    let mut child =
        cmd.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        let _ = std::io::Write::write_all(&mut stdin, format!("{input}\n").as_bytes());
    }
    child.wait_with_output()
}

fn failure(out: &std::process::Output) -> Outcome {
    let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    Outcome::Failed(if err.is_empty() { format!("the installer exited with {}", out.status) } else { err })
}

fn run_install(p: &Prepared, elevation: Elevation) -> Outcome {
    let Some(args) = script_args(p) else { return Outcome::Failed("this copy cannot be updated".into()) };
    let script = updates_dir().join("install-update.sh");
    if let Err(e) = std::fs::write(&script, SCRIPT) {
        return Outcome::Failed(e.to_string());
    }
    let script_path = script.to_string_lossy().into_owned();
    let mut sh_args = vec![script_path.clone()];
    sh_args.extend(args.iter().cloned());
    let result = match elevation {
        Elevation::None => run_with_input(Command::new("/bin/sh").args(&sh_args), None),
        Elevation::Polkit => {
            if !has("pkexec") {
                return Outcome::NoAgent;
            }
            match run_with_input(Command::new("pkexec").arg("/bin/sh").args(&sh_args), None) {
                Ok(out) if !String::from_utf8_lossy(&out.stdout).contains(STARTED) => {
                    // pkexec exits with 126 when the user dismisses the prompt and with 127
                    // when it cannot authorize (no agent, for one).
                    return if out.status.code() == Some(126) { Outcome::Cancelled } else { Outcome::NoAgent };
                }
                other => other,
            }
        }
        Elevation::Sudo(password) => {
            if !has("sudo") {
                return Outcome::Failed("sudo is not installed".into());
            }
            // Check the password first, so a wrong one is not reported as a failed install.
            let valid = run_with_input(Command::new("sudo").args(["-S", "-k", "-v", "-p", ""]), Some(&password));
            if !valid.is_ok_and(|o| o.status.success()) {
                return Outcome::WrongPassword;
            }
            run_with_input(Command::new("sudo").args(["-S", "-k", "-p", "", "--", "/bin/sh"]).args(&sh_args), Some(&password))
        }
        Elevation::MacAdmin(prompt) => {
            let command = std::iter::once("/bin/sh".to_owned()).chain(sh_args.iter().map(|a| shell_quote(a))).collect::<Vec<_>>().join(" ");
            let source = format!(
                "do shell script {} with prompt {} with administrator privileges",
                applescript_string(&command),
                applescript_string(prompt)
            );
            match run_with_input(Command::new("osascript").args(["-e", &source]), None) {
                // "User canceled. (-128)"
                Ok(out) if !out.status.success() && String::from_utf8_lossy(&out.stderr).contains("-128") => return Outcome::Cancelled,
                other => other,
            }
        }
    };
    match result {
        Ok(out) if out.status.success() => {
            let _ = std::fs::remove_dir_all(updates_dir());
            Outcome::Done
        }
        Ok(out) => failure(&out),
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

// ----- State shared with the UI -----

/// Where the updater is. Workers send the next stage over a channel.
pub enum Stage {
    Idle,
    Checking,
    /// The last (manual) check found nothing newer.
    UpToDate,
    CheckFailed(String),
    Downloading {
        version: String,
        received: Arc<AtomicU64>,
        total: u64,
    },
    /// Downloaded; installing it needs the administrator password, so the user is asked first.
    Ready(Prepared),
    /// polkit could not ask for the password: BGPaint asks for it and runs `sudo`.
    Password {
        prepared: Prepared,
        password: String,
        wrong: bool,
        focus: bool,
    },
    /// `interactive`: the user started it (and may be looking at a password prompt).
    Installing {
        version: String,
        interactive: bool,
    },
    Installed {
        version: String,
        install: Install,
    },
    /// A newer version that this copy cannot install (`manager`: who installed it).
    Available {
        version: String,
        manager: Option<&'static str>,
    },
    Failed(String),
}

pub struct Updater {
    pub stage: Stage,
    /// The dialog for the current stage was closed.
    dismissed: bool,
    /// Start the new version once the window closes.
    pub relaunch: bool,
    tx: Sender<(Stage, bool)>,
    rx: Receiver<(Stage, bool)>,
}

impl Default for Updater {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self { stage: Stage::Idle, dismissed: false, relaunch: false, tx, rx }
    }
}

/// Whether to check at startup: not in builds from source (debug), automated UI tests, or
/// with `BGPAINT_NO_UPDATE` set.
pub fn automatic_checks_allowed() -> bool {
    !cfg!(debug_assertions) && !cfg!(feature = "automation") && std::env::var_os("BGPAINT_NO_UPDATE").is_none()
}

impl Updater {
    /// A worker is running.
    pub fn busy(&self) -> bool {
        matches!(self.stage, Stage::Checking | Stage::Downloading { .. } | Stage::Installing { .. })
    }

    /// Runs `job` on a worker thread; it reports stages through `send(stage, show_dialog)`.
    fn spawn(&self, ctx: &egui::Context, job: impl FnOnce(&dyn Fn(Stage, bool), &dyn Fn()) + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let send = |stage: Stage, show: bool| {
                let _ = tx.send((stage, show));
                ctx.request_repaint();
            };
            let tick = || ctx.request_repaint();
            job(&send, &tick);
        });
    }

    /// Looks for a newer release, then downloads it and installs it (silently when that
    /// needs no password). `manual`: asked for from Settings, so every result is reported.
    pub fn check(&mut self, ctx: &egui::Context, manual: bool) {
        match self.stage {
            _ if self.busy() => return,
            // Already found: show it again.
            Stage::Ready(_) | Stage::Password { .. } | Stage::Installed { .. } | Stage::Available { .. } if manual => {
                self.dismissed = false;
                return;
            }
            Stage::Installed { .. } => return,
            _ => {}
        }
        self.stage = Stage::Checking;
        self.dismissed = false;
        self.spawn(ctx, move |send, tick| {
            let idle = |send: &dyn Fn(Stage, bool), err: Option<String>| match err {
                Some(e) if manual => send(Stage::CheckFailed(e), false),
                None if manual => send(Stage::UpToDate, false),
                _ => send(Stage::Idle, false),
            };
            let install = detect();
            if install == Install::Unsupported && !manual {
                return idle(send, None);
            }
            let release = match latest_release() {
                Ok(r) => r,
                Err(e) => return idle(send, Some(e)),
            };
            let (Some(current), Some(latest)) = (Version::parse(env!("CARGO_PKG_VERSION")), Version::parse(&release.tag_name)) else {
                return idle(send, Some(format!("unexpected version: {}", release.tag_name)));
            };
            if latest <= current {
                return idle(send, None);
            }
            let version = release.tag_name.trim_start_matches(['v', 'V']).to_owned();
            let asset = asset_for(&install, &release.assets, std::env::consts::ARCH);
            let (Some(asset), false) = (asset, matches!(install, Install::Managed(_) | Install::Unsupported)) else {
                let manager = if let Install::Managed(m) = install { Some(m) } else { None };
                // A copy that cannot update itself is not worth a dialog at every start.
                return send(Stage::Available { version, manager }, manual);
            };

            // Earlier downloads are dropped, except this very file when it is complete.
            let dir = updates_dir();
            let file = dir.join(&asset.name);
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries.flatten().filter(|e| e.path() != file) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
            let complete = asset.digest.is_some() && file.is_file() && digest_matches(&file, asset.digest.as_deref()) == Ok(true);
            if !complete {
                let received = Arc::new(AtomicU64::new(0));
                send(Stage::Downloading { version: version.clone(), received: received.clone(), total: asset.size }, false);
                if let Err(e) = download(&asset.browser_download_url, &file, &received, tick) {
                    return send(Stage::Failed(e), manual);
                }
                match digest_matches(&file, asset.digest.as_deref()) {
                    Ok(true) => {}
                    Ok(false) => {
                        let _ = std::fs::remove_file(&file);
                        return send(Stage::Failed(format!("{} is damaged (its SHA-256 does not match the release)", asset.name)), true);
                    }
                    Err(e) => return send(Stage::Failed(e), true),
                }
            }

            let prepared = Prepared { version: version.clone(), install: install.clone(), file };
            if needs_root(&install) {
                return send(Stage::Ready(prepared), true);
            }
            send(Stage::Installing { version: version.clone(), interactive: false }, false);
            match run_install(&prepared, Elevation::None) {
                Outcome::Done => send(Stage::Installed { version, install }, true),
                Outcome::Failed(e) => send(Stage::Failed(e), true),
                _ => send(Stage::Failed("unexpected installer result".into()), true),
            }
        });
    }

    /// Installs a downloaded update that needs privileges. `admin_prompt` is the reason shown
    /// in the macOS password prompt.
    pub fn install(&mut self, ctx: &egui::Context, admin_prompt: &'static str) {
        let (prepared, elevation) = match std::mem::replace(&mut self.stage, Stage::Idle) {
            Stage::Ready(p) if cfg!(target_os = "macos") => (p, Elevation::MacAdmin(admin_prompt)),
            Stage::Ready(p) => (p, Elevation::Polkit),
            Stage::Password { prepared, password, .. } => (prepared, Elevation::Sudo(password)),
            other => {
                self.stage = other;
                return;
            }
        };
        self.stage = Stage::Installing { version: prepared.version.clone(), interactive: true };
        self.dismissed = false;
        self.spawn(ctx, move |send, _| {
            match run_install(&prepared, elevation) {
                Outcome::Done => send(Stage::Installed { version: prepared.version.clone(), install: prepared.install.clone() }, true),
                // Asked again only when the user wants (Settings).
                Outcome::Cancelled => send(Stage::Ready(prepared), false),
                Outcome::NoAgent => send(Stage::Password { prepared, password: String::new(), wrong: false, focus: true }, true),
                Outcome::WrongPassword => send(Stage::Password { prepared, password: String::new(), wrong: true, focus: true }, true),
                Outcome::Failed(e) => send(Stage::Failed(e), true),
            }
        });
    }

    /// Takes the stages workers sent.
    pub fn poll(&mut self) {
        while let Ok((stage, show)) = self.rx.try_recv() {
            self.stage = stage;
            self.dismissed = !show;
        }
    }

    /// Whether the update dialog should be shown.
    pub fn wants_dialog(&self) -> bool {
        !self.dismissed
            && matches!(
                self.stage,
                Stage::Ready(_)
                    | Stage::Password { .. }
                    | Stage::Installing { interactive: true, .. }
                    | Stage::Installed { .. }
                    | Stage::Available { .. }
                    | Stage::Failed(_)
            )
    }

    pub fn dismiss(&mut self) {
        self.dismissed = true;
    }

    /// Leaves the password prompt for the "ready to install" stage.
    pub fn cancel_password(&mut self) {
        if let Stage::Password { .. } = self.stage
            && let Stage::Password { prepared, .. } = std::mem::replace(&mut self.stage, Stage::Idle)
        {
            self.stage = Stage::Ready(prepared);
        }
        self.dismissed = true;
    }

    /// Starts the installed version (after this one's window closed), opening `files`.
    pub fn relaunch(&self, files: &[PathBuf]) {
        use std::os::unix::process::CommandExt;
        let Stage::Installed { install, .. } = &self.stage else { return };
        let mut cmd = match install {
            Install::MacApp(app) => {
                let mut c = Command::new("open");
                c.arg("-n").arg(app);
                if !files.is_empty() {
                    c.arg("--args");
                }
                c
            }
            Install::AppImage(file) => Command::new(file),
            Install::Bundle(app) => Command::new(app.join("bin/bgpaint")),
            Install::Prefix(prefix) => Command::new(prefix.join("bin/bgpaint")),
            Install::Deb(exe) | Install::Rpm(exe) | Install::Pacman(exe) => Command::new(exe),
            Install::Managed(_) | Install::Unsupported => return,
        };
        // Not this AppImage's environment for the new copy.
        if !matches!(install, Install::AppImage(_)) {
            cmd.env_remove("APPIMAGE").env_remove("APPDIR");
        }
        let _ = cmd.args(files).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn versions_parse() {
        assert_eq!(v("v1.2.3"), v("1.2.3"));
        assert_eq!(v("1.2.3+build.5"), v("1.2.3"));
        for bad in ["", "1.2", "1.2.3.4", "1.x.3", "1.2.3-", "1.2.3-a..b"] {
            assert!(Version::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn versions_order() {
        let ordered =
            ["0.2.0", "0.2.1", "0.10.0", "1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-alpha.beta", "1.0.0-beta.2", "1.0.0-beta.11", "1.0.0"];
        for pair in ordered.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
        }
        assert_eq!(v("1.0.0").cmp(&v("v1.0.0")), Ordering::Equal);
    }

    #[test]
    fn installs_are_recognized() {
        let p = Path::new;
        assert_eq!(
            classify(p("/home/u/.local/bgpaint.app/bin/bgpaint"), None, false),
            Install::Bundle(p("/home/u/.local/bgpaint.app").into())
        );
        assert_eq!(classify(p("/usr/bin/bgpaint"), None, false), Install::Prefix(p("/usr").into()));
        assert_eq!(classify(p("/usr/local/bin/bgpaint"), None, false), Install::Prefix(p("/usr/local").into()));
        assert_eq!(
            classify(p("/tmp/.mount_x/usr/bin/bgpaint"), Some(p("/opt/BGPaint.AppImage")), false),
            Install::AppImage(p("/opt/BGPaint.AppImage").into())
        );
        assert_eq!(classify(p("/home/u/src/bgpaint/target/release/bgpaint"), None, false), Install::Unsupported);
        assert_eq!(classify(p("/home/u/.cargo/bin/bgpaint"), None, false), Install::Unsupported);
        assert_eq!(
            classify(p("/Applications/BGPaint.app/Contents/MacOS/bgpaint"), None, true),
            Install::MacApp(p("/Applications/BGPaint.app").into())
        );
        assert_eq!(classify(p("/Volumes/BGPaint/BGPaint.app/Contents/MacOS/bgpaint"), None, true), Install::Unsupported);
        assert_eq!(classify(p("/Users/u/src/bgpaint/target/release/bgpaint"), None, true), Install::Unsupported);
    }

    #[test]
    fn assets_match_the_install() {
        let assets: Vec<Asset> = [
            "bgpaint-linux-x86_64.tar.gz",
            "BGPaint_0.3.0_aarch64.dmg",
            "bgpaint_0.3.0_amd64.deb",
            "bgpaint_0.3.0_x86_64.AppImage",
            "bgpaint_0.3.0_x86_64.tar.gz",
            "bgpaint-0.3.0-1.x86_64.rpm",
            "bgpaint-0.3.0-1-x86_64.pkg.tar.zst",
            "BGPaint_0.3.0_x86_64.dmg",
            "install.sh",
            "PKGBUILD",
        ]
        .iter()
        .map(|n| Asset { name: (*n).into(), browser_download_url: String::new(), size: 0, digest: None })
        .collect();
        let pick = |install: Install, arch: &str| asset_for(&install, &assets, arch).map(|a| a.name.as_str());
        let any = PathBuf::from("/x");
        assert_eq!(pick(Install::Bundle(any.clone()), "x86_64"), Some("bgpaint-linux-x86_64.tar.gz"));
        assert_eq!(pick(Install::Prefix(any.clone()), "x86_64"), Some("bgpaint-linux-x86_64.tar.gz"));
        assert_eq!(pick(Install::Bundle(any.clone()), "aarch64"), None);
        assert_eq!(pick(Install::Deb(any.clone()), "x86_64"), Some("bgpaint_0.3.0_amd64.deb"));
        assert_eq!(pick(Install::AppImage(any.clone()), "x86_64"), Some("bgpaint_0.3.0_x86_64.AppImage"));
        assert_eq!(pick(Install::MacApp(any.clone()), "aarch64"), Some("BGPaint_0.3.0_aarch64.dmg"));
        assert_eq!(pick(Install::MacApp(any.clone()), "x86_64"), Some("BGPaint_0.3.0_x86_64.dmg"));
        assert_eq!(pick(Install::Rpm(any.clone()), "x86_64"), Some("bgpaint-0.3.0-1.x86_64.rpm"));
        assert_eq!(pick(Install::Rpm(any.clone()), "aarch64"), None);
        assert_eq!(pick(Install::Pacman(any.clone()), "x86_64"), Some("bgpaint-0.3.0-1-x86_64.pkg.tar.zst"));
        assert_eq!(pick(Install::Pacman(any), "aarch64"), None);
        assert_eq!(pick(Install::Managed("pacman"), "x86_64"), None);
    }

    #[test]
    fn releases_parse() {
        let json = r#"{"tag_name":"v0.3.0","html_url":"x","assets":[{"name":"a.deb","browser_download_url":"https://e/a.deb","size":12,"digest":"sha256:ab"}]}"#;
        let r: Release = serde_json::from_str(json).unwrap();
        assert_eq!(r.tag_name, "v0.3.0");
        assert_eq!(r.assets[0].digest.as_deref(), Some("sha256:ab"));
    }

    #[test]
    fn quoting() {
        assert_eq!(shell_quote("/a b/it's"), r"'/a b/it'\''s'");
        assert_eq!(applescript_string(r#"say "hi" \o/"#), r#""say \"hi\" \\o/""#);
    }

    /// The whole path against the published release: `cargo test -p bgpaint -- --ignored`.
    #[test]
    #[ignore = "downloads the latest release from GitHub"]
    fn latest_release_installs_over_a_bundle() {
        let release = latest_release().unwrap();
        assert!(Version::parse(&release.tag_name).is_some(), "{}", release.tag_name);
        let root = std::env::temp_dir().join(format!("bgpaint-update-test-{}", std::process::id()));
        let app = root.join("bgpaint.app");
        std::fs::create_dir_all(app.join("bin")).unwrap();
        std::fs::write(app.join("bin/bgpaint"), "old").unwrap();
        let install = Install::Bundle(app.clone());
        let asset = asset_for(&install, &release.assets, "x86_64").expect("a Linux tarball");
        let file = root.join(&asset.name);
        let received = AtomicU64::new(0);
        download(&asset.browser_download_url, &file, &received, &|| {}).unwrap();
        assert_eq!(digest_matches(&file, asset.digest.as_deref()), Ok(true));
        let prepared = Prepared { version: release.tag_name.clone(), install, file };
        assert!(matches!(run_install(&prepared, Elevation::None), Outcome::Done));
        assert!(std::fs::metadata(app.join("bin/bgpaint")).unwrap().len() > 1_000_000);
        assert!(app.join("share/applications/io.github.belizariogr.bgpaint.desktop").is_file());
        let leftovers: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(leftovers.len(), 2, "{leftovers:?}"); // the app and the download
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn digests_are_checked() {
        let dir = std::env::temp_dir().join(format!("bgpaint-digest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("f");
        std::fs::write(&file, b"abc").unwrap();
        let abc = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(digest_matches(&file, Some(abc)), Ok(true));
        assert_eq!(digest_matches(&file, Some("sha256:00")), Ok(false));
        assert_eq!(digest_matches(&file, None), Ok(true));
        let _ = std::fs::remove_dir_all(dir);
    }
}
