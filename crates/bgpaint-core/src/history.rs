//! Undo/redo. Every change is stored in a form that is its own inverse: applying it swaps the
//! stored data with the document, so the same operation serves undo and redo.

use crate::document::{DocState, Document, LayerId};
use crate::patch::Patch;
use crate::raster::Reorient;
use crate::vector::VectorShape;

/// One reversible modification.
#[derive(Debug)]
pub enum Change {
    /// The changed tiles of a region of one layer.
    Pixels { layer: LayerId, patch: Patch },
    /// The vector objects of one layer.
    Objects { layer: LayerId, objects: Vec<VectorShape> },
    /// The whole document (layer list, canvas size, background...).
    Document(Box<Snapshot>),
    /// A flip or quarter turn of the whole image: undone by the inverse operation, so no pixels
    /// are kept. The objects are kept as they were (turning them back could drift).
    Reorient { op: Reorient, objects: Vec<(LayerId, Vec<VectorShape>)> },
}

/// A document state kept by the history. Layers whose pixels are still those of the live layer
/// with the same id do not keep them: undo reaches this step only after undoing every later
/// change, so by then the live layer holds exactly those pixels again. Keeping no reference lets
/// the next edit of that layer change it in place instead of copying it whole.
#[derive(Debug)]
pub struct Snapshot {
    state: DocState,
    /// Per layer: the pixels are taken from the live layer when this state is restored.
    live: Vec<bool>,
}

impl Change {
    /// A structural undo step: `state` is the document before the change, which `doc` now holds.
    pub fn document(mut state: DocState, doc: &Document) -> Self {
        let live = doc.detach_live(&mut state);
        Change::Document(Box::new(Snapshot { state, live }))
    }

    /// The undo step of `doc.reorient(op)`, given what it returned.
    pub fn reorient(op: Reorient, objects: Vec<(LayerId, Vec<VectorShape>)>) -> Self {
        Change::Reorient { op: op.inverse(), objects }
    }

    /// An undo step for the vector objects of `layer`, which were `objects` before the change.
    pub fn objects(layer: LayerId, objects: Vec<VectorShape>) -> Self {
        Change::Objects { layer, objects }
    }

    fn byte_size(&self) -> usize {
        match self {
            Change::Pixels { patch, .. } => patch.byte_size(),
            Change::Objects { objects, .. } => objects.iter().map(VectorShape::unshared_bytes).sum(),
            Change::Document(s) => s.state.byte_size(),
            Change::Reorient { objects, .. } => objects.iter().flat_map(|(_, o)| o).map(VectorShape::unshared_bytes).sum(),
        }
    }

    /// Swaps the stored data with the document (undoes, or redoes, this change).
    pub fn apply(&mut self, doc: &mut Document) {
        match self {
            Change::Pixels { layer, patch } => {
                doc.swap_pixels(*layer, patch);
            }
            Change::Objects { layer, objects } => doc.swap_objects(*layer, objects),
            Change::Reorient { op, objects } => {
                doc.reorient_pixels(*op);
                for (layer, objects) in objects.iter_mut() {
                    doc.swap_objects(*layer, objects);
                }
                *op = op.inverse();
            }
            Change::Document(snapshot) => {
                let placeholder = DocState {
                    width: 0,
                    height: 0,
                    layers: Vec::new(),
                    active: 0,
                    background: snapshot.state.background,
                    vector_mode: false,
                };
                let mut stored = std::mem::replace(&mut snapshot.state, placeholder);
                doc.attach_live(&mut stored, &snapshot.live);
                let mut previous = doc.swap_state(stored);
                snapshot.live = doc.detach_live(&mut previous);
                snapshot.state = previous;
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
        h.push("resize", vec![Change::document(before, &doc)]);
        assert_eq!((doc.width(), doc.layers().len()), (8, 2));
        h.undo(&mut doc);
        assert_eq!((doc.width(), doc.height(), doc.layers().len()), (4, 4, 1));
        h.redo(&mut doc);
        assert_eq!((doc.width(), doc.layers().len()), (8, 2));
    }

    #[test]
    fn budget_drops_oldest() {
        let mut doc = Document::new(10, 10).unwrap();
        // One saved solid tile each: only one step fits.
        let mut h = History::with_budget(50);
        for v in 1..4 {
            let c = doc.edit_active(doc.bounds(), |p| raster::fill_region(p, PixelRect::new(0, 0, 10, 10), Color::rgb(v, v, v)));
            h.push_one("invert", c);
        }
        assert!(h.undo(&mut doc));
        assert!(!h.undo(&mut doc));
    }

    #[test]
    fn edits_after_a_structural_step_do_not_copy_the_layer() {
        let mut doc = Document::new(8, 8).unwrap();
        let mut h = History::default();
        let before = doc.snapshot();
        doc.set_background(crate::document::Background { color: Color::BLACK, visible: true });
        h.push("background", vec![Change::document(before, &doc)]);
        let pixels = std::sync::Arc::as_ptr(&doc.layers()[0].pixels);
        let c = doc.edit_active(doc.bounds(), |p| raster::fill_region(p, PixelRect::new(0, 0, 2, 2), Color::WHITE));
        h.push_one("fill", c);
        // Changed in place: the history kept no reference to the layer.
        assert_eq!(std::sync::Arc::as_ptr(&doc.layers()[0].pixels), pixels);
        h.undo(&mut doc);
        h.undo(&mut doc);
        assert_eq!(doc.background().color, Color::WHITE);
        assert_eq!(doc.sample(0, 0), Some(Color::WHITE));
        assert!(doc.layers()[0].pixels.data().iter().all(|&b| b == 0));
        h.redo(&mut doc);
        h.redo(&mut doc);
        assert_eq!(doc.background().color, Color::BLACK);
        assert_eq!(doc.layers()[0].pixels.pixel(0, 0).map(|p| p.alpha()), Some(255));
        assert_eq!(doc.layers()[0].pixels.pixel(5, 5).map(|p| p.alpha()), Some(0));
        h.undo(&mut doc);
        assert_eq!(doc.layers()[0].pixels.pixel(0, 0).map(|p| p.alpha()), Some(0));
    }

    #[test]
    fn a_structural_step_keeps_replaced_pixels() {
        let mut doc = Document::new(4, 4).unwrap();
        doc.edit_active(doc.bounds(), |p| raster::fill_region(p, PixelRect::new(0, 0, 4, 4), Color::BLACK));
        let mut h = History::default();
        let before = doc.snapshot();
        doc.map_layers(|p| {
            let mut out = p.clone();
            raster::invert_colors(&mut out);
            out
        });
        h.push("invert", vec![Change::document(before, &doc)]);
        assert_eq!(doc.sample(1, 1), Some(Color::WHITE));
        h.undo(&mut doc);
        assert_eq!(doc.sample(1, 1), Some(Color::BLACK));
        h.redo(&mut doc);
        assert_eq!(doc.sample(1, 1), Some(Color::WHITE));
    }

    #[test]
    fn reorienting_keeps_no_pixels_and_undoes_exactly() {
        let mut doc = Document::new(6, 4).unwrap();
        doc.edit_active(doc.bounds(), |p| raster::fill_region(p, PixelRect::new(0, 0, 2, 1), Color::BLACK));
        let original = doc.layers()[0].pixels.data().to_vec();
        let mut h = History::default();
        let op = Reorient::Rotate(raster::QuarterTurn::Cw90);
        let objects = doc.reorient(op);
        h.push("rotate", vec![Change::reorient(op, objects)]);
        assert_eq!((doc.width(), doc.height()), (4, 6));
        assert!(h.bytes < 100);
        h.undo(&mut doc);
        assert_eq!((doc.width(), doc.height()), (6, 4));
        assert_eq!(doc.layers()[0].pixels.data(), &original[..]);
        h.redo(&mut doc);
        assert_eq!((doc.width(), doc.height()), (4, 6));
        assert_eq!(doc.sample(3, 0), Some(Color::BLACK));
    }
}
