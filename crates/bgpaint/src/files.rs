//! New/open/save, native file dialogs (run off the UI thread) and the "save changes?" flow.

use crate::app::PaintApp;
use crate::i18n::Strings;
use crate::ui::dialogs::Modal;
use bgpaint_core::document::{DEFAULT_HEIGHT, DEFAULT_WIDTH, Document};
use bgpaint_core::io::{self, OPEN_EXTENSIONS, SaveFormat};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

/// Where the current document lives on disk.
#[derive(Default, Clone, Debug)]
pub struct FileState {
    pub path: Option<PathBuf>,
    /// Format used by plain "Save".
    pub format: Option<SaveFormat>,
    /// History state at the last save/open.
    pub saved_state: u64,
    pub disk_size: Option<u64>,
}

impl FileState {
    pub fn display_name(&self, tr: &Strings) -> String {
        self.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| tr.untitled.to_owned())
    }
}

/// An action waiting for the user to resolve unsaved changes.
#[derive(Clone, Debug, PartialEq)]
pub enum Pending {
    CloseTab,
    Exit,
}

/// Results of dialogs running on a background thread.
pub enum DialogResult {
    Open(Vec<PathBuf>),
    /// Import into the tab with this id.
    Import(u64, PathBuf),
    /// Save the tab with this id, then run the action.
    SaveAs(u64, PathBuf, SaveFormat, Option<Pending>),
    /// A background job finished; show this message.
    Toast(String),
}

pub struct Async {
    tx: Sender<DialogResult>,
    rx: Receiver<DialogResult>,
    /// A dialog is currently shown (avoid opening two).
    pub busy: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Files to open or import, held back while a dialog is shown so they do not change the
    /// document under it.
    queue: std::collections::VecDeque<Queued>,
}

enum Queued {
    Open(PathBuf),
    Import(u64, PathBuf),
}

impl Default for Async {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self { tx, rx, busy: Default::default(), queue: Default::default() }
    }
}

/// Runs a file dialog on a worker thread. Returns false when another dialog is already shown.
fn spawn_dialog(app: &PaintApp, ctx: &egui::Context, job: impl FnOnce() -> Option<DialogResult> + Send + 'static) -> bool {
    use std::sync::atomic::Ordering;
    if app.files.busy.swap(true, Ordering::SeqCst) {
        return false;
    }
    let tx = app.files.tx.clone();
    let busy = app.files.busy.clone();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        if let Some(r) = job() {
            let _ = tx.send(r);
        }
        busy.store(false, Ordering::SeqCst);
        ctx.request_repaint();
    });
    true
}

/// The absolute form of a path, so one file is recognized however it was named.
fn absolute(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).or_else(|_| std::path::absolute(path)).unwrap_or_else(|_| path.to_path_buf())
}

impl PaintApp {
    fn start_dir(&self) -> Option<PathBuf> {
        self.file
            .path
            .as_ref()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .or_else(|| self.settings.last_dir.clone())
            .or_else(dirs::picture_dir)
    }

    pub fn poll_async(&mut self, ctx: &egui::Context) {
        while let Ok(r) = self.files.rx.try_recv() {
            match r {
                DialogResult::Open(paths) => self.files.queue.extend(paths.into_iter().map(Queued::Open)),
                DialogResult::Import(tab, p) => self.files.queue.push_back(Queued::Import(tab, p)),
                DialogResult::Toast(text) => self.show_toast(text),
                DialogResult::SaveAs(tab, p, f, then) => {
                    // The user may have switched tabs while the dialog was up: save the tab
                    // that asked (and drop the result if it was closed meanwhile).
                    let Some(i) = self.tab_index(tab) else { continue };
                    self.switch_tab(i);
                    if self.save_to(&p, f)
                        && let Some(then) = then
                    {
                        self.run_pending(then);
                    }
                }
            }
        }
        while self.ui.modal.is_none()
            && let Some(q) = self.files.queue.pop_front()
        {
            match q {
                Queued::Open(p) => self.open_path(ctx, p),
                Queued::Import(tab, p) => {
                    if let Some(i) = self.tab_index(tab) {
                        self.switch_tab(i);
                        self.import_path(p);
                    }
                }
            }
        }
    }

    /// Asks to save if needed, then performs `action`.
    pub fn confirm_unsaved(&mut self, action: Pending) {
        self.commit_interaction();
        if self.modified() {
            self.ui.modal = Some(Modal::ConfirmSave(action));
        } else {
            self.ui.modal = None;
            self.run_pending(action);
        }
    }

    pub fn run_pending(&mut self, action: Pending) {
        match action {
            Pending::CloseTab => self.close_current_tab(),
            Pending::Exit => {
                // This tab was saved or its changes discarded; go on with the next one.
                self.exit_discarded.insert(self.tab_id);
                self.request_exit();
            }
        }
    }

    /// Resolves the "save changes?" dialog.
    pub fn resolve_unsaved(&mut self, ctx: &egui::Context, action: Pending, save: bool) {
        self.ui.modal = None;
        if save {
            match (&self.file.path, self.file.format) {
                (Some(p), Some(f)) => {
                    let p = p.clone();
                    if self.save_to(&p, f) {
                        self.run_pending(action);
                    }
                }
                _ => self.save_as_dialog(ctx, Some(SaveFormat::Png), Some(action)),
            }
        } else {
            self.run_pending(action);
        }
    }

    pub fn open_dialog(&mut self, ctx: &egui::Context) {
        let tr = self.tr;
        let dir = self.start_dir();
        spawn_dialog(self, ctx, move || {
            let mut d = rfd::AsyncFileDialog::new().add_filter(tr.all_images, OPEN_EXTENSIONS).add_filter("*", &["*"]);
            if let Some(dir) = dir {
                d = d.set_directory(dir);
            }
            let files = pollster::block_on(d.pick_files())?;
            Some(DialogResult::Open(files.iter().map(|f| f.path().to_path_buf()).collect()))
        });
    }

    /// "New": a blank canvas in a new tab.
    pub fn new_tab(&mut self) {
        let doc = Document::new(DEFAULT_WIDTH, DEFAULT_HEIGHT).expect("default canvas size is valid");
        self.add_tab(doc, FileState::default());
    }

    pub fn import_dialog(&mut self, ctx: &egui::Context) {
        let tr = self.tr;
        let dir = self.start_dir();
        let tab = self.tab_id;
        spawn_dialog(self, ctx, move || {
            let mut d = rfd::AsyncFileDialog::new().add_filter(tr.all_images, OPEN_EXTENSIONS);
            if let Some(dir) = dir {
                d = d.set_directory(dir);
            }
            pollster::block_on(d.pick_file()).map(|f| DialogResult::Import(tab, f.path().to_path_buf()))
        });
    }

    /// Shows the save dialog. `format` preselects a type (None offers all).
    pub fn save_as_dialog(&mut self, ctx: &egui::Context, format: Option<SaveFormat>, then: Option<Pending>) {
        self.commit_interaction();
        let tr = self.tr;
        let dir = self.start_dir();
        let stem = self
            .file
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| tr.untitled.to_owned());
        let tab = self.tab_id;
        let retry = then.clone();
        let shown = spawn_dialog(self, ctx, move || {
            let formats: Vec<SaveFormat> = match format {
                Some(f) => vec![f],
                None => SaveFormat::IMAGE_FORMATS.to_vec(),
            };
            let first = formats[0];
            let mut d = rfd::AsyncFileDialog::new().set_file_name(format!("{stem}.{}", first.extensions()[0]));
            for f in &formats {
                let name =
                    if *f == SaveFormat::Ora { tr.paint_project.to_owned() } else { format!("{} (*.{})", f.name(), f.extensions()[0]) };
                d = d.add_filter(name, f.extensions());
            }
            if let Some(dir) = dir {
                d = d.set_directory(dir);
            }
            let mut path = pollster::block_on(d.save_file())?.path().to_path_buf();
            let chosen = SaveFormat::from_path(&path).filter(|f| formats.contains(f));
            let fmt = match chosen {
                Some(f) => f,
                None => {
                    path.set_extension(first.extensions()[0]);
                    first
                }
            };
            Some(DialogResult::SaveAs(tab, path, fmt, then))
        });
        if !shown && let Some(action) = retry {
            // Another file dialog is still open: keep asking instead of dropping the action.
            self.ui.modal = Some(Modal::ConfirmSave(action));
        }
    }

    /// Plain "Save": writes back to the current file or asks for a name.
    pub fn save(&mut self, ctx: &egui::Context) {
        self.commit_interaction();
        match (&self.file.path, self.file.format) {
            (Some(p), Some(f)) => {
                let p = p.clone();
                self.save_to(&p, f);
            }
            _ => self.save_as_dialog(ctx, Some(SaveFormat::Png), None),
        }
    }

    /// Writes the document; returns false (and shows an error) on failure.
    pub fn save_to(&mut self, path: &Path, format: SaveFormat) -> bool {
        match io::save(&self.doc, path, format) {
            Ok(size) => {
                let path = &absolute(path);
                self.detach_other_tabs_from(path);
                self.file.saved_state = self.history.state_id();
                self.file.disk_size = Some(size);
                // Saving as a project keeps the image path for plain saves only if it is a project.
                self.file.path = Some(path.to_path_buf());
                self.file.format = Some(format);
                self.settings.last_dir = path.parent().map(Path::to_path_buf);
                self.settings.add_recent(path.to_path_buf());
                self.settings.save();
                true
            }
            Err(e) => {
                self.ui.modal = Some(Modal::Error(format!("{}\n{e}", self.tr.save_failed)));
                false
            }
        }
    }

    /// Opens a file in its own tab (or switches to it when it is already open).
    pub fn open_path(&mut self, _ctx: &egui::Context, path: PathBuf) {
        let path = absolute(&path);
        if self.focus_open_path(&path) {
            return;
        }
        match io::open(&path) {
            Ok(opened) => {
                let size = std::fs::metadata(&path).ok().map(|m| m.len());
                let file = FileState { path: Some(path.clone()), format: opened.save_format, saved_state: 0, disk_size: size };
                self.show_document(opened.document, file);
                self.settings.last_dir = path.parent().map(Path::to_path_buf);
                self.settings.add_recent(path);
                self.settings.save();
            }
            Err(e) => {
                self.settings.recent_files.retain(|p| p != &path);
                self.ui.modal = Some(Modal::Error(format!("{}\n{e}", self.tr.open_failed)));
            }
        }
    }

    /// "Import to canvas → From a file": pastes the image as a floating selection.
    pub fn import_path(&mut self, path: PathBuf) {
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
        match std::fs::read(&path).map_err(io::IoError::from).and_then(|b| io::decode(&b, &ext)) {
            Ok(pixmap) => self.paste_pixmap(pixmap),
            Err(e) => self.ui.modal = Some(Modal::Error(format!("{}\n{e}", self.tr.open_failed))),
        }
    }

    pub fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        self.files.queue.extend(dropped.into_iter().map(Queued::Open));
    }
}

impl PaintApp {
    /// Runs `job` on a worker thread and shows its message (if any) when it finishes.
    pub fn run_in_background(&self, ctx: &egui::Context, job: impl FnOnce() -> Option<String> + Send + 'static) {
        let tx = self.files.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            if let Some(msg) = job() {
                let _ = tx.send(DialogResult::Toast(msg));
                ctx.request_repaint();
            }
        });
    }
}
