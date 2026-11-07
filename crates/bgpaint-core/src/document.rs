//! The layered document: a stack of transparent layers over an optional solid background. In
//! vector mode a layer can also hold shapes as editable objects, drawn above its pixels.

use crate::color::Color;
use crate::geom::Point;
use crate::geom::{DirtyRect, PixelRect};
use crate::history::Change;
use crate::raster::{self, TooLarge};
use crate::vector::VectorShape;
use std::borrow::Cow;
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
    /// Vector objects above the pixels, bottom first (only while vector mode is on).
    pub objects: Vec<VectorShape>,
}

impl Layer {
    fn new(id: LayerId, name: String, pixels: Pixmap, visible: bool) -> Self {
        Self { id, name, pixels: Arc::new(pixels), visible, objects: Vec::new() }
    }

    /// Draws the vector objects into the pixels and removes them.
    fn bake_objects(&mut self) {
        if self.objects.is_empty() {
            return;
        }
        let pixels = Arc::make_mut(&mut self.pixels);
        for o in self.objects.drain(..) {
            o.draw_into(pixels, (0, 0));
        }
    }
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
    /// New shapes become vector objects instead of pixels.
    pub vector_mode: bool,
}

impl DocState {
    /// Memory held only by this snapshot: layers still shared with the live document (or other
    /// snapshots) are not counted.
    pub fn byte_size(&self) -> usize {
        let pixels: usize = self.layers.iter().filter(|l| Arc::strong_count(&l.pixels) == 1).map(|l| l.pixels.data().len()).sum();
        let objects: usize = self.layers.iter().flat_map(|l| &l.objects).map(VectorShape::unshared_bytes).sum();
        pixels + objects
    }
}

/// Something drawn on top of the active layer while a tool is in progress (a stroke, a floating
/// selection, a shape being edited...). It is rendered exactly as it would be committed.
pub trait ActivePreview {
    /// Canvas area that the preview can touch.
    fn bounds(&self) -> PixelRect;
    /// Draws the preview into `region`, a copy of the active layer covering `rect` (canvas coords).
    fn apply(&self, region: &mut Pixmap, rect: PixelRect);
    /// Drawn above the layer's vector objects (a shape) rather than into its pixels below them.
    fn above_objects(&self) -> bool {
        false
    }
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
        let layer = Layer::new(LayerId(1), "1".into(), pixels, true);
        let mut doc = Self {
            state: DocState { width, height, layers: vec![layer], active: 0, background, vector_mode: false },
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
            state: DocState { width, height, layers: Vec::new(), active: 0, background, vector_mode: false },
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
            doc.state.layers.push(Layer::new(id, name, pixels, visible));
        }
        if doc.state.layers.is_empty() {
            let id = doc.alloc_id();
            doc.state.layers.push(Layer::new(id, "1".into(), raster::pixmap(width, height), true));
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

    /// Replaces every layer by `f(layer)`, which may change the canvas size. Vector objects are
    /// drawn into their layers first.
    pub fn map_layers(&mut self, mut f: impl FnMut(&Pixmap) -> Pixmap) {
        let mut size = None;
        for layer in &mut self.state.layers {
            layer.bake_objects();
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
        self.state.layers[i].bake_objects();
        let p = f(&self.state.layers[i].pixels);
        debug_assert_eq!((p.width(), p.height()), (self.state.width, self.state.height));
        self.state.layers[i].pixels = Arc::new(p);
        self.invalidate_all();
    }

    /// Changes the canvas size, placing old content at (`dx`,`dy`). Vector objects move along
    /// and stay editable.
    pub fn resize_canvas(&mut self, width: u32, height: u32, dx: i32, dy: i32) {
        self.map_layers_shifted(dx, dy, |p| raster::resize_canvas(p, width, height, dx, dy));
    }

    /// Like [`Document::map_layers`] for an `f` that only moves the content by (`dx`,`dy`) (a
    /// canvas resize, a rectangular crop): vector objects move the same way and stay editable.
    pub fn map_layers_shifted(&mut self, dx: i32, dy: i32, f: impl FnMut(&Pixmap) -> Pixmap) {
        let objects: Vec<Vec<VectorShape>> = self.state.layers.iter_mut().map(|l| std::mem::take(&mut l.objects)).collect();
        self.map_layers(f);
        let canvas = (self.width(), self.height());
        for (layer, objects) in self.state.layers.iter_mut().zip(objects) {
            layer.objects = objects
                .into_iter()
                .map(|mut o| {
                    o.geom = o.geom.translated(dx as f32, dy as f32);
                    o.rasterize(canvas);
                    o
                })
                .collect();
        }
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
        let layer = Layer::new(id, name, raster::pixmap(self.width(), self.height()), true);
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
        let layer =
            Layer { id, name: src.name.clone(), pixels: Arc::new((*src.pixels).clone()), visible: src.visible, objects: src.objects };
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
        let mut upper = self.state.layers.remove(index);
        upper.bake_objects();
        let lower = &mut self.state.layers[index - 1];
        lower.bake_objects();
        if upper.visible {
            raster::draw_over(Arc::make_mut(&mut lower.pixels), &upper.pixels, 0, 0, 1.0);
        }
        self.state.active = index - 1;
        self.invalidate_all();
        true
    }

    /// Moves a layer up (`up == true`) or down in the stack.
    pub fn move_layer(&mut self, index: usize, up: bool) -> bool {
        let target = if up { index + 1 } else { index.wrapping_sub(1) };
        self.move_layer_to(index, target)
    }

    /// Moves the layer at `from` so that it ends up at index `to` (bottom = 0); the layers in
    /// between shift by one. The active layer stays the same layer.
    pub fn move_layer_to(&mut self, from: usize, to: usize) -> bool {
        let len = self.state.layers.len();
        if from >= len || to >= len || from == to {
            return false;
        }
        let layer = self.state.layers.remove(from);
        self.state.layers.insert(to, layer);
        let a = self.state.active;
        self.state.active = if a == from {
            to
        } else if from < a && a <= to {
            a - 1
        } else if to <= a && a < from {
            a + 1
        } else {
            a
        };
        self.invalidate_all();
        true
    }

    pub fn set_layer_visible(&mut self, index: usize, visible: bool) {
        if let Some(l) = self.state.layers.get_mut(index)
            && l.visible != visible
        {
            l.visible = visible;
            self.invalidate_all();
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
            let objects = layer.objects.iter().filter(|o| !o.bounds().intersect(&rect).is_empty());
            match preview {
                Some(p) if i == self.state.active => {
                    if let Some(mut region) = raster::copy_region(&layer.pixels, rect) {
                        let above = p.above_objects();
                        if !above {
                            p.apply(&mut region, rect);
                        }
                        for o in objects {
                            o.draw_into(&mut region, (rect.x0, rect.y0));
                        }
                        if above {
                            p.apply(&mut region, rect);
                        }
                        raster::draw_over(out, &region, 0, 0, 1.0);
                    }
                }
                _ => {
                    raster::draw_over(out, &layer.pixels, -rect.x0, -rect.y0, 1.0);
                    for o in objects {
                        o.draw_into(out, (rect.x0, rect.y0));
                    }
                }
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

    // ----- Vector objects -----

    pub fn vector_mode(&self) -> bool {
        self.state.vector_mode
    }

    /// Turns vector mode on or off. Turning it off draws every vector object into its layer.
    /// Returns whether anything changed.
    pub fn set_vector_mode(&mut self, on: bool) -> bool {
        if self.state.vector_mode == on {
            return false;
        }
        self.state.vector_mode = on;
        if !on {
            for layer in &mut self.state.layers {
                layer.bake_objects();
            }
        }
        self.invalidate_all();
        true
    }

    /// Adds an object on top of the active layer.
    pub fn add_object(&mut self, shape: VectorShape) {
        self.invalidate(shape.bounds());
        let i = self.state.active;
        self.state.layers[i].objects.push(shape);
    }

    /// Puts an object back at `index` in the stack of layer `layer` (the active layer when that
    /// layer is gone).
    pub fn insert_object(&mut self, layer: LayerId, index: usize, shape: VectorShape) {
        self.invalidate(shape.bounds());
        let i = self.state.layers.iter().position(|l| l.id == layer).unwrap_or(self.state.active);
        let objects = &mut self.state.layers[i].objects;
        objects.insert(index.min(objects.len()), shape);
    }

    /// Removes object `index` of layer `layer` and returns it.
    pub fn take_object(&mut self, layer: usize, index: usize) -> Option<VectorShape> {
        let objects = &mut self.state.layers.get_mut(layer)?.objects;
        if index >= objects.len() {
            return None;
        }
        let shape = objects.remove(index);
        self.invalidate(shape.bounds());
        Some(shape)
    }

    /// The topmost object under `p` (within `tol` pixels) on a visible layer: (layer, object).
    /// Only the canvas counts: what lies outside it is not shown, so it cannot be picked.
    pub fn object_at(&self, p: Point, tol: f32) -> Option<(usize, usize)> {
        if !self.bounds().contains(p.x.floor() as i32, p.y.floor() as i32) {
            return None;
        }
        self.state
            .layers
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, l)| l.visible)
            .find_map(|(li, l)| l.objects.iter().rposition(|o| o.hit(p, tol)).map(|oi| (li, oi)))
    }

    /// Layer `index` as it looks: its pixels with its vector objects drawn on top.
    pub fn layer_image(&self, index: usize) -> Cow<'_, Pixmap> {
        let layer = &self.state.layers[index];
        if layer.objects.is_empty() {
            return Cow::Borrowed(&layer.pixels);
        }
        let mut p = (*layer.pixels).clone();
        for o in &layer.objects {
            o.draw_into(&mut p, (0, 0));
        }
        Cow::Owned(p)
    }

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
    fn move_layer_to_keeps_the_active_layer() {
        let mut doc = Document::new(2, 2).unwrap();
        doc.add_layer();
        doc.add_layer();
        doc.add_layer();
        let names = |d: &Document| d.layers().iter().map(|l| l.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&doc), ["1", "2", "3", "4"]);
        doc.set_active(1);
        // Top layer dropped at the bottom.
        assert!(doc.move_layer_to(3, 0));
        assert_eq!(names(&doc), ["4", "1", "2", "3"]);
        assert_eq!(doc.active_layer().name, "2");
        // The active layer itself, moved up.
        assert!(doc.move_layer_to(2, 3));
        assert_eq!(names(&doc), ["4", "1", "3", "2"]);
        assert_eq!(doc.active_layer().name, "2");
        doc.set_active(0);
        assert!(doc.move_layer_to(1, 3));
        assert_eq!(names(&doc), ["4", "3", "2", "1"]);
        assert_eq!(doc.active_layer().name, "4");
        assert!(!doc.move_layer_to(1, 1));
        assert!(!doc.move_layer_to(0, 4));
    }

    fn red_square(x: f32) -> VectorShape {
        use crate::geom::RectF;
        use crate::shapes::{ShapeKind, ShapePaint, ShapeStyle};
        use crate::vector::ShapeGeom;
        let paint = ShapePaint {
            outline: ShapeStyle::None,
            fill: ShapeStyle::Solid,
            width: 1.0,
            outline_color: Color::BLACK,
            fill_color: Color::rgb(255, 0, 0),
            opacity: 1.0,
        };
        let geom = ShapeGeom::Box { kind: ShapeKind::Rectangle, rect: RectF::from_xywh(x, 2.0, 4.0, 4.0), angle: 0.0, points: None };
        VectorShape::new(geom, paint, (20, 10))
    }

    #[test]
    fn vector_objects_draw_above_pixels_until_baked() {
        let mut doc = Document::new(20, 10).unwrap();
        assert!(doc.set_vector_mode(true));
        doc.edit_active(doc.bounds(), |p| raster::fill_region(p, PixelRect::new(0, 0, 20, 10), Color::BLACK));
        doc.add_object(red_square(2.0));
        assert_eq!(doc.sample(3, 3), Some(Color::rgb(255, 0, 0)));
        assert_eq!(raster::get_pixel(&doc.layers()[0].pixels, 3, 3), Some(Color::BLACK));
        assert_eq!(raster::get_pixel(&doc.layer_image(0), 3, 3), Some(Color::rgb(255, 0, 0)));
        assert_eq!(doc.object_at(Point::new(3.0, 3.0), 0.0), Some((0, 0)));
        assert_eq!(doc.object_at(Point::new(15.0, 3.0), 1.0), None);

        // Undo snapshots carry the objects.
        let before = doc.snapshot();
        assert!(doc.set_vector_mode(false));
        assert!(doc.layers()[0].objects.is_empty());
        assert_eq!(raster::get_pixel(&doc.layers()[0].pixels, 3, 3), Some(Color::rgb(255, 0, 0)));
        doc.swap_state(before);
        assert!(doc.vector_mode());
        assert_eq!(doc.layers()[0].objects.len(), 1);
        assert_eq!(raster::get_pixel(&doc.layers()[0].pixels, 3, 3), Some(Color::BLACK));
    }

    #[test]
    fn objects_follow_canvas_resize_and_survive_layer_ops() {
        let mut doc = Document::new(20, 10).unwrap();
        doc.set_vector_mode(true);
        doc.add_object(red_square(2.0));
        doc.resize_canvas(30, 10, 5, 0);
        assert_eq!(doc.sample(8, 3), Some(Color::rgb(255, 0, 0)));
        assert_eq!(doc.object_at(Point::new(8.0, 3.0), 0.0), Some((0, 0)));
        doc.duplicate_layer(0);
        assert_eq!(doc.layers()[1].objects.len(), 1);
        // Merging draws the objects of both layers into the result.
        assert!(doc.merge_down(1));
        assert!(doc.layers()[0].objects.is_empty());
        assert_eq!(raster::get_pixel(&doc.layers()[0].pixels, 8, 3), Some(Color::rgb(255, 0, 0)));
        // Moving an object out and back in keeps its place in the stack.
        doc.add_object(red_square(10.0));
        doc.add_object(red_square(20.0));
        let s = doc.take_object(0, 0).unwrap();
        doc.insert_object(doc.layers()[0].id, 0, s);
        assert_eq!(doc.object_at(Point::new(11.0, 3.0), 0.0), Some((0, 0)));
    }

    #[test]
    fn shifted_maps_keep_objects_and_only_the_canvas_picks() {
        let mut doc = Document::new(20, 10).unwrap();
        doc.set_vector_mode(true);
        doc.add_object(red_square(10.0));
        // A crop to x 8..18: the square (10..14) ends up at 2..6, still an object.
        doc.map_layers_shifted(-8, 0, |p| raster::copy_region(p, PixelRect::new(8, 0, 18, 10)).unwrap());
        assert_eq!((doc.width(), doc.height()), (10, 10));
        assert_eq!(doc.object_at(Point::new(3.0, 3.0), 0.0), Some((0, 0)));
        assert_eq!(doc.sample(3, 3), Some(Color::rgb(255, 0, 0)));
        // Outside the canvas nothing can be picked, even where an object's raster reaches.
        assert_eq!(doc.object_at(Point::new(-1.0, 3.0), 2.0), None);
        // The objects' rasters count in the history's memory budget: once the document moves on
        // (a canvas resize redraws them), a snapshot alone holds the old ones.
        let before = doc.snapshot();
        let pixels_only = 10 * 10 * 4;
        doc.resize_canvas(12, 10, 0, 0);
        assert!(before.byte_size() > pixels_only, "{}", before.byte_size());
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
