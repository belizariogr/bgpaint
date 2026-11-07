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
    Import(PathBuf),
    SaveAs(PathBuf, SaveFormat, Option<Pending>),
    /// A background job finished; show this message.
    Toast(String),
}

pub struct Async {
    tx: Sender<DialogResult>,
    rx: Receiver<DialogResult>,
    /// A dialog is currently shown (avoid opening two).
    pub busy: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// An action to run at the start of the next frame.
    pub deferred: Option<Pending>,
}

impl Default for Async {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self { tx, rx, busy: Default::default(), deferred: None }
    }
}

fn spawn_dialog(app: &PaintApp, ctx: &egui::Context, job: impl FnOnce() -> Option<DialogResult> + Send + 'static) {
    use std::sync::atomic::Ordering;
    if app.files.busy.swap(true, Ordering::SeqCst) {
        return;
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
                DialogResult::Open(paths) => {
                    for p in paths {
                        self.open_path(ctx, p);
                    }
                }
                DialogResult::Import(p) => self.import_path(ctx, p),
                DialogResult::Toast(text) => self.show_toast(text),
                DialogResult::SaveAs(p, f, then) => {
                    if self.save_to(&p, f)
                        && let Some(then) = then
                    {
                        self.run_pending(ctx, then);
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
            self.pending_now(action);
        }
    }

    fn pending_now(&mut self, action: Pending) {
        // Context-free part; actions that need a dialog are dispatched on the next frame.
        self.files.deferred = Some(action);
    }

    pub fn run_pending(&mut self, _ctx: &egui::Context, action: Pending) {
        match action {
            Pending::CloseTab => self.close_current_tab(),
            Pending::Exit => {
                // This tab was saved or its changes discarded; go on with the next one.
                self.file.saved_state = self.history.state_id();
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
                        self.run_pending(ctx, action);
                    }
                }
                _ => self.save_as_dialog(ctx, Some(SaveFormat::Png), Some(action)),
            }
        } else {
            self.run_pending(ctx, action);
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
        spawn_dialog(self, ctx, move || {
            let mut d = rfd::AsyncFileDialog::new().add_filter(tr.all_images, OPEN_EXTENSIONS);
            if let Some(dir) = dir {
                d = d.set_directory(dir);
            }
            pollster::block_on(d.pick_file()).map(|f| DialogResult::Import(f.path().to_path_buf()))
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
        spawn_dialog(self, ctx, move || {
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
            Some(DialogResult::SaveAs(path, fmt, then))
        });
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
    pub fn import_path(&mut self, _ctx: &egui::Context, path: PathBuf) {
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
        match std::fs::read(&path).map_err(io::IoError::from).and_then(|b| io::decode(&b, &ext)) {
            Ok(pixmap) => self.paste_pixmap(pixmap),
            Err(e) => self.ui.modal = Some(Modal::Error(format!("{}\n{e}", self.tr.open_failed))),
        }
    }

    pub fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        if let Some(deferred) = self.files.deferred.take() {
            self.run_pending(ctx, deferred);
        }
        let dropped: Vec<PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        for path in dropped {
            self.open_path(ctx, path);
        }
    }
}

impl PaintApp {
    /// Runs `job` on a worker thread and shows its message when it finishes.
    pub fn run_in_background(&self, ctx: &egui::Context, job: impl FnOnce() -> String + Send + 'static) {
        let tx = self.files.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(DialogResult::Toast(job()));
            ctx.request_repaint();
        });
    }
}
