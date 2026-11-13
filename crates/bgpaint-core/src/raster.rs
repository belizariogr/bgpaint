//! Whole-pixmap and region operations on premultiplied RGBA [`Pixmap`]s.

use crate::blend::{self, Px};
use crate::color::Color;
use crate::geom::PixelRect;
use rayon::prelude::*;
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

/// Largest supported canvas side, in pixels.
pub const MAX_SIDE: u32 = 30_000;
/// Largest supported canvas area, in pixels (~1.6 GB per RGBA layer would be absurd; cap at 400 MP).
pub const MAX_AREA: u64 = 400_000_000;

/// Error returned when an image would be too large to allocate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooLarge {
    pub width: u32,
    pub height: u32,
}

impl std::fmt::Display for TooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "image of {}×{} pixels is too large", self.width, self.height)
    }
}

impl std::error::Error for TooLarge {}

/// Allocates a transparent pixmap, validating the size.
pub fn try_pixmap(width: u32, height: u32) -> Result<Pixmap, TooLarge> {
    let err = TooLarge { width, height };
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(err);
    }
    if u64::from(width) * u64::from(height) > MAX_AREA {
        return Err(err);
    }
    Pixmap::new(width, height).ok_or(err)
}

/// Allocates a pixmap whose size is already known to be valid (clamped to at least 1×1).
pub fn pixmap(width: u32, height: u32) -> Pixmap {
    let (w, h) = (width.clamp(1, MAX_SIDE), height.clamp(1, MAX_SIDE));
    Pixmap::new(w, h).unwrap_or_else(|| panic!("failed to allocate {w}×{h} pixmap"))
}

/// Bounds of a pixmap as a rect.
pub fn bounds(p: &Pixmap) -> PixelRect {
    PixelRect::from_size(p.width(), p.height())
}

/// Copies a region (clamped to the pixmap) into a new pixmap. Returns `None` if the clamped region is empty.
pub fn copy_region(src: &Pixmap, r: PixelRect) -> Option<Pixmap> {
    let r = r.intersect(&bounds(src));
    if r.is_empty() {
        return None;
    }
    let mut out = pixmap(r.width(), r.height());
    let sw = src.width() as usize * 4;
    let row = r.width() as usize * 4;
    let s = src.data();
    let d = out.data_mut();
    for (i, y) in (r.y0..r.y1).enumerate() {
        let so = y as usize * sw + r.x0 as usize * 4;
        d[i * row..(i + 1) * row].copy_from_slice(&s[so..so + row]);
    }
    Some(out)
}

/// Iterates over matching row slices of `dst` (at `x`,`y`) and `src`, clipped to `dst`.
fn for_each_row_pair(dst: &mut Pixmap, src: &Pixmap, x: i32, y: i32, mut f: impl FnMut(&mut [u8], &[u8])) {
    let target = PixelRect::from_xywh(x, y, src.width(), src.height()).intersect(&bounds(dst));
    if target.is_empty() {
        return;
    }
    let dw = dst.width() as usize * 4;
    let sw = src.width() as usize * 4;
    let len = target.width() as usize * 4;
    let sx = (target.x0 - x) as usize * 4;
    let s = src.data();
    let d = dst.data_mut();
    for ty in target.y0..target.y1 {
        let sy = (ty - y) as usize;
        let doff = ty as usize * dw + target.x0 as usize * 4;
        let soff = sy * sw + sx;
        f(&mut d[doff..doff + len], &s[soff..soff + len]);
    }
}

/// Replaces pixels of `dst` at (`x`,`y`) with `src` (no blending).
pub fn paste_region(dst: &mut Pixmap, src: &Pixmap, x: i32, y: i32) {
    for_each_row_pair(dst, src, x, y, |d, s| d.copy_from_slice(s));
}

/// Swaps the pixels of `buf` with the region of `dst` at (`x`,`y`). `buf` must lie inside `dst`.
pub fn swap_region(dst: &mut Pixmap, buf: &mut Pixmap, x: i32, y: i32) {
    debug_assert!(
        bounds(dst).intersect(&PixelRect::from_xywh(x, y, buf.width(), buf.height())).area()
            == u64::from(buf.width()) * u64::from(buf.height())
    );
    let dw = dst.width() as usize * 4;
    let bw = buf.width() as usize * 4;
    let rows = buf.height() as usize;
    let d = dst.data_mut();
    let b = buf.data_mut();
    for row in 0..rows {
        let doff = (y as usize + row) * dw + x as usize * 4;
        d[doff..doff + bw].swap_with_slice(&mut b[row * bw..(row + 1) * bw]);
    }
}

/// Composites `src` over `dst` at an integer offset.
pub fn draw_over(dst: &mut Pixmap, src: &Pixmap, x: i32, y: i32, opacity: f32) {
    let w = dst.width() as usize;
    let rows = PixelRect::new(0, y, 1, y + src.height() as i32).intersect(&PixelRect::new(0, 0, 1, dst.height() as i32));
    if rows.is_empty() {
        return;
    }
    let op = blend::opacity_u8(opacity);
    let (d, _) = dst.data_mut().as_chunks_mut::<4>();
    let first = rows.y0 as usize;
    let part = &mut d[first * w..rows.y1 as usize * w];
    blend::par_bands(part, w, blend::band_rows(w), |r0, band| blend::over_into(band, w, (first + r0) as i32, src, x, y, op));
}

/// Draws `src` transformed onto `dst` with smooth filtering.
pub fn draw_transformed(dst: &mut Pixmap, src: &Pixmap, transform: Transform, smooth: bool) {
    let paint = PixmapPaint { quality: if smooth { FilterQuality::Bilinear } else { FilterQuality::Nearest }, ..PixmapPaint::default() };
    dst.draw_pixmap(0, 0, src.as_ref(), &paint, transform, None);
}

/// Makes a region fully transparent.
pub fn clear_region(dst: &mut Pixmap, r: PixelRect) {
    fill_region(dst, r, Color::TRANSPARENT);
}

/// Fills a region with a color, replacing existing pixels.
pub fn fill_region(dst: &mut Pixmap, r: PixelRect, color: Color) {
    let r = r.intersect(&bounds(dst));
    if r.is_empty() {
        return;
    }
    let p = color.premultiplied();
    let w = dst.width() as usize * 4;
    let d = dst.data_mut();
    for y in r.y0..r.y1 {
        let row = &mut d[y as usize * w + r.x0 as usize * 4..y as usize * w + r.x1 as usize * 4];
        for px in row.as_chunks_mut::<4>().0.iter_mut() {
            px.copy_from_slice(&p);
        }
    }
}

/// Reads a pixel as a straight-alpha color.
pub fn get_pixel(p: &Pixmap, x: i32, y: i32) -> Option<Color> {
    if x < 0 || y < 0 {
        return None;
    }
    let c = p.pixel(x as u32, y as u32)?;
    Some(Color::from_premultiplied([c.red(), c.green(), c.blue(), c.alpha()]))
}

pub fn flip_horizontal(p: &mut Pixmap) {
    let w = p.width() as usize;
    let (d, _) = p.data_mut().as_chunks_mut::<4>();
    d.par_chunks_mut(w).for_each(<[Px]>::reverse);
}

pub fn flip_vertical(p: &mut Pixmap) {
    let w = p.width() as usize;
    let h = p.height() as usize;
    let (d, _) = p.data_mut().as_chunks_mut::<4>();
    let (top, rest) = d.split_at_mut(h / 2 * w);
    let bottom = &mut rest[(h % 2) * w..];
    top.par_chunks_mut(w).zip(bottom.par_chunks_mut(w).rev()).for_each(|(a, b)| a.swap_with_slice(b));
}

/// Rotation by a multiple of 90 degrees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuarterTurn {
    Cw90,
    Ccw90,
    Half,
}

impl QuarterTurn {
    pub fn inverse(self) -> Self {
        match self {
            QuarterTurn::Cw90 => QuarterTurn::Ccw90,
            QuarterTurn::Ccw90 => QuarterTurn::Cw90,
            QuarterTurn::Half => QuarterTurn::Half,
        }
    }
}

pub fn rotate_quarter(src: &Pixmap, turn: QuarterTurn) -> Pixmap {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let (nw, nh) = match turn {
        QuarterTurn::Half => (w, h),
        _ => (h, w),
    };
    let mut out = pixmap(nw as u32, nh as u32);
    let (s, _) = src.data().as_chunks::<4>();
    let (d, _) = out.data_mut().as_chunks_mut::<4>();
    // Bands of output rows; within a band, each source row is read in one contiguous run.
    const BAND: usize = 32;
    d.par_chunks_mut(nw * BAND).enumerate().for_each(|(b, band)| {
        let ny0 = b * BAND;
        let rows = band.len() / nw;
        match turn {
            QuarterTurn::Half => {
                for (i, row) in band.chunks_exact_mut(nw).enumerate() {
                    let sy = h - 1 - (ny0 + i);
                    row.copy_from_slice(&s[sy * w..(sy + 1) * w]);
                    row.reverse();
                }
            }
            // out(nx, ny) = src(ny, h - 1 - nx)
            QuarterTurn::Cw90 => {
                for nx in 0..nw {
                    let sy = h - 1 - nx;
                    let src_row = &s[sy * w + ny0..sy * w + ny0 + rows];
                    for (i, px) in src_row.iter().enumerate() {
                        band[i * nw + nx] = *px;
                    }
                }
            }
            // out(nx, ny) = src(w - 1 - ny, nx)
            QuarterTurn::Ccw90 => {
                for nx in 0..nw {
                    let src_row = &s[nx * w..(nx + 1) * w];
                    for i in 0..rows {
                        band[i * nw + nx] = src_row[w - 1 - (ny0 + i)];
                    }
                }
            }
        }
    });
    out
}

/// A lossless change of orientation of the whole image, which undo can reverse exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reorient {
    FlipHorizontal,
    FlipVertical,
    Rotate(QuarterTurn),
}

impl Reorient {
    pub fn inverse(self) -> Self {
        match self {
            Reorient::Rotate(t) => Reorient::Rotate(t.inverse()),
            flip => flip,
        }
    }

    /// How it maps a `w`×`h` image.
    pub fn transform(self, w: u32, h: u32) -> Transform {
        match self {
            Reorient::FlipHorizontal => flip_transform(w, h, true),
            Reorient::FlipVertical => flip_transform(w, h, false),
            Reorient::Rotate(t) => rotate_quarter_transform(w, h, t),
        }
    }

    /// Size of a `w`×`h` image once reoriented.
    pub fn size(self, w: u32, h: u32) -> (u32, u32) {
        match self {
            Reorient::Rotate(QuarterTurn::Cw90 | QuarterTurn::Ccw90) => (h, w),
            _ => (w, h),
        }
    }

    /// Reorients `p`, in place when the size stays.
    pub fn apply(self, p: &mut Pixmap) {
        match self {
            Reorient::FlipHorizontal => flip_horizontal(p),
            Reorient::FlipVertical => flip_vertical(p),
            Reorient::Rotate(t) => *p = rotate_quarter(p, t),
        }
    }
}

/// Smoothly resamples a pixmap to a new size.
pub fn resize(src: &Pixmap, width: u32, height: u32) -> Pixmap {
    let (width, height) = (width.clamp(1, MAX_SIDE), height.clamp(1, MAX_SIDE));
    if width == src.width() && height == src.height() {
        return src.clone();
    }
    // Filtering premultiplied data is the correct way to resample images with alpha.
    let img =
        image::RgbaImage::from_raw(src.width(), src.height(), src.data().to_vec()).expect("pixmap buffer size matches its dimensions");
    let resized = image::imageops::resize(&img, width, height, image::imageops::FilterType::Triangle);
    let mut out = pixmap(width, height);
    out.data_mut().copy_from_slice(resized.as_raw());
    fix_premultiplied(&mut out);
    out
}

/// Fast downscale for thumbnails: averages up to 4×4 samples per destination pixel.
pub fn thumbnail(src: &Pixmap, width: u32, height: u32) -> Pixmap {
    let (width, height) = (width.max(1), height.max(1));
    let mut out = pixmap(width, height);
    let all = bounds(&out);
    thumbnail_into(&mut out, src, (0, 0), (src.width(), src.height()), all);
    out
}

/// Recomputes the pixels of `out` (a thumbnail of a `full`-sized image, as [`thumbnail`] makes
/// it) inside `area`, sampling `src`, which holds the image's pixels from `origin` on. Samples
/// falling outside `src` are clamped to its edge.
pub fn thumbnail_into(out: &mut Pixmap, src: &Pixmap, origin: (i32, i32), full: (u32, u32), area: PixelRect) {
    let area = area.intersect(&bounds(out));
    if area.is_empty() {
        return;
    }
    let width = out.width() as usize;
    let (fx, fy) = (full.0 as f32 / out.width() as f32, full.1 as f32 / out.height() as f32);
    let n = 4usize;
    let s = src.data();
    let stride = src.width() as usize * 4;
    let (max_x, max_y) = (src.width() as i32 - 1, src.height() as i32 - 1);
    let d = out.data_mut();
    for dy in area.y0..area.y1 {
        for dx in area.x0..area.x1 {
            let mut acc = [0u32; 4];
            for sy in 0..n {
                let y = (((dy as f32 + (sy as f32 + 0.5) / n as f32) * fy) as i32 - origin.1).clamp(0, max_y) as usize;
                for sx in 0..n {
                    let x = (((dx as f32 + (sx as f32 + 0.5) / n as f32) * fx) as i32 - origin.0).clamp(0, max_x) as usize;
                    let o = y * stride + x * 4;
                    for c in 0..4 {
                        acc[c] += u32::from(s[o + c]);
                    }
                }
            }
            let o = (dy as usize * width + dx as usize) * 4;
            for c in 0..4 {
                d[o + c] = (acc[c] / (n * n) as u32) as u8;
            }
        }
    }
}

/// Clamps color channels to alpha so a filtered buffer stays a valid premultiplied image.
pub fn fix_premultiplied(p: &mut Pixmap) {
    for px in p.data_mut().as_chunks_mut::<4>().0.iter_mut() {
        let a = px[3];
        px[0] = px[0].min(a);
        px[1] = px[1].min(a);
        px[2] = px[2].min(a);
    }
}

/// Applies an arbitrary affine transform, returning the transformed image on a canvas that fits it
/// and the offset of that canvas relative to the source origin.
pub fn transform_fit(src: &Pixmap, t: Transform) -> (Pixmap, i32, i32) {
    let (shifted, r) = fit_transform(src.width(), src.height(), t);
    let mut out = pixmap(r.width(), r.height());
    draw_transformed(&mut out, src, shifted, true);
    (out, r.x0, r.y0)
}

/// `t` followed by the shift that brings a transformed `w`×`h` image to (0,0), and the area the
/// image covers before that shift.
pub fn fit_transform(w: u32, h: u32, t: Transform) -> (Transform, PixelRect) {
    let (w, h) = (w as f32, h as f32);
    let mut pts = [
        tiny_skia::Point::from_xy(0.0, 0.0),
        tiny_skia::Point::from_xy(w, 0.0),
        tiny_skia::Point::from_xy(0.0, h),
        tiny_skia::Point::from_xy(w, h),
    ];
    t.map_points(&mut pts);
    let min_x = pts.iter().map(|p| p.x).fold(f32::MAX, f32::min);
    let min_y = pts.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    let max_x = pts.iter().map(|p| p.x).fold(f32::MIN, f32::max);
    let max_y = pts.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    let r = PixelRect::enclosing(min_x, min_y, max_x, max_y);
    (t.post_translate(-r.x0 as f32, -r.y0 as f32), r)
}

/// Rotates by an arbitrary angle (degrees, clockwise) around the center, growing the canvas to fit.
pub fn rotate_degrees(src: &Pixmap, degrees: f32) -> Pixmap {
    let (cx, cy) = (src.width() as f32 / 2.0, src.height() as f32 / 2.0);
    transform_fit(src, Transform::from_rotate_at(degrees, cx, cy)).0
}

/// How [`rotate_degrees`] maps a `w`×`h` image.
pub fn rotate_degrees_transform(w: u32, h: u32, degrees: f32) -> Transform {
    fit_transform(w, h, Transform::from_rotate_at(degrees, w as f32 / 2.0, h as f32 / 2.0)).0
}

/// Skews horizontally and vertically by the given angles in degrees, growing the canvas to fit.
pub fn skew_degrees(src: &Pixmap, deg_x: f32, deg_y: f32) -> Pixmap {
    transform_fit(src, skew(deg_x, deg_y)).0
}

/// How [`skew_degrees`] maps a `w`×`h` image.
pub fn skew_degrees_transform(w: u32, h: u32, deg_x: f32, deg_y: f32) -> Transform {
    fit_transform(w, h, skew(deg_x, deg_y)).0
}

fn skew(deg_x: f32, deg_y: f32) -> Transform {
    Transform::from_row(1.0, deg_y.to_radians().tan(), deg_x.to_radians().tan(), 1.0, 0.0, 0.0)
}

/// How [`rotate_quarter`] maps a `w`×`h` image.
pub fn rotate_quarter_transform(w: u32, h: u32, turn: QuarterTurn) -> Transform {
    let (w, h) = (w as f32, h as f32);
    match turn {
        QuarterTurn::Cw90 => Transform::from_row(0.0, 1.0, -1.0, 0.0, h, 0.0),
        QuarterTurn::Ccw90 => Transform::from_row(0.0, -1.0, 1.0, 0.0, 0.0, w),
        QuarterTurn::Half => Transform::from_row(-1.0, 0.0, 0.0, -1.0, w, h),
    }
}

/// How [`flip_horizontal`] (or [`flip_vertical`]) maps a `w`×`h` image.
pub fn flip_transform(w: u32, h: u32, horizontal: bool) -> Transform {
    if horizontal {
        Transform::from_row(-1.0, 0.0, 0.0, 1.0, w as f32, 0.0)
    } else {
        Transform::from_row(1.0, 0.0, 0.0, -1.0, 0.0, h as f32)
    }
}

/// Inverts the colors of every pixel (keeps alpha).
pub fn invert_colors(p: &mut Pixmap) {
    for px in p.data_mut().as_chunks_mut::<4>().0.iter_mut() {
        let a = px[3];
        px[0] = a - px[0];
        px[1] = a - px[1];
        px[2] = a - px[2];
    }
}

/// Creates a canvas of a new size with the old content placed at (`dx`,`dy`).
pub fn resize_canvas(src: &Pixmap, width: u32, height: u32, dx: i32, dy: i32) -> Pixmap {
    let mut out = pixmap(width, height);
    paste_region(&mut out, src, dx, dy);
    out
}

/// True if any pixel is not fully opaque.
pub fn has_transparency(p: &Pixmap) -> bool {
    has_transparency_par(p)
}

/// True if every pixel is fully transparent.
pub fn is_fully_transparent(p: &Pixmap) -> bool {
    p.data().as_chunks::<4>().0.iter().all(|px| px[3] == 0)
}

/// Tight bounds of the non transparent pixels.
pub fn opaque_bounds(p: &Pixmap) -> PixelRect {
    let w = p.width() as usize;
    let mut r = PixelRect::EMPTY;
    for (y, row) in p.data().chunks_exact(w * 4).enumerate() {
        let first = row.as_chunks::<4>().0.iter().position(|px| px[3] != 0);
        if let Some(first) = first {
            let last = row.as_chunks::<4>().0.iter().rposition(|px| px[3] != 0).unwrap_or(first);
            r = r.union(&PixelRect::new(first as i32, y as i32, last as i32 + 1, y as i32 + 1));
        }
    }
    r
}

/// Straight-alpha RGBA bytes of the whole pixmap.
pub fn to_straight_rgba(p: &Pixmap) -> Vec<u8> {
    let mut out = vec![0u8; p.data().len()];
    let (src, _) = p.data().as_chunks::<4>();
    let (dst, _) = out.as_chunks_mut::<4>();
    dst.par_chunks_mut(1 << 14).zip(src.par_chunks(1 << 14)).for_each(|(d, s)| {
        for (d, s) in d.iter_mut().zip(s) {
            *d = demultiply(*s);
        }
    });
    out
}

/// A premultiplied pixel with straight alpha.
#[inline]
pub fn demultiply(p: Px) -> Px {
    match p[3] {
        0 => [0; 4],
        255 => p,
        a => {
            let a = u32::from(a);
            let un = |c: u8| ((u32::from(c) * 255 + a / 2) / a).min(255) as u8;
            [un(p[0]), un(p[1]), un(p[2]), p[3]]
        }
    }
}

/// Builds a premultiplied pixmap from straight-alpha RGBA bytes.
pub fn from_straight_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Pixmap, TooLarge> {
    from_straight_rgba_vec(width, height, rgba.to_vec())
}

/// Like [`from_straight_rgba`], premultiplying the buffer in place (no copy).
pub fn from_straight_rgba_vec(width: u32, height: u32, mut rgba: Vec<u8>) -> Result<Pixmap, TooLarge> {
    let err = TooLarge { width, height };
    try_pixmap(width, height)?;
    let (px, _) = rgba.as_chunks_mut::<4>();
    px.par_chunks_mut(1 << 14).for_each(|c| {
        for p in c {
            if p[3] != 255 {
                *p = Color::rgba(p[0], p[1], p[2], p[3]).premultiplied();
            }
        }
    });
    let size = tiny_skia::IntSize::from_wh(width, height).ok_or(err)?;
    Pixmap::from_vec(rgba, size).ok_or(err)
}

/// True if any pixel is not fully opaque.
pub fn has_transparency_par(p: &Pixmap) -> bool {
    p.data().as_chunks::<4>().0.par_chunks(1 << 16).any(|c| c.iter().any(|px| px[3] != 255))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_thumbnail_matches_full_one() {
        let src = numbered(301, 157);
        let full = thumbnail(&src, 76, 40);
        let mut patched = thumbnail(&pixmap(301, 157), 76, 40);
        // The thumbnail pixels of an area, recomputed from a copy of just the source they sample.
        let area = PixelRect::new(10, 5, 31, 22);
        let (fx, fy) = (301.0 / 76.0, 157.0 / 40.0);
        let r = PixelRect::new(
            (area.x0 as f32 * fx) as i32,
            (area.y0 as f32 * fy) as i32,
            (area.x1 as f32 * fx).ceil() as i32,
            (area.y1 as f32 * fy).ceil() as i32,
        );
        let region = copy_region(&src, r).unwrap();
        thumbnail_into(&mut patched, &region, (r.x0, r.y0), (301, 157), area);
        assert_eq!(copy_region(&full, area).unwrap().data(), copy_region(&patched, area).unwrap().data());
    }

    fn numbered(w: u32, h: u32) -> Pixmap {
        let mut p = pixmap(w, h);
        for (i, px) in p.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
            px.copy_from_slice(&[i as u8, i as u8, i as u8, 255]);
        }
        p
    }

    fn at(p: &Pixmap, x: u32, y: u32) -> u8 {
        p.pixel(x, y).unwrap().red()
    }

    #[test]
    fn copy_paste_swap() {
        let src = numbered(4, 4);
        let part = copy_region(&src, PixelRect::new(1, 1, 3, 3)).unwrap();
        assert_eq!((part.width(), part.height()), (2, 2));
        assert_eq!(at(&part, 0, 0), 5);
        let mut dst = pixmap(4, 4);
        paste_region(&mut dst, &part, 3, 3);
        assert_eq!(at(&dst, 3, 3), 5);
        let mut buf = pixmap(2, 2);
        let mut dst2 = src.clone();
        swap_region(&mut dst2, &mut buf, 0, 0);
        assert_eq!(dst2.pixel(0, 0).unwrap().alpha(), 0);
        assert_eq!(at(&buf, 1, 1), 5);
    }

    #[test]
    fn rotations_and_flips() {
        let src = numbered(3, 2); // 0 1 2 / 3 4 5
        let cw = rotate_quarter(&src, QuarterTurn::Cw90);
        assert_eq!((cw.width(), cw.height()), (2, 3));
        assert_eq!(at(&cw, 0, 0), 3);
        assert_eq!(at(&cw, 1, 0), 0);
        let ccw = rotate_quarter(&src, QuarterTurn::Ccw90);
        assert_eq!(at(&ccw, 0, 0), 2);
        let half = rotate_quarter(&src, QuarterTurn::Half);
        assert_eq!(at(&half, 0, 0), 5);
        let mut f = src.clone();
        flip_horizontal(&mut f);
        assert_eq!(at(&f, 0, 0), 2);
        let mut v = src.clone();
        flip_vertical(&mut v);
        assert_eq!(at(&v, 0, 0), 3);
    }

    #[test]
    fn resize_and_bounds() {
        let mut p = pixmap(10, 10);
        fill_region(&mut p, PixelRect::new(2, 3, 5, 7), Color::BLACK);
        assert_eq!(opaque_bounds(&p), PixelRect::new(2, 3, 5, 7));
        let r = resize(&p, 20, 5);
        assert_eq!((r.width(), r.height()), (20, 5));
        assert!(has_transparency(&p));
        assert!(try_pixmap(0, 10).is_err());
        assert!(try_pixmap(MAX_SIDE + 1, 10).is_err());
    }

    #[test]
    fn straight_roundtrip() {
        let rgba = [255u8, 0, 0, 128, 0, 0, 0, 0];
        let p = from_straight_rgba(2, 1, &rgba).unwrap();
        let back = to_straight_rgba(&p);
        assert!(back[0] >= 254 && back[3] == 128 && back[7] == 0);
    }
}
