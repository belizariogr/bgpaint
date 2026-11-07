//! GPU textures holding the composited image, split into tiles so that only the parts that changed
//! are recomposited and uploaded, and images larger than the maximum texture size still work.

use bgpaint_core::document::{ActivePreview, Document};
use bgpaint_core::geom::PixelRect;
use bgpaint_core::raster;
use egui::{Color32, ColorImage, Painter, Rect, TextureHandle, TextureOptions};

const TILE: u32 = 512;

pub struct Tiles {
    width: u32,
    height: u32,
    cols: u32,
    rows: u32,
    textures: Vec<Option<TextureHandle>>,
    structure_version: u64,
    smooth: bool,
}

impl Default for Tiles {
    fn default() -> Self {
        Self { width: 0, height: 0, cols: 0, rows: 0, textures: Vec::new(), structure_version: u64::MAX, smooth: false }
    }
}

fn options(smooth: bool) -> TextureOptions {
    if smooth {
        TextureOptions {
            magnification: egui::TextureFilter::Linear,
            minification: egui::TextureFilter::Linear,
            wrap_mode: egui::TextureWrapMode::ClampToEdge,
            mipmap_mode: Some(egui::TextureFilter::Linear),
        }
    } else {
        TextureOptions::NEAREST
    }
}

impl Tiles {
    fn tile_rect(&self, col: u32, row: u32) -> PixelRect {
        PixelRect::from_xywh((col * TILE) as i32, (row * TILE) as i32, TILE, TILE).clamp_to(self.width, self.height)
    }

    /// Brings the textures up to date. `dirty` is the area to recomposite (document changes plus
    /// tool previews). `smooth` selects filtering (used when zoomed out).
    pub fn update(
        &mut self,
        ctx: &egui::Context,
        doc: &Document,
        preview: Option<&dyn ActivePreview>,
        dirty: Option<PixelRect>,
        smooth: bool,
    ) {
        let rebuild = doc.width() != self.width
            || doc.height() != self.height
            || doc.structure_version() != self.structure_version
            || smooth != self.smooth;
        if rebuild {
            self.width = doc.width();
            self.height = doc.height();
            self.cols = self.width.div_ceil(TILE);
            self.rows = self.height.div_ceil(TILE);
            self.structure_version = doc.structure_version();
            // Recreate every texture: tile sizes change with the canvas size even when the tile
            // count does not.
            {
                self.textures = vec![None; (self.cols * self.rows) as usize];
            }
            self.smooth = smooth;
        }
        let dirty = if rebuild { Some(doc.bounds()) } else { dirty };
        let opts = options(self.smooth);
        for row in 0..self.rows {
            for col in 0..self.cols {
                let tile = self.tile_rect(col, row);
                let idx = (row * self.cols + col) as usize;
                let missing = self.textures[idx].is_none();
                let area = if missing { tile } else { dirty.map(|d| d.intersect(&tile)).unwrap_or(PixelRect::EMPTY) };
                if area.is_empty() {
                    continue;
                }
                let mut buf = raster::pixmap(area.width(), area.height());
                doc.composite_into(&mut buf, area, true, preview);
                let image = ColorImage::from_rgba_premultiplied([area.width() as usize, area.height() as usize], buf.data());
                match &mut self.textures[idx] {
                    Some(tex) if !missing => {
                        let pos = [(area.x0 - tile.x0) as usize, (area.y0 - tile.y0) as usize];
                        tex.set_partial(pos, image, opts);
                    }
                    slot => *slot = Some(ctx.load_texture(format!("canvas-{col}-{row}"), image, opts)),
                }
            }
        }
    }

    /// Paints the tiles. `origin` is the screen position of canvas pixel (0,0) and `scale` the size
    /// of a canvas pixel in points. Tile edges are snapped to physical pixels to avoid seams.
    pub fn paint(&self, painter: &Painter, origin: egui::Pos2, scale: f32, ppp: f32) {
        let snap = |v: f32| (v * ppp).round() / ppp;
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        for row in 0..self.rows {
            for col in 0..self.cols {
                let Some(tex) = &self.textures[(row * self.cols + col) as usize] else { continue };
                let t = self.tile_rect(col, row);
                let r = Rect::from_min_max(
                    egui::pos2(snap(origin.x + t.x0 as f32 * scale), snap(origin.y + t.y0 as f32 * scale)),
                    egui::pos2(snap(origin.x + t.x1 as f32 * scale), snap(origin.y + t.y1 as f32 * scale)),
                );
                if painter.clip_rect().intersects(r) {
                    painter.image(tex.id(), r, uv, Color32::WHITE);
                }
            }
        }
    }
}

/// A tiny repeating checkerboard used behind transparent pixels.
#[derive(Default)]
pub struct Checker {
    texture: Option<(TextureHandle, [Color32; 2])>,
}

impl Checker {
    pub fn paint(&mut self, ctx: &egui::Context, painter: &Painter, rect: Rect, colors: [Color32; 2]) {
        self.paint_tinted(ctx, painter, rect, colors, Color32::WHITE);
    }

    /// Like [`Checker::paint`], multiplied by `tint` (e.g. to fade it out).
    pub fn paint_tinted(&mut self, ctx: &egui::Context, painter: &Painter, rect: Rect, colors: [Color32; 2], tint: Color32) {
        if self.texture.as_ref().is_none_or(|(_, c)| *c != colors) {
            let img = ColorImage::new([2, 2], vec![colors[0], colors[1], colors[1], colors[0]]);
            let opts = TextureOptions { wrap_mode: egui::TextureWrapMode::Repeat, ..TextureOptions::NEAREST };
            self.texture = Some((ctx.load_texture("checker", img, opts), colors));
        }
        if let Some((tex, _)) = &self.texture {
            // Each texel covers 8×8 points.
            let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(rect.width() / 16.0, rect.height() / 16.0));
            painter.image(tex.id(), rect, uv, tint);
        }
    }
}
