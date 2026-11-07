//! Several documents open at once, one per tab.
//!
//! The active document lives directly in [`PaintApp`] (`doc`, `history`, `file`, `view`, …) so
//! tools and dialogs simply work on `self`; the other tabs are parked in `PaintApp::tabs`.
//! `PaintApp::current_tab` is the position of the active document in the tab strip, so the full
//! order is `tabs[..current_tab]`, the active document, then `tabs[current_tab..]`.

use crate::app::PaintApp;
use crate::canvas::{tiles::Tiles, view::View};
use crate::files::{FileState, Pending};
use crate::i18n::Strings;
use crate::ui::layers::Thumbnails;
use bgpaint_core::document::Document;
use bgpaint_core::geom::DirtyRect;
use bgpaint_core::history::History;
use std::path::Path;

/// A document that is not the active one.
pub struct DocTab {
    pub doc: Document,
    pub history: History,
    pub file: FileState,
    view: View,
    tiles: Tiles,
    thumbs: Thumbnails,
    preview_dirty: DirtyRect,
}

impl DocTab {
    pub fn modified(&self) -> bool {
        self.history.state_id() != self.file.saved_state
    }

    pub fn name(&self, tr: &Strings) -> String {
        self.file.display_name(tr)
    }
}

/// What the tab strip needs to draw one tab.
pub struct TabInfo {
    pub name: String,
    pub modified: bool,
    pub path: Option<std::path::PathBuf>,
}

impl PaintApp {
    pub fn tab_count(&self) -> usize {
        self.tabs.len() + 1
    }

    /// Name, modified flag and path of every tab, in strip order.
    pub fn tab_infos(&self) -> Vec<TabInfo> {
        let mut v: Vec<TabInfo> =
            self.tabs.iter().map(|t| TabInfo { name: t.name(self.tr), modified: t.modified(), path: t.file.path.clone() }).collect();
        let current = TabInfo { name: self.file.display_name(self.tr), modified: self.modified(), path: self.file.path.clone() };
        v.insert(self.current_tab, current);
        v
    }

    fn any_parked_modified(&self) -> Option<usize> {
        let p = self.tabs.iter().position(DocTab::modified)?;
        Some(if p < self.current_tab { p } else { p + 1 })
    }

    pub fn any_modified(&self) -> bool {
        self.modified() || self.any_parked_modified().is_some()
    }

    fn take_current(&mut self) -> DocTab {
        let placeholder = Document::new(1, 1).expect("1x1 is a valid size");
        DocTab {
            doc: std::mem::replace(&mut self.doc, placeholder),
            history: std::mem::take(&mut self.history),
            file: std::mem::take(&mut self.file),
            view: std::mem::take(&mut self.view),
            tiles: std::mem::take(&mut self.tiles),
            thumbs: std::mem::take(&mut self.thumbs),
            preview_dirty: std::mem::take(&mut self.preview_dirty),
        }
    }

    fn load(&mut self, t: DocTab) {
        self.doc = t.doc;
        self.history = t.history;
        self.file = t.file;
        self.view = t.view;
        self.tiles = t.tiles;
        self.thumbs = t.thumbs;
        self.preview_dirty = t.preview_dirty;
        self.status = Default::default();
    }

    /// Makes tab `i` (strip position) the active one.
    pub fn switch_tab(&mut self, i: usize) {
        if i == self.current_tab || i >= self.tab_count() {
            return;
        }
        self.commit_interaction();
        self.ui.open_menu = None;
        let current = self.take_current();
        self.tabs.insert(self.current_tab, current);
        let next = self.tabs.remove(i);
        self.load(next);
        self.current_tab = i;
    }

    /// Activates the next (or previous) tab, wrapping around.
    pub fn cycle_tab(&mut self, forward: bool) {
        let n = self.tab_count();
        let i = if forward { (self.current_tab + 1) % n } else { (self.current_tab + n - 1) % n };
        self.switch_tab(i);
    }

    /// Moves the active tab to strip position `to` (drag to reorder). The order of the other
    /// tabs does not change, so only the active tab's position moves.
    pub fn move_current_tab(&mut self, to: usize) {
        self.current_tab = to.min(self.tabs.len());
    }

    /// Strip position of the tab showing `path`, if it is open.
    fn tab_with_path(&self, path: &Path) -> Option<usize> {
        let same = |p: &Option<std::path::PathBuf>| p.as_deref().is_some_and(|p| p == path);
        if same(&self.file.path) {
            return Some(self.current_tab);
        }
        let p = self.tabs.iter().position(|t| same(&t.file.path))?;
        Some(if p < self.current_tab { p } else { p + 1 })
    }

    /// An untouched, never saved document (the blank canvas at startup) that a newly opened file
    /// can replace instead of getting its own tab.
    fn current_is_pristine(&self) -> bool {
        self.file.path.is_none() && !self.history.can_undo() && !self.history.can_redo() && self.active.is_idle()
    }

    /// Opens `doc` in a new tab after the last one and activates it.
    pub fn add_tab(&mut self, doc: Document, file: FileState) {
        self.commit_interaction();
        let current = self.take_current();
        self.tabs.insert(self.current_tab, current);
        self.current_tab = self.tabs.len();
        self.set_document(doc);
        self.file = FileState { saved_state: self.history.state_id(), ..file };
    }

    /// Shows an opened file: switches to its tab when already open, reuses a pristine blank
    /// document, or adds a tab.
    pub fn show_document(&mut self, doc: Document, file: FileState) {
        if let Some(i) = file.path.as_deref().and_then(|p| self.tab_with_path(p)) {
            self.switch_tab(i);
            return;
        }
        if self.current_is_pristine() {
            self.set_document(doc);
            self.file = FileState { saved_state: self.history.state_id(), ..file };
        } else {
            self.add_tab(doc, file);
        }
    }

    /// Whether `path` is already open in some tab (no need to read it again).
    pub fn focus_open_path(&mut self, path: &Path) -> bool {
        match self.tab_with_path(path) {
            Some(i) => {
                self.switch_tab(i);
                true
            }
            None => false,
        }
    }

    /// Closes tab `i`, asking to save it first when it has changes.
    pub fn close_tab(&mut self, i: usize) {
        self.switch_tab(i);
        self.confirm_unsaved(Pending::CloseTab);
    }

    /// Drops the active document (already confirmed). Closing the last tab closes the window.
    pub fn close_current_tab(&mut self) {
        self.cancel_interaction();
        if self.tabs.is_empty() {
            self.file.saved_state = self.history.state_id();
            self.allow_close = true;
            return;
        }
        let next = self.current_tab.min(self.tabs.len() - 1);
        let t = self.tabs.remove(next);
        self.load(t);
        self.current_tab = next;
    }

    /// Asks about every tab with unsaved changes in turn, then closes the window.
    pub fn request_exit(&mut self) {
        self.commit_interaction();
        if !self.modified()
            && let Some(i) = self.any_parked_modified()
        {
            self.switch_tab(i);
        }
        if self.modified() {
            self.ui.modal = Some(crate::ui::dialogs::Modal::ConfirmSave(Pending::Exit));
        } else {
            self.allow_close = true;
        }
    }
}
