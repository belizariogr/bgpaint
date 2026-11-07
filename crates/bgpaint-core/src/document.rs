//! The layered document: a stack of transparent layers over an optional solid background.

use crate::color::Color;
use crate::geom::{DirtyRect, PixelRect};
use crate::history::Change;
use crate::raster::{self, TooLarge};
use std::sync::Arc;
use tiny_skia::Pixmap;

/// Default size of a new canvas, matching Paint's classic default.
pub const DEFAULT_WIDTH: u32 = 1152;
pub const DEFAULT_HEIGHT: u32 = 648;

/// Stable identifier of a layer (survives reordering).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerId(pub u64);

#[derive(Clone, Debug)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    /// Shared so that document snapshots (undo) are cheap; mutated through [`Arc::make_mut`].
    pub pixels: Arc<Pixmap>,
    pub visible: bool,
}

/// The solid background below all layers ("Tela de fundo" in Paint).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Background {
    pub color: Color,
    pub visible: bool,
}

/// Everything that a structural undo step restores.
#[derive(Clone, Debug)]
pub struct DocState {
    pub width: u32,
    pub height: u32,
    pub layers: Vec<Layer>,
    pub active: usize,
    pub background: Background,
}

impl DocState {
    /// Memory held only by this snapshot: layers still shared with the live document (or other
    /// snapshots) are not counted.
    pub fn byte_size(&self) -> usize {
        self.layers.iter().filter(|l| Arc::strong_count(&l.pixels) == 1).map(|l| l.pixels.data().len()).sum()
    }
}

/// Something drawn on top of the active layer while a tool is in progress (a stroke, a floating
/// selection, a shape being edited...). It is rendered exactly as it would be committed.
pub trait ActivePreview {
    /// Canvas area that the preview can touch.
    fn bounds(&self) -> PixelRect;
    /// Draws the preview into `region`, a copy of the active layer covering `rect` (canvas coords).
    fn apply(&self, region: &mut Pixmap, rect: PixelRect);
}

#[derive(Debug)]
pub struct Document {
    state: DocState,
    next_id: u64,
    dirty: DirtyRect,
    structure_version: u64,
}

impl Document {
    /// A new white canvas with a single empty layer.
    pub fn new(width: u32, height: u32) -> Result<Self, TooLarge> {
        let pixels = raster::try_pixmap(width, height)?;
        Ok(Self::with_layer(pixels, Background { color: Color::WHITE, visible: true }))
    }

    /// A document whose single layer holds `pixels`. Images with transparency get a hidden
    /// background so that the transparency is preserved.
    pub fn from_image(pixels: Pixmap) -> Self {
        let visible = !raster::has_transparency(&pixels);
        Self::with_layer(pixels, Background { color: Color::WHITE, visible })
    }

    fn with_layer(pixels: Pixmap, background: Background) -> Self {
        let (width, height) = (pixels.width(), pixels.height());
        let layer = Layer { id: LayerId(1), name: "1".into(), pixels: Arc::new(pixels), visible: true };
        let mut doc = Self {
            state: DocState { width, height, layers: vec![layer], active: 0, background },
            next_id: 2,
            dirty: DirtyRect::default(),
            structure_version: 0,
        };
        doc.invalidate_all();
        doc
    }

    /// Builds a document from explicit layers (bottom first). Used by project loading.
    pub fn from_layers(width: u32, height: u32, layers: Vec<(String, Pixmap, bool)>, background: Background) -> Self {
        let mut doc = Self {
            state: DocState { width, height, layers: Vec::new(), active: 0, background },
            next_id: 1,
            dirty: DirtyRect::default(),
            structure_version: 0,
        };
        for (name, pixels, visible) in layers {
            let id = doc.alloc_id();
            let pixels = if pixels.width() == width && pixels.height() == height {
                pixels
            } else {
                raster::resize_canvas(&pixels, width, height, 0, 0)
            };
            doc.state.layers.push(Layer { id, name, pixels: Arc::new(pixels), visible });
        }
        if doc.state.layers.is_empty() {
            let id = doc.alloc_id();
            doc.state.layers.push(Layer { id, name: "1".into(), pixels: Arc::new(raster::pixmap(width, height)), visible: true });
        }
        doc.state.active = doc.state.layers.len() - 1;
        doc.invalidate_all();
        doc
    }

    fn alloc_id(&mut self) -> LayerId {
        let id = LayerId(self.next_id);
        self.next_id += 1;
        id
    }

    pub fn width(&self) -> u32 {
        self.state.width
    }

    pub fn height(&self) -> u32 {
        self.state.height
    }

    pub fn bounds(&self) -> PixelRect {
        PixelRect::from_size(self.state.width, self.state.height)
    }

    pub fn layers(&self) -> &[Layer] {
        &self.state.layers
    }

    pub fn active_index(&self) -> usize {
        self.state.active
    }

    pub fn active_layer(&self) -> &Layer {
        &self.state.layers[self.state.active]
    }

    pub fn background(&self) -> Background {
        self.state.background
    }

    /// Changes whenever the layer list or canvas size changes.
    pub fn structure_version(&self) -> u64 {
        self.structure_version
    }

    /// Takes the accumulated area that needs to be redisplayed.
    pub fn take_dirty(&mut self) -> Option<PixelRect> {
        self.dirty.take()
    }

    pub fn invalidate(&mut self, r: PixelRect) {
        self.dirty.add(r.intersect(&self.bounds()));
    }

    pub fn invalidate_all(&mut self) {
        self.dirty.add(self.bounds());
        self.structure_version += 1;
    }

    /// Direct mutable access to the active layer pixels. Callers must record undo and invalidate.
    fn active_pixels_mut(&mut self) -> &mut Pixmap {
        let i = self.state.active;
        Arc::make_mut(&mut self.state.layers[i].pixels)
    }

    /// Edits the active layer. `rect` must cover every pixel `f` modifies; it is saved for undo.
    pub fn edit_active(&mut self, rect: PixelRect, f: impl FnOnce(&mut Pixmap)) -> Option<Change> {
        let rect = rect.intersect(&self.bounds());
        if rect.is_empty() {
            return None;
        }
        let layer = self.active_layer().id;
        let before = raster::copy_region(&self.active_layer().pixels, rect)?;
        f(self.active_pixels_mut());
        self.invalidate(rect);
        Some(Change::Pixels { layer, x: rect.x0, y: rect.y0, pixels: before })
    }

    /// A cheap snapshot of the whole document, for structural undo.
    pub fn snapshot(&self) -> DocState {
        self.state.clone()
    }

    /// Replaces the document state (used by undo/redo), returning the previous state.
    pub fn swap_state(&mut self, mut state: DocState) -> DocState {
        std::mem::swap(&mut self.state, &mut state);
        self.state.active = self.state.active.min(self.state.layers.len().saturating_sub(1));
        self.invalidate_all();
        state
    }

    /// Swaps a stored region with the layer content (undo/redo of a pixel change).
    pub(crate) fn swap_pixels(&mut self, layer: LayerId, x: i32, y: i32, pixels: &mut Pixmap) -> bool {
        let Some(l) = self.state.layers.iter_mut().find(|l| l.id == layer) else {
            return false;
        };
        let target = Arc::make_mut(&mut l.pixels);
        let fits = PixelRect::from_xywh(x, y, pixels.width(), pixels.height());
        if fits.intersect(&raster::bounds(target)) != fits {
            return false;
        }
        raster::swap_region(target, pixels, x, y);
        self.invalidate(fits);
        true
    }

    // ----- Structural operations (callers snapshot first for undo) -----

    /// Replaces every layer by `f(layer)`, which may change the canvas size.
    pub fn map_layers(&mut self, mut f: impl FnMut(&Pixmap) -> Pixmap) {
        let mut size = None;
        for layer in &mut self.state.layers {
            let p = f(&layer.pixels);
            size = Some((p.width(), p.height()));
            layer.pixels = Arc::new(p);
        }
        if let Some((w, h)) = size {
            self.state.width = w;
            self.state.height = h;
        }
        self.invalidate_all();
    }

    /// Replaces only the active layer by `f(layer)`. The result must keep the canvas size.
    pub fn map_active(&mut self, f: impl FnOnce(&Pixmap) -> Pixmap) {
        let i = self.state.active;
        let p = f(&self.state.layers[i].pixels);
        debug_assert_eq!((p.width(), p.height()), (self.state.width, self.state.height));
        self.state.layers[i].pixels = Arc::new(p);
        self.invalidate_all();
    }

    /// Changes the canvas size, placing old content at (`dx`,`dy`).
    pub fn resize_canvas(&mut self, width: u32, height: u32, dx: i32, dy: i32) {
        self.map_layers(|p| raster::resize_canvas(p, width, height, dx, dy));
    }

    pub fn set_active(&mut self, index: usize) {
        if index < self.state.layers.len() && index != self.state.active {
            self.state.active = index;
            self.structure_version += 1;
        }
    }

    /// Adds an empty layer above the active one and activates it.
    pub fn add_layer(&mut self) {
        let id = self.alloc_id();
        let name = self.unique_layer_name();
        let layer = Layer { id, name, pixels: Arc::new(raster::pixmap(self.width(), self.height())), visible: true };
        let at = self.state.active + 1;
        self.state.layers.insert(at, layer);
        self.state.active = at;
        self.invalidate_all();
    }

    fn unique_layer_name(&self) -> String {
        let mut n = self.state.layers.len() + 1;
        loop {
            let name = n.to_string();
            if !self.state.layers.iter().any(|l| l.name == name) {
                return name;
            }
            n += 1;
        }
    }

    pub fn duplicate_layer(&mut self, index: usize) {
        let Some(src) = self.state.layers.get(index).cloned() else { return };
        let id = self.alloc_id();
        let layer = Layer { id, name: src.name.clone(), pixels: Arc::new((*src.pixels).clone()), visible: src.visible };
        self.state.layers.insert(index + 1, layer);
        self.state.active = index + 1;
        self.invalidate_all();
    }

    /// Deletes a layer; the last remaining layer cannot be deleted.
    pub fn delete_layer(&mut self, index: usize) -> bool {
        if self.state.layers.len() <= 1 || index >= self.state.layers.len() {
            return false;
        }
        self.state.layers.remove(index);
        if self.state.active >= index && self.state.active > 0 {
            self.state.active -= 1;
        }
        self.invalidate_all();
        true
    }

    /// Merges layer `index` into the layer below it.
    pub fn merge_down(&mut self, index: usize) -> bool {
        if index == 0 || index >= self.state.layers.len() {
            return false;
        }
        let upper = self.state.layers.remove(index);
        let lower = &mut self.state.layers[index - 1];
        if upper.visible {
            raster::draw_over(Arc::make_mut(&mut lower.pixels), &upper.pixels, 0, 0, 1.0);
        }
        self.state.active = index - 1;
        self.invalidate_all();
        true
    }

    /// Moves a layer up (`up == true`) or down in the stack.
    pub fn move_layer(&mut self, index: usize, up: bool) -> bool {
        let len = self.state.layers.len();
        let target = if up { index + 1 } else { index.wrapping_sub(1) };
        if index >= len || target >= len {
            return false;
        }
        self.state.layers.swap(index, target);
        if self.state.active == index {
            self.state.active = target;
        } else if self.state.active == target {
            self.state.active = index;
        }
        self.invalidate_all();
        true
    }

    pub fn set_layer_visible(&mut self, index: usize, visible: bool) {
        if let Some(l) = self.state.layers.get_mut(index) {
            if l.visible != visible {
                l.visible = visible;
                self.invalidate_all();
            }
        }
    }

    pub fn set_background(&mut self, background: Background) {
        if self.state.background != background {
            self.state.background = background;
            self.invalidate_all();
        }
    }

    // ----- Rendering -----

    /// Composites the visible layers covering `rect` into `out` (which has the size of `rect`).
    pub fn composite_into(&self, out: &mut Pixmap, rect: PixelRect, with_background: bool, preview: Option<&dyn ActivePreview>) {
        debug_assert_eq!((out.width(), out.height()), (rect.width(), rect.height()));
        let bg = self.state.background;
        if with_background && bg.visible {
            out.fill(bg.color.to_skia());
        } else {
            out.fill(tiny_skia::Color::TRANSPARENT);
        }
        let preview = preview.filter(|p| !p.bounds().intersect(&rect).is_empty());
        for (i, layer) in self.state.layers.iter().enumerate() {
            if !layer.visible {
                continue;
            }
            match preview {
                Some(p) if i == self.state.active => {
                    if let Some(mut region) = raster::copy_region(&layer.pixels, rect) {
                        p.apply(&mut region, rect);
                        raster::draw_over(out, &region, 0, 0, 1.0);
                    }
                }
                _ => raster::draw_over(out, &layer.pixels, -rect.x0, -rect.y0, 1.0),
            }
        }
    }

    /// The flattened image (background included when visible).
    pub fn flatten(&self) -> Pixmap {
        self.flatten_region(self.bounds(), true)
    }

    pub fn flatten_region(&self, rect: PixelRect, with_background: bool) -> Pixmap {
        let rect = rect.intersect(&self.bounds());
        let mut out = raster::pixmap(rect.width(), rect.height());
        self.composite_into(&mut out, rect, with_background, None);
        out
    }

    /// Color of the composited image at a pixel.
    pub fn sample(&self, x: i32, y: i32) -> Option<Color> {
        if !self.bounds().contains(x, y) {
            return None;
        }
        let p = self.flatten_region(PixelRect::from_xywh(x, y, 1, 1), true);
        raster::get_pixel(&p, 0, 0)
    }

    // ----- Layer property edits (callers snapshot first for undo) -----

    pub fn rename_layer(&mut self, index: usize, name: String) {
        if let Some(l) = self.state.layers.get_mut(index) {
            l.name = name;
            self.structure_version += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_document() {
        let doc = Document::new(10, 5).unwrap();
        assert_eq!((doc.width(), doc.height()), (10, 5));
        assert_eq!(doc.layers().len(), 1);
        assert!(doc.background().visible);
        let flat = doc.flatten();
        assert_eq!(raster::get_pixel(&flat, 3, 3), Some(Color::WHITE));
    }

    #[test]
    fn transparent_image_hides_background() {
        let doc = Document::from_image(raster::pixmap(4, 4));
        assert!(!doc.background().visible);
        assert!(raster::is_fully_transparent(&doc.flatten()));
    }

    #[test]
    fn layer_ops() {
        let mut doc = Document::new(4, 4).unwrap();
        doc.add_layer();
        assert_eq!(doc.layers().len(), 2);
        assert_eq!(doc.active_index(), 1);
        doc.edit_active(doc.bounds(), |p| raster::fill_region(p, PixelRect::new(0, 0, 2, 2), Color::BLACK));
        doc.duplicate_layer(1);
        assert_eq!(doc.layers().len(), 3);
        assert!(doc.move_layer(2, false));
        assert!(doc.merge_down(1));
        assert_eq!(doc.layers().len(), 2);
        assert!(doc.delete_layer(1));
        assert!(!doc.delete_layer(0));
        assert_eq!(doc.active_index(), 0);
    }

    #[test]
    fn composite_respects_visibility_and_order() {
        let mut doc = Document::new(2, 1).unwrap();
        doc.edit_active(doc.bounds(), |p| raster::fill_region(p, PixelRect::new(0, 0, 1, 1), Color::BLACK));
        doc.add_layer();
        doc.edit_active(doc.bounds(), |p| raster::fill_region(p, PixelRect::new(0, 0, 1, 1), Color::rgb(255, 0, 0)));
        assert_eq!(doc.sample(0, 0), Some(Color::rgb(255, 0, 0)));
        doc.set_layer_visible(1, false);
        assert_eq!(doc.sample(0, 0), Some(Color::BLACK));
        assert_eq!(doc.sample(1, 0), Some(Color::WHITE));
    }
}
