//! Source-over compositing of premultiplied RGBA rows, with fast paths for the transparent and
//! opaque runs that make up most of a painting, and helpers to spread work over row bands on the
//! shared thread pool.

use crate::geom::PixelRect;
use rayon::prelude::*;
use tiny_skia::Pixmap;

/// A premultiplied RGBA pixel.
pub type Px = [u8; 4];

/// Pixels per chunk checked for a transparent or opaque run.
const RUN: usize = 16;

/// Below this many pixels, work stays on the calling thread.
pub const PARALLEL_MIN: usize = 1 << 18;

/// `v / 255`, rounded, for `v <= 255 * 255` (the same rounding as tiny-skia).
#[inline]
pub fn div255(v: u32) -> u32 {
    let v = v + 128;
    (v + (v >> 8)) >> 8
}

#[inline]
fn blend(d: &mut Px, s: &Px) {
    let inv = 255 - u32::from(s[3]);
    for c in 0..4 {
        d[c] = (u32::from(s[c]) + div255(u32::from(d[c]) * inv)).min(255) as u8;
    }
}

/// Draws `src` over `dst` (same length).
pub fn over_row(dst: &mut [Px], src: &[Px]) {
    debug_assert_eq!(dst.len(), src.len());
    for (d, s) in dst.chunks_mut(RUN).zip(src.chunks(RUN)) {
        let (mut all, mut any) = (255u8, 0u8);
        for p in s {
            all &= p[3];
            any |= p[3];
        }
        if any == 0 {
            continue;
        }
        if all == 255 {
            d.copy_from_slice(s);
            continue;
        }
        for (d, s) in d.iter_mut().zip(s) {
            match s[3] {
                0 => {}
                255 => *d = *s,
                _ => blend(d, s),
            }
        }
    }
}

/// Draws `src` faded to `opacity` (0–255) over `dst`.
pub fn over_row_faded(dst: &mut [Px], src: &[Px], opacity: u8) {
    if opacity == 255 {
        return over_row(dst, src);
    }
    if opacity == 0 {
        return;
    }
    let k = u32::from(opacity);
    for (d, s) in dst.iter_mut().zip(src) {
        if s[3] == 0 {
            continue;
        }
        let s = [0, 1, 2, 3].map(|c| div255(u32::from(s[c]) * k) as u8);
        blend(d, &s);
    }
}

/// Draws `src` with its top-left at (`x`, `y`) over `dst`, an image `width` pixels wide whose
/// first row is image row `y0` (a band of a larger image). Only the overlap is touched.
pub fn over_into(dst: &mut [Px], width: usize, y0: i32, src: &Pixmap, x: i32, y: i32, opacity: u8) {
    if width == 0 {
        return;
    }
    let rows = (dst.len() / width) as i32;
    let target =
        PixelRect::new(x, y, x + src.width() as i32, y + src.height() as i32).intersect(&PixelRect::new(0, y0, width as i32, y0 + rows));
    if target.is_empty() {
        return;
    }
    let (s, _) = src.data().as_chunks::<4>();
    let sw = src.width() as usize;
    let (cols, sx) = (target.width() as usize, (target.x0 - x) as usize);
    for ty in target.y0..target.y1 {
        let so = (ty - y) as usize * sw + sx;
        let d0 = (ty - y0) as usize * width + target.x0 as usize;
        over_row_faded(&mut dst[d0..d0 + cols], &s[so..so + cols], opacity);
    }
}

/// Opacity from 0.0–1.0 to 0–255.
pub fn opacity_u8(opacity: f32) -> u8 {
    (opacity.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Runs `f(first row, rows)` over bands of `rows_per_band` rows of an image `width` pixels wide,
/// in parallel when the image is large.
pub fn par_bands(data: &mut [Px], width: usize, rows_per_band: usize, f: impl Fn(usize, &mut [Px]) + Sync + Send) {
    if width == 0 {
        return;
    }
    let chunk = width * rows_per_band.max(1);
    if data.len() < PARALLEL_MIN || data.len() <= chunk {
        data.chunks_mut(chunk).enumerate().for_each(|(i, band)| f(i * rows_per_band, band));
    } else {
        data.par_chunks_mut(chunk).enumerate().for_each(|(i, band)| f(i * rows_per_band, band));
    }
}

/// Rows per band for an image `width` pixels wide: about 64K pixels each.
pub fn band_rows(width: usize) -> usize {
    (65_536 / width.max(1)).clamp(1, 256)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::{PixmapPaint, Transform};

    fn noise(w: u32, h: u32, seed: u32, opaque: bool) -> Pixmap {
        let mut p = Pixmap::new(w, h).unwrap();
        for (i, px) in p.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let v = (i as u32 ^ seed).wrapping_mul(2_654_435_761);
            let a = if opaque { 255 } else { [0, 255, (v >> 24) as u8][(v % 3) as usize] };
            let c = |s: u32| ((v >> s) as u8).min(a);
            *px = [c(0), c(8), c(16), a];
        }
        p
    }

    #[test]
    fn matches_tiny_skia() {
        for opacity in [1.0, 0.5, 0.2] {
            let mut ours = noise(300, 200, 1, true);
            let mut theirs = ours.clone();
            let src = noise(200, 150, 7, false);
            let paint = PixmapPaint { opacity, ..PixmapPaint::default() };
            theirs.draw_pixmap(-20, 70, src.as_ref(), &paint, Transform::identity(), None);
            let (d, _) = ours.data_mut().as_chunks_mut::<4>();
            over_into(d, 300, 0, &src, -20, 70, opacity_u8(opacity));
            let diff = ours.data().iter().zip(theirs.data()).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
            assert!(diff <= if opacity == 1.0 { 0 } else { 2 }, "opacity {opacity}: off by {diff}");
        }
    }

    #[test]
    fn bands_cover_every_row() {
        let mut p = Pixmap::new(700, 900).unwrap();
        let (d, _) = p.data_mut().as_chunks_mut::<4>();
        par_bands(d, 700, 7, |y0, band| {
            for (i, row) in band.chunks_mut(700).enumerate() {
                row.fill([(y0 + i) as u8, 0, 0, 255]);
            }
        });
        assert!(p.pixels().iter().enumerate().all(|(i, c)| c.red() == (i / 700) as u8));
    }
}
