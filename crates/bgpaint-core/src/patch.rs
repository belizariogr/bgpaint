//! Compact copies of layer regions for undo. A region is cut into tiles; only the tiles an edit
//! changed are kept, and each one is stored as a single color, as runs of equal pixels or raw,
//! whichever is smallest. Filling a blank 10000×10000 canvas keeps a few hundred kilobytes
//! instead of a 400 MB copy.

use crate::geom::PixelRect;
use rayon::prelude::*;
use tiny_skia::Pixmap;

/// Tile side, in pixels.
const TILE: i32 = 64;

type Px = [u8; 4];

#[derive(Debug)]
enum Tile {
    Solid(Px),
    /// (length, color) runs in row order.
    Runs(Vec<(u32, Px)>),
    Raw(Vec<Px>),
}

/// Rows of one band of a layer: full-width rows (`stride` pixels each), of which a tile uses
/// columns `x0..x1`.
#[derive(Clone, Copy)]
struct Cols {
    stride: usize,
    x0: usize,
    x1: usize,
}

impl Cols {
    fn rows(self, band: &[Px]) -> impl Iterator<Item = &[Px]> + Clone {
        band.chunks_exact(self.stride).map(move |r| &r[self.x0..self.x1])
    }

    fn rows_mut(self, band: &mut [Px]) -> impl Iterator<Item = &mut [Px]> {
        band.chunks_exact_mut(self.stride).map(move |r| &mut r[self.x0..self.x1])
    }
}

impl Tile {
    fn byte_size(&self) -> usize {
        match self {
            Tile::Solid(_) => 4,
            Tile::Runs(r) => r.len() * 8,
            Tile::Raw(p) => p.len() * 4,
        }
    }

    /// Encodes the tile at `cols` of `band`.
    fn read(band: &[Px], cols: Cols) -> Tile {
        let area = (cols.x1 - cols.x0) * (band.len() / cols.stride);
        // Runs only pay off below half the raw size.
        let max_runs = area / 4;
        let mut runs: Vec<(u32, Px)> = Vec::new();
        for row in cols.rows(band) {
            for &c in row {
                if let Some((n, last)) = runs.last_mut()
                    && *last == c
                {
                    *n += 1;
                } else if runs.len() >= max_runs {
                    return Tile::Raw(cols.rows(band).flatten().copied().collect());
                } else {
                    runs.push((1, c));
                }
            }
        }
        match runs.as_slice() {
            [(_, c)] => Tile::Solid(*c),
            _ => Tile::Runs(runs),
        }
    }

    /// Whether `band` holds exactly this tile at `cols`.
    fn matches(&self, band: &[Px], cols: Cols) -> bool {
        match self {
            Tile::Solid(c) => cols.rows(band).all(|row| row.iter().all(|p| p == c)),
            Tile::Raw(p) => cols.rows(band).zip(p.chunks_exact(cols.x1 - cols.x0)).all(|(a, b)| a == b),
            Tile::Runs(runs) => {
                let mut it = runs.iter().flat_map(|(n, c)| std::iter::repeat_n(c, *n as usize));
                cols.rows(band).flatten().all(|p| it.next() == Some(p))
            }
        }
    }

    /// Writes this tile into `band` at `cols`.
    fn write(&self, band: &mut [Px], cols: Cols) {
        let rows = cols.rows_mut(band);
        match self {
            Tile::Solid(c) => rows.for_each(|r| r.fill(*c)),
            Tile::Raw(p) => {
                for (r, src) in rows.zip(p.chunks_exact(cols.x1 - cols.x0)) {
                    r.copy_from_slice(src);
                }
            }
            Tile::Runs(runs) => {
                let mut it = runs.iter().flat_map(|(n, c)| std::iter::repeat_n(*c, *n as usize));
                for r in rows {
                    for (d, c) in r.iter_mut().zip(&mut it) {
                        *d = c;
                    }
                }
            }
        }
    }
}

/// The tiles of one band of rows: (tile column, tile).
type Band = Vec<(u32, Tile)>;

/// A saved region of a layer, swapped back and forth by undo and redo.
#[derive(Debug)]
pub struct Patch {
    rect: PixelRect,
    /// One entry per band of [`TILE`] rows, from the top of `rect`.
    bands: Vec<Band>,
}

impl Patch {
    /// Saves the `rect` region of `layer` (which must lie inside it).
    pub fn capture(layer: &Pixmap, rect: PixelRect) -> Self {
        Self::capture_where(layer, rect, |_| true)
    }

    /// Saves the tiles of the `rect` region of `layer` for which `touched(tile area)` holds: an
    /// edit that knows where it can draw saves only that.
    pub fn capture_where(layer: &Pixmap, rect: PixelRect, touched: impl Fn(PixelRect) -> bool + Sync) -> Self {
        let stride = layer.width() as usize;
        let (data, _) = layer.data().as_chunks::<4>();
        let part = &data[rect.y0 as usize * stride..rect.y1 as usize * stride];
        let bands = part
            .par_chunks(stride * TILE as usize)
            .enumerate()
            .map(|(b, band)| {
                tile_rects(rect, b).filter(|(_, r)| touched(*r)).map(|(col, r)| (col, Tile::read(band, cols(stride, r)))).collect()
            })
            .collect();
        Self { rect, bands }
    }

    /// Forgets the tiles that `layer` still holds unchanged.
    pub fn keep_changed(&mut self, layer: &Pixmap) {
        let rect = self.rect;
        let stride = layer.width() as usize;
        let (data, _) = layer.data().as_chunks::<4>();
        let part = &data[rect.y0 as usize * stride..rect.y1 as usize * stride];
        self.bands.par_iter_mut().zip(part.par_chunks(stride * TILE as usize)).enumerate().for_each(|(b, (tiles, band))| {
            tiles.retain(|(col, t)| !t.matches(band, cols(stride, tile_rect(rect, *col, b))));
        });
    }

    pub fn is_empty(&self) -> bool {
        self.bands.iter().all(Vec::is_empty)
    }

    /// Area the saved tiles cover.
    pub fn bounds(&self) -> PixelRect {
        let mut out = PixelRect::EMPTY;
        for (b, band) in self.bands.iter().enumerate() {
            for (col, _) in band {
                out = out.union(&tile_rect(self.rect, *col, b));
            }
        }
        out
    }

    pub fn byte_size(&self) -> usize {
        self.bands.iter().flatten().map(|(_, t)| t.byte_size() + 8).sum::<usize>() + self.bands.len() * 24
    }

    /// Exchanges the saved tiles with the layer content. Returns false (and changes nothing) when
    /// the region no longer fits the layer.
    pub fn swap(&mut self, layer: &mut Pixmap) -> bool {
        let r = self.rect;
        if r.x0 < 0 || r.y0 < 0 || r.x1 > layer.width() as i32 || r.y1 > layer.height() as i32 {
            return false;
        }
        let stride = layer.width() as usize;
        let (data, _) = layer.data_mut().as_chunks_mut::<4>();
        let part = &mut data[r.y0 as usize * stride..r.y1 as usize * stride];
        self.bands.par_iter_mut().zip(part.par_chunks_mut(stride * TILE as usize)).enumerate().for_each(|(b, (tiles, band))| {
            for (col, t) in tiles.iter_mut() {
                let c = cols(stride, tile_rect(r, *col, b));
                let current = Tile::read(band, c);
                t.write(band, c);
                *t = current;
            }
        });
        true
    }
}

fn cols(stride: usize, r: PixelRect) -> Cols {
    Cols { stride, x0: r.x0 as usize, x1: r.x1 as usize }
}

fn tile_rect(r: PixelRect, col: u32, band: usize) -> PixelRect {
    let x0 = r.x0 + col as i32 * TILE;
    let y0 = r.y0 + band as i32 * TILE;
    PixelRect::new(x0, y0, (x0 + TILE).min(r.x1), (y0 + TILE).min(r.y1))
}

fn tile_rects(r: PixelRect, band: usize) -> impl Iterator<Item = (u32, PixelRect)> {
    let cols = r.width().div_ceil(TILE as u32);
    (0..cols).map(move |c| (c, tile_rect(r, c, band)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::raster;

    fn noisy(w: u32, h: u32) -> Pixmap {
        let mut p = raster::pixmap(w, h);
        for (i, px) in p.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let v = (i as u32).wrapping_mul(2_654_435_761) >> 24;
            px.copy_from_slice(&[v as u8, (v >> 1) as u8, 0, 255]);
        }
        p
    }

    #[test]
    fn swap_restores_and_redoes() {
        let mut layer = noisy(150, 130);
        raster::fill_region(&mut layer, PixelRect::new(0, 0, 150, 40), Color::WHITE);
        let original = layer.clone();
        let rect = PixelRect::new(5, 3, 149, 129);
        let mut patch = Patch::capture(&layer, rect);
        raster::fill_region(&mut layer, PixelRect::new(10, 10, 70, 20), Color::BLACK);
        let edited = layer.clone();
        patch.keep_changed(&layer);
        assert_eq!(patch.bounds(), PixelRect::new(5, 3, 133, 67));
        assert!(patch.swap(&mut layer));
        assert_eq!(layer.data(), original.data());
        assert!(patch.swap(&mut layer));
        assert_eq!(layer.data(), edited.data());
    }

    #[test]
    fn blank_regions_are_small() {
        let mut layer = raster::pixmap(1000, 1000);
        let mut patch = Patch::capture(&layer, raster::bounds(&layer));
        raster::fill_region(&mut layer, PixelRect::new(0, 0, 1000, 1000), Color::BLACK);
        patch.keep_changed(&layer);
        assert!(patch.byte_size() < 10_000, "{}", patch.byte_size());
        assert!(patch.swap(&mut layer));
        assert!(layer.data().iter().all(|&b| b == 0));
    }

    #[test]
    fn unchanged_regions_are_empty() {
        let layer = noisy(100, 100);
        let mut patch = Patch::capture(&layer, PixelRect::new(0, 0, 100, 100));
        patch.keep_changed(&layer);
        assert!(patch.is_empty());
    }
}
