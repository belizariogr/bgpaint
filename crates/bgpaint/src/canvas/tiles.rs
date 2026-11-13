//! GPU textures holding the composited image, split into tiles so that only the parts that changed
//! are recomposited and uploaded, and images larger than the maximum texture size still work.

use bgpaint_core::document::{ActivePreview, Document};
use bgpaint_core::geom::PixelRect;
use bgpaint_core::raster;
use egui::{Color32, ColorImage, Painter, Rect, TextureHandle, TextureOptions};
use rayon::prelude::*;

/// Texture size of a tile, in texels.
const TILE: u32 = 512;
/// Coarsest resolution level: 1/2^MAX_LEVEL of the canvas resolution.
const MAX_LEVEL: u32 = 8;
/// Canvas pixels composited at once before being reduced for a coarse level.
const BLOCK: u32 = 1024;
/// Longest side of the overview level (the navigator and the print preview), in texels.
const OVERVIEW: u32 = 512;
/// Canvas blocks composited per frame for the overview while it catches up.
const OVERVIEW_BLOCKS: usize = 48;
/// Texels kept in GPU memory across all levels (4 bytes each): beyond it, the tiles shown least
/// recently are dropped and recomposited if they come back into view.
const TEXEL_BUDGET: usize = 64 << 20;

/// Crisp pixels when zoomed in, smooth when zoomed out. No mipmaps: the level is chosen so a
/// texel never shrinks below half a screen pixel, and partial uploads stay cheap.
const OPTIONS: TextureOptions = TextureOptions {
    magnification: egui::TextureFilter::Nearest,
    minification: egui::TextureFilter::Linear,
    wrap_mode: egui::TextureWrapMode::ClampToEdge,
    mipmap_mode: None,
};

/// The composited image at 1/2^`shift` of the canvas resolution, in tiles.
struct Level {
    shift: u32,
    cols: u32,
    rows: u32,
    textures: Vec<Option<TextureHandle>>,
    /// Per tile, the canvas area whose texture is out of date.
    stale: Vec<Option<PixelRect>>,
    /// Per tile, the frame it was last shown in.
    used: Vec<u64>,
}

impl Level {
    fn new(shift: u32, width: u32, height: u32) -> Self {
        let span = TILE << shift;
        let (cols, rows) = (width.div_ceil(span), height.div_ceil(span));
        let n = (cols * rows) as usize;
        let mut level = Self { shift, cols, rows, textures: vec![None; n], stale: Vec::new(), used: vec![0; n] };
        level.stale = (0..n).map(|i| Some(level.tile_rect(i, width, height))).collect();
        level
    }

    /// Canvas area covered by tile `idx`.
    fn tile_rect(&self, idx: usize, width: u32, height: u32) -> PixelRect {
        let span = TILE << self.shift;
        let (col, row) = (idx as u32 % self.cols, idx as u32 / self.cols);
        PixelRect::from_xywh((col * span) as i32, (row * span) as i32, span, span).clamp_to(width, height)
    }

    /// Texels of tile `idx`.
    fn texels(&self, idx: usize, width: u32, height: u32) -> usize {
        let t = self.tile_rect(idx, width, height);
        (t.width().div_ceil(1 << self.shift) * t.height().div_ceil(1 << self.shift)) as usize
    }

    /// Marks `d` (canvas coords) out of date.
    fn invalidate(&mut self, d: PixelRect, width: u32, height: u32) {
        let span = (TILE << self.shift) as i32;
        let (c0, c1) = ((d.x0.max(0) / span) as u32, (d.x1.max(0) as u32).div_ceil(span as u32).min(self.cols));
        let (r0, r1) = ((d.y0.max(0) / span) as u32, (d.y1.max(0) as u32).div_ceil(span as u32).min(self.rows));
        for row in r0..r1 {
            for col in c0..c1 {
                let idx = (row * self.cols + col) as usize;
                let area = d.intersect(&self.tile_rect(idx, width, height));
                if !area.is_empty() {
                    let s = &mut self.stale[idx];
                    *s = Some(s.map_or(area, |x| x.union(&area)));
                }
            }
        }
    }

    /// Whether every tile has an up-to-date texture.
    fn complete(&self) -> bool {
        self.stale.iter().all(Option::is_none)
    }
}

/// GPU textures holding the composited image. Only the tiles of the level matching the zoom that
/// are on screen (plus a ring around them) are kept up to date; the rest catch up when they are
/// shown. A small overview level serves the navigator and the print preview.
pub struct Tiles {
    width: u32,
    height: u32,
    levels: Vec<Option<Level>>,
    /// The level being shown.
    current: usize,
    structure_version: u64,
    frame: u64,
    /// The overview was painted since the last update: keep it up to date.
    overview_wanted: bool,
}

impl Default for Tiles {
    fn default() -> Self {
        Self { width: 0, height: 0, levels: Vec::new(), current: 0, structure_version: u64::MAX, frame: 0, overview_wanted: false }
    }
}

/// Composites `area` (canvas coords, aligned to 2^`shift` except at the canvas edge) reduced by
/// 2^`shift`, each texel averaging the canvas pixels it covers.
fn composite_level(doc: &Document, area: PixelRect, shift: u32, preview: Option<&dyn ActivePreview>) -> ColorImage {
    let (w, h) = (area.width() as usize, area.height() as usize);
    let mut buf = raster::pixmap(area.width(), area.height());
    doc.composite_into(&mut buf, area, true, preview);
    if shift == 0 {
        return ColorImage::from_rgba_premultiplied([w, h], buf.data());
    }
    let f = 1usize << shift;
    let (ow, oh) = (w.div_ceil(f), h.div_ceil(f));
    let (data, _) = buf.data().as_chunks::<4>();
    let mut pixels = Vec::with_capacity(ow * oh);
    let mut acc = vec![[0u32; 4]; ow];
    for oy in 0..oh {
        acc.fill([0; 4]);
        let rows = oy * f..((oy + 1) * f).min(h);
        let n_rows = rows.len() as u32;
        for row in data[rows.start * w..rows.end * w].chunks_exact(w) {
            for (s, cell) in acc.iter_mut().zip(row.chunks(f)) {
                for p in cell {
                    s[0] += u32::from(p[0]);
                    s[1] += u32::from(p[1]);
                    s[2] += u32::from(p[2]);
                    s[3] += u32::from(p[3]);
                }
            }
        }
        for (ox, s) in acc.iter().enumerate() {
            let n = (f.min(w - ox * f) as u32 * n_rows).max(1);
            let c = |i: usize| ((s[i] + n / 2) / n) as u8;
            pixels.push(Color32::from_rgba_premultiplied(c(0), c(1), c(2), c(3)));
        }
    }
    ColorImage { size: [ow, oh], pixels, source_size: egui::vec2(ow as f32, oh as f32) }
}

/// Splits `area` into blocks aligned to the `BLOCK` grid.
fn blocks(idx: usize, area: PixelRect, out: &mut Vec<(usize, PixelRect)>) {
    let b = BLOCK as i32;
    let mut y = area.y0;
    while y < area.y1 {
        let y1 = ((y.div_euclid(b) + 1) * b).min(area.y1);
        let mut x = area.x0;
        while x < area.x1 {
            let x1 = ((x.div_euclid(b) + 1) * b).min(area.x1);
            out.push((idx, PixelRect::new(x, y, x1, y1)));
            x = x1;
        }
        y = y1;
    }
}

impl Tiles {
    /// The level whose tiles show the whole canvas within [`OVERVIEW`] texels.
    fn overview_shift(&self) -> u32 {
        let side = self.width.max(self.height);
        (0..=MAX_LEVEL).find(|s| side >> s <= OVERVIEW).unwrap_or(MAX_LEVEL)
    }

    /// Brings the textures up to date. `dirty` is the area to recomposite (document changes plus
    /// tool previews). `zoom` is the number of screen pixels per canvas pixel. Tiles touching
    /// `visible` (canvas coords) and a ring around it are updated now.
    pub fn update(
        &mut self,
        ctx: &egui::Context,
        doc: &Document,
        preview: Option<&dyn ActivePreview>,
        dirty: Option<PixelRect>,
        visible: PixelRect,
        zoom: f32,
    ) {
        self.frame += 1;
        if doc.width() != self.width || doc.height() != self.height {
            self.width = doc.width();
            self.height = doc.height();
            // Recreate every texture: tile sizes change with the canvas size.
            self.levels = (0..=MAX_LEVEL).map(|_| None).collect();
        }
        let (width, height) = (self.width, self.height);
        let mut dirty = dirty;
        if doc.structure_version() != self.structure_version {
            self.structure_version = doc.structure_version();
            // Another active layer: the tool preview moves with it. (Changes to pixels mark
            // their area dirty themselves.)
            if let Some(b) = preview.map(|p| p.bounds()) {
                dirty = Some(dirty.map_or(b, |d| d.union(&b)));
            }
        }
        if let Some(d) = dirty {
            for level in self.levels.iter_mut().flatten() {
                level.invalidate(d, width, height);
            }
        }

        // Texels no larger than screen pixels.
        let shift = if zoom > 0.0 { (1.0 / zoom).log2().floor().clamp(0.0, MAX_LEVEL as f32) as u32 } else { 0 };
        self.current = shift as usize;
        let overview = self.overview_shift();
        let want_overview = std::mem::take(&mut self.overview_wanted) && overview != shift;
        for s in [shift, overview] {
            if self.levels[s as usize].is_none() {
                self.levels[s as usize] = Some(Level::new(s, width, height));
            }
        }

        // What to recomposite this frame: the visible tiles and a ring around them, then (when
        // it is shown) a share of the overview.
        let mut jobs: Vec<(usize, usize, PixelRect)> = Vec::new();
        let frame = self.frame;
        let level = self.levels[shift as usize].as_mut().expect("created above");
        let near = visible.expand((TILE << shift) as i32);
        for idx in 0..level.stale.len() {
            let tile = level.tile_rect(idx, width, height);
            if tile.intersect(&visible).is_empty() && tile.intersect(&near).is_empty() {
                continue;
            }
            if !tile.intersect(&visible).is_empty() {
                level.used[idx] = frame;
            }
            if let Some(area) = level.stale[idx] {
                jobs.push((shift as usize, idx, job_area(level, idx, area, width, height)));
            }
        }
        let mut pending = false;
        if want_overview {
            let level = self.levels[overview as usize].as_mut().expect("created above");
            let mut budget = OVERVIEW_BLOCKS;
            for idx in 0..level.stale.len() {
                level.used[idx] = frame;
                let Some(area) = level.stale[idx] else { continue };
                let area = job_area(level, idx, area, width, height);
                let cost = (area.area() as usize).div_ceil((BLOCK * BLOCK) as usize);
                if budget == 0 || (cost > budget && budget < OVERVIEW_BLOCKS) {
                    pending = true;
                    continue;
                }
                budget = budget.saturating_sub(cost);
                jobs.push((overview as usize, idx, area));
            }
        }
        if pending {
            ctx.request_repaint();
        }
        if !jobs.is_empty() {
            self.run(ctx, doc, preview, &jobs);
        }
        self.evict();
    }

    /// Composites `jobs` (level, tile, canvas area) in parallel and uploads them.
    fn run(&mut self, ctx: &egui::Context, doc: &Document, preview: Option<&dyn ActivePreview>, jobs: &[(usize, usize, PixelRect)]) {
        let (width, height) = (self.width, self.height);
        let mut work: Vec<(usize, usize, PixelRect)> = Vec::new();
        for &(l, idx, area) in jobs {
            let mut b = Vec::new();
            blocks(idx, area, &mut b);
            work.extend(b.into_iter().map(|(i, r)| (l, i, r)));
        }
        let done: Vec<(usize, usize, PixelRect, ColorImage)> =
            work.par_iter().map(|&(l, i, a)| (l, i, a, composite_level(doc, a, l as u32, preview))).collect();
        for &(l, idx, _) in jobs {
            let level = self.levels[l].as_mut().expect("level of a job");
            level.stale[idx] = None;
            if level.textures[idx].is_none() {
                let tile = level.tile_rect(idx, width, height);
                let size = [tile.width().div_ceil(1 << level.shift) as usize, tile.height().div_ceil(1 << level.shift) as usize];
                let blank = ColorImage::new(size, vec![Color32::TRANSPARENT; size[0] * size[1]]);
                level.textures[idx] = Some(ctx.load_texture(format!("canvas-{}-{idx}", level.shift), blank, OPTIONS));
            }
        }
        for (l, idx, area, image) in done {
            let level = self.levels[l].as_mut().expect("level of a job");
            let tile = level.tile_rect(idx, width, height);
            let shift = level.shift;
            if let Some(tex) = &mut level.textures[idx] {
                if area == tile {
                    tex.set(image, OPTIONS);
                } else {
                    let pos = [((area.x0 - tile.x0) >> shift) as usize, ((area.y0 - tile.y0) >> shift) as usize];
                    tex.set_partial(pos, image, OPTIONS);
                }
            }
        }
    }

    /// Drops the tiles shown least recently while the textures exceed [`TEXEL_BUDGET`]. Tiles
    /// shown this frame are kept.
    fn evict(&mut self) {
        let (width, height) = (self.width, self.height);
        let mut held: Vec<(u64, usize, usize, usize)> = Vec::new();
        let mut total = 0;
        for (l, level) in self.levels.iter().enumerate() {
            let Some(level) = level else { continue };
            for (idx, tex) in level.textures.iter().enumerate() {
                if tex.is_some() {
                    let n = level.texels(idx, width, height);
                    total += n;
                    held.push((level.used[idx], l, idx, n));
                }
            }
        }
        if total <= TEXEL_BUDGET {
            return;
        }
        held.sort_unstable();
        for (used, l, idx, n) in held {
            if total <= TEXEL_BUDGET || used == self.frame {
                break;
            }
            let level = self.levels[l].as_mut().expect("held level");
            level.textures[idx] = None;
            level.stale[idx] = Some(level.tile_rect(idx, width, height));
            total -= n;
        }
    }

    /// Paints the tiles of the level last brought up to date. `origin` is the screen position of
    /// canvas pixel (0,0) and `scale` the size of a canvas pixel in points. Tile edges are
    /// snapped to physical pixels to avoid seams.
    pub fn paint(&self, painter: &Painter, origin: egui::Pos2, scale: f32, ppp: f32) {
        if let Some(level) = self.levels.get(self.current).and_then(Option::as_ref) {
            self.paint_level(level, painter, origin, scale, ppp);
        }
    }

    /// Paints the whole canvas small (the navigator, the print preview), from the overview level
    /// once it is complete, which later frames keep up to date.
    pub fn paint_overview(&mut self, painter: &Painter, origin: egui::Pos2, scale: f32, ppp: f32) {
        self.overview_wanted = true;
        let shift = self.overview_shift() as usize;
        let level = match self.levels.get(shift).and_then(Option::as_ref) {
            Some(l) if l.complete() || shift == self.current => l,
            _ => match self.levels.get(self.current).and_then(Option::as_ref) {
                Some(l) => l,
                None => return,
            },
        };
        self.paint_level(level, painter, origin, scale, ppp);
    }

    fn paint_level(&self, level: &Level, painter: &Painter, origin: egui::Pos2, scale: f32, ppp: f32) {
        let snap = |v: f32| (v * ppp).round() / ppp;
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        for (idx, tex) in level.textures.iter().enumerate() {
            let Some(tex) = tex else { continue };
            let t = level.tile_rect(idx, self.width, self.height);
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

/// The part of tile `idx` to recomposite for its stale `area`: the whole tile when it has no
/// texture yet, else whole texels around `area`.
fn job_area(level: &Level, idx: usize, area: PixelRect, width: u32, height: u32) -> PixelRect {
    let tile = level.tile_rect(idx, width, height);
    if level.textures[idx].is_none() {
        return tile;
    }
    let shift = level.shift;
    let align = |v: i32| v & !((1 << shift) - 1);
    PixelRect::new(align(area.x0), align(area.y0), align(area.x1 + (1 << shift) - 1), align(area.y1 + (1 << shift) - 1)).intersect(&tile)
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
