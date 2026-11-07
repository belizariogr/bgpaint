//! Undo/redo. Every change is stored in a form that is its own inverse: applying it swaps the
//! stored data with the document, so the same operation serves undo and redo.

use crate::document::{DocState, Document, LayerId};
use tiny_skia::Pixmap;

/// One reversible modification.
#[derive(Debug)]
pub enum Change {
    /// A rectangular region of one layer.
    Pixels { layer: LayerId, x: i32, y: i32, pixels: Pixmap },
    /// The whole document (layer list, canvas size, background...).
    Document(Box<DocState>),
}

impl Change {
    pub fn document(state: DocState) -> Self {
        Change::Document(Box::new(state))
    }

    fn byte_size(&self) -> usize {
        match self {
            Change::Pixels { pixels, .. } => pixels.data().len(),
            Change::Document(s) => s.byte_size(),
        }
    }

    /// Swaps the stored data with the document (undoes, or redoes, this change).
    pub fn apply(&mut self, doc: &mut Document) {
        match self {
            Change::Pixels { layer, x, y, pixels } => {
                doc.swap_pixels(*layer, *x, *y, pixels);
            }
            Change::Document(state) => {
                let placeholder = DocState { width: 0, height: 0, layers: Vec::new(), active: 0, background: state.background };
                let stored = std::mem::replace(state.as_mut(), placeholder);
                **state = doc.swap_state(stored);
            }
        }
    }
}

#[derive(Debug)]
struct Entry {
    id: u64,
    label: &'static str,
    changes: Vec<Change>,
}

impl Entry {
    /// Current memory cost. Document snapshots get more expensive as the live document diverges
    /// from them (copy-on-write), so this is recomputed rather than cached.
    fn bytes(&self) -> usize {
        self.changes.iter().map(Change::byte_size).sum()
    }
}

/// Linear undo history with a memory budget.
#[derive(Debug)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    next_id: u64,
    bytes: usize,
    budget: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::with_budget(1536 * 1024 * 1024)
    }
}

impl History {
    pub fn with_budget(budget: usize) -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), next_id: 1, bytes: 0, budget }
    }

    /// Records changes that were already applied to the document.
    pub fn push(&mut self, label: &'static str, changes: Vec<Change>) {
        if changes.is_empty() {
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.redo.clear();
        self.undo.push(Entry { id, label, changes });
        self.recount();
        while self.bytes > self.budget && self.undo.len() > 1 {
            let e = self.undo.remove(0);
            self.bytes = self.bytes.saturating_sub(e.bytes());
        }
    }

    /// Records a single change.
    pub fn push_one(&mut self, label: &'static str, change: Option<Change>) {
        if let Some(c) = change {
            self.push(label, vec![c]);
        }
    }

    fn recount(&mut self) {
        self.bytes = self.undo.iter().chain(&self.redo).map(Entry::bytes).sum();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&'static str> {
        self.undo.last().map(|e| e.label)
    }

    pub fn undo(&mut self, doc: &mut Document) -> bool {
        let Some(mut e) = self.undo.pop() else { return false };
        for c in e.changes.iter_mut().rev() {
            c.apply(doc);
        }
        self.redo.push(e);
        true
    }

    pub fn redo(&mut self, doc: &mut Document) -> bool {
        let Some(mut e) = self.redo.pop() else { return false };
        for c in e.changes.iter_mut() {
            c.apply(doc);
        }
        self.undo.push(e);
        true
    }

    /// Identifier of the current state; compare with a saved value to know if the document changed.
    pub fn state_id(&self) -> u64 {
        self.undo.last().map_or(0, |e| e.id)
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::geom::PixelRect;
    use crate::raster;

    #[test]
    fn undo_redo_pixels() {
        let mut doc = Document::new(4, 4).unwrap();
        let mut h = History::default();
        let saved = h.state_id();
        let c = doc.edit_active(PixelRect::new(0, 0, 2, 2), |p| raster::fill_region(p, PixelRect::new(0, 0, 2, 2), Color::BLACK));
        h.push_one("fill", c);
        assert_ne!(h.state_id(), saved);
        assert_eq!(doc.sample(0, 0), Some(Color::BLACK));
        assert!(h.undo(&mut doc));
        assert_eq!(doc.sample(0, 0), Some(Color::WHITE));
        assert_eq!(h.state_id(), saved);
        assert!(h.redo(&mut doc));
        assert_eq!(doc.sample(0, 0), Some(Color::BLACK));
        assert!(!h.redo(&mut doc));
    }

    #[test]
    fn undo_redo_structure() {
        let mut doc = Document::new(4, 4).unwrap();
        let mut h = History::default();
        let before = doc.snapshot();
        doc.resize_canvas(8, 2, 0, 0);
        doc.add_layer();
        h.push("resize", vec![Change::document(before)]);
        assert_eq!((doc.width(), doc.layers().len()), (8, 2));
        h.undo(&mut doc);
        assert_eq!((doc.width(), doc.height(), doc.layers().len()), (4, 4, 1));
        h.redo(&mut doc);
        assert_eq!((doc.width(), doc.layers().len()), (8, 2));
    }

    #[test]
    fn budget_drops_oldest() {
        let mut doc = Document::new(10, 10).unwrap();
        let mut h = History::with_budget(10 * 10 * 4 + 1);
        for _ in 0..3 {
            let c = doc.edit_active(doc.bounds(), raster::invert_colors);
            h.push_one("invert", c);
        }
        assert!(h.undo(&mut doc));
        assert!(!h.undo(&mut doc));
    }
}
