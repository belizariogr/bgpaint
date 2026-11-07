//! Flood fill ("Fill with color").

use crate::brush::StrokeMask;
use crate::color::Color;
use crate::geom::PixelRect;
use tiny_skia::Pixmap;

/// A flood filled region: the matched area plus the seed color it grew from.
pub struct Flood {
    pub mask: StrokeMask,
    pub seed: [u8; 4],
}

impl Flood {
    /// Grows the region around (`x`,`y`); see [`flood_mask`]. `None` outside the image.
    pub fn at(p: &Pixmap, x: i32, y: i32, tolerance: u8) -> Option<Self> {
        let i = (y.max(0) as usize * p.width() as usize + x.max(0) as usize) * 4;
        let mask = flood_mask(p, x, y, tolerance)?;
        let d = p.data();
        Some(Self { mask, seed: [d[i], d[i + 1], d[i + 2], d[i + 3]] })
    }

    /// Pixels [`Flood::paint`] may change: the region plus its one pixel fringe (corners included).
    pub fn bounds(&self) -> PixelRect {
        self.mask.bounds().expand(1).clamp_to(self.mask.width(), self.mask.height())
    }

    /// Replaces the region with `color` and recolors the anti-aliased fringe around it.
    ///
    /// Edges drawn with anti-aliasing have a ring of pixels that mix the seed color with the
    /// outline. They do not match the seed, so a plain fill leaves them as a light halo inside
    /// the outline. Each fringe pixel is modeled as `a * seed + (1 - a) * line`, where `line` is
    /// the farthest-from-seed color next to it, and the seed share is swapped for `color`.
    pub fn paint(&self, p: &mut Pixmap, color: Color) {
        let (w, h) = (p.width() as i32, p.height() as i32);
        let fill = color.premultiplied().map(f32::from);
        let seed = self.seed.map(f32::from);
        let in_mask = |x: i32, y: i32| self.mask.get(x, y) != 0;
        let area = self.bounds();

        // Compute the fringe from the untouched pixels before painting anything.
        let mut fringe = Vec::new();
        {
            let data = p.data();
            let px = |x: i32, y: i32| -> [f32; 4] {
                let i = (y as usize * w as usize + x as usize) * 4;
                [data[i], data[i + 1], data[i + 2], data[i + 3]].map(f32::from)
            };
            let dist2 = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b.iter()).map(|(a, b)| (a - b) * (a - b)).sum::<f32>();
            for y in area.y0..area.y1 {
                for x in area.x0..area.x1 {
                    if in_mask(x, y) {
                        continue;
                    }
                    let here = px(x, y);
                    let edge = [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|(dx, dy)| in_mask(x + dx, y + dy));
                    // A pixel touching the region only diagonally (the inside of a sharp corner)
                    // belongs to the fringe unless a line runs between them: at least one of the
                    // two pixels it shares with that diagonal must be no farther from the seed.
                    let corner = || {
                        let d = dist2(here, seed);
                        [(-1, -1), (1, -1), (-1, 1), (1, 1)].iter().any(|&(dx, dy)| {
                            in_mask(x + dx, y + dy) && (dist2(px(x + dx, y), seed) <= d || dist2(px(x, y + dy), seed) <= d)
                        })
                    };
                    if !edge && !corner() {
                        continue;
                    }
                    // The outline color: the neighbor (outside the region) farthest from the seed.
                    let mut line = here;
                    let mut far = dist2(here, seed);
                    for ny in (y - 1).max(0)..=(y + 1).min(h - 1) {
                        for nx in (x - 1).max(0)..=(x + 1).min(w - 1) {
                            if in_mask(nx, ny) {
                                continue;
                            }
                            let c = px(nx, ny);
                            let d = dist2(c, seed);
                            if d > far {
                                far = d;
                                line = c;
                            }
                        }
                    }
                    if far < 1.0 {
                        continue;
                    }
                    // Seed share of this pixel: its projection on the line -> seed segment.
                    let dot: f32 = (0..4).map(|c| (here[c] - line[c]) * (seed[c] - line[c])).sum();
                    let a = (dot / far).clamp(0.0, 1.0);
                    if a <= 0.0 {
                        continue;
                    }
                    let mut out = [0u8; 4];
                    for c in 0..4 {
                        out[c] = (here[c] + a * (fill[c] - seed[c])).round().clamp(0.0, 255.0) as u8;
                    }
                    // Keep the pixel valid premultiplied color.
                    for c in 0..3 {
                        out[c] = out[c].min(out[3]);
                    }
                    fringe.push((x, y, out));
                }
            }
        }

        let pm = color.premultiplied();
        let data = p.data_mut();
        let b = self.mask.bounds();
        for y in b.y0..b.y1 {
            for x in b.x0..b.x1 {
                if in_mask(x, y) {
                    let i = (y as usize * w as usize + x as usize) * 4;
                    data[i..i + 4].copy_from_slice(&pm);
                }
            }
        }
        for (x, y, out) in fringe {
            let i = (y as usize * w as usize + x as usize) * 4;
            data[i..i + 4].copy_from_slice(&out);
        }
    }
}

/// Computes the contiguous area around (`x`,`y`) whose pixels match the seed pixel within
/// `tolerance` (max channel difference, premultiplied). Returns `None` outside the image.
pub fn flood_mask(p: &Pixmap, x: i32, y: i32, tolerance: u8) -> Option<StrokeMask> {
    let (w, h) = (p.width() as i32, p.height() as i32);
    if x < 0 || y < 0 || x >= w || y >= h {
        return None;
    }
    let data = p.data();
    let px = |x: i32, y: i32| -> [u8; 4] {
        let i = (y as usize * w as usize + x as usize) * 4;
        [data[i], data[i + 1], data[i + 2], data[i + 3]]
    };
    let seed = px(x, y);
    let matches = |c: [u8; 4]| c.iter().zip(seed.iter()).all(|(a, b)| a.abs_diff(*b) <= tolerance);

    let mut mask = StrokeMask::new(p.width(), p.height());
    let mut visited = vec![false; w as usize * h as usize];
    let mut stack = vec![(x, y)];
    let mut min = (x, y);
    let mut max = (x, y);
    while let Some((sx, sy)) = stack.pop() {
        if visited[sy as usize * w as usize + sx as usize] {
            continue;
        }
        // Expand left and right along the scanline.
        let mut l = sx;
        while l > 0 && !visited[sy as usize * w as usize + (l - 1) as usize] && matches(px(l - 1, sy)) {
            l -= 1;
        }
        let mut r = sx;
        while r + 1 < w && !visited[sy as usize * w as usize + (r + 1) as usize] && matches(px(r + 1, sy)) {
            r += 1;
        }
        let row = mask.row_mut(sy);
        for cx in l..=r {
            visited[sy as usize * w as usize + cx as usize] = true;
            row[cx as usize] = 255;
        }
        min = (min.0.min(l), min.1.min(sy));
        max = (max.0.max(r), max.1.max(sy));
        for ny in [sy - 1, sy + 1] {
            if ny < 0 || ny >= h {
                continue;
            }
            let mut cx = l;
            while cx <= r {
                let idx = ny as usize * w as usize + cx as usize;
                if !visited[idx] && matches(px(cx, ny)) {
                    stack.push((cx, ny));
                    // Skip the rest of this run; it will be expanded from the pushed seed.
                    while cx <= r && matches(px(cx, ny)) {
                        cx += 1;
                    }
                } else {
                    cx += 1;
                }
            }
        }
    }
    mask.include(crate::geom::PixelRect::new(min.0, min.1, max.0 + 1, max.1 + 1));
    Some(mask)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::geom::PixelRect;
    use crate::raster;

    #[test]
    fn fills_enclosed_region_only() {
        let mut p = raster::pixmap(10, 10);
        // A vertical wall at x = 5.
        raster::fill_region(&mut p, PixelRect::new(5, 0, 6, 10), Color::BLACK);
        let m = flood_mask(&p, 1, 1, 0).unwrap();
        assert_eq!(m.bounds(), PixelRect::new(0, 0, 5, 10));
        assert_eq!(m.get(4, 9), 255);
        assert_eq!(m.get(5, 5), 0);
        assert_eq!(m.get(7, 5), 0);
        assert!(flood_mask(&p, -1, 0, 0).is_none());
    }

    #[test]
    fn fills_around_obstacles() {
        let mut p = raster::pixmap(8, 8);
        raster::fill_region(&mut p, PixelRect::new(2, 2, 6, 6), Color::BLACK);
        let m = flood_mask(&p, 0, 0, 0).unwrap();
        let filled = (0..8).flat_map(|y| (0..8).map(move |x| (x, y))).filter(|&(x, y)| m.get(x, y) == 255).count();
        assert_eq!(filled, 64 - 16);
    }

    #[test]
    fn paint_recolors_antialiased_fringe() {
        use tiny_skia::{Paint, PathBuilder, Stroke, Transform};
        let mut p = raster::pixmap(60, 60);
        raster::fill_region(&mut p, PixelRect::new(0, 0, 60, 60), Color::WHITE);
        let mut pb = PathBuilder::new();
        pb.move_to(8.3, 5.1);
        pb.line_to(52.7, 9.6);
        pb.line_to(31.2, 54.4);
        pb.line_to(9.1, 40.2);
        pb.close();
        let path = pb.finish().unwrap();
        let mut paint = Paint::default();
        paint.set_color(Color::BLACK.to_skia());
        paint.anti_alias = true;
        p.stroke_path(&path, &paint, &Stroke { width: 2.0, ..Stroke::default() }, Transform::identity(), None);

        let red = Color::rgb(200, 0, 20);
        let flood = Flood::at(&p, 30, 25, 0).unwrap();
        flood.paint(&mut p, red);

        // Inside the outline nothing should stay lighter than both the fill and the outline:
        // with a plain fill, the anti-aliased ring kept a white-ish halo.
        let d = p.data();
        for y in 0..60 {
            for x in 0..60 {
                let touches = (-1..=1).any(|dy| (-1..=1).any(|dx| flood.mask.get(x + dx, y + dy) != 0));
                if flood.mask.get(x, y) != 0 || !touches {
                    continue;
                }
                let i = (y as usize * 60 + x as usize) * 4;
                assert!(d[i + 1] < 40 && d[i + 2] < 60, "halo at {x},{y}: {:?}", &d[i..i + 4]);
            }
        }
        // Outside stays white.
        assert_eq!(&d[0..4], &[255, 255, 255, 255]);
    }

    #[test]
    fn paint_fringe_does_not_cross_diagonal_line() {
        // An aliased diagonal wall: the far side touches the region only diagonally.
        let mut p = raster::pixmap(6, 6);
        raster::fill_region(&mut p, PixelRect::new(0, 0, 6, 6), Color::WHITE);
        for i in 0..6 {
            raster::fill_region(&mut p, PixelRect::new(i, i, i + 1, i + 1), Color::BLACK);
        }
        let flood = Flood::at(&p, 5, 0, 0).unwrap();
        flood.paint(&mut p, Color::rgb(255, 0, 0));
        let d = p.data();
        let px = |x: usize, y: usize| &d[(y * 6 + x) * 4..(y * 6 + x) * 4 + 4];
        assert_eq!(px(5, 0), &[255, 0, 0, 255]);
        assert_eq!(px(0, 1), &[255, 255, 255, 255]);
        assert_eq!(px(3, 4), &[255, 255, 255, 255]);
    }

    #[test]
    fn paint_fringe_on_transparent_layer() {
        // Seed transparent, outline half covered black: the fringe becomes fill over the line.
        let mut p = raster::pixmap(3, 1);
        raster::fill_region(&mut p, PixelRect::new(1, 0, 2, 1), Color::rgba(0, 0, 0, 128));
        raster::fill_region(&mut p, PixelRect::new(2, 0, 3, 1), Color::BLACK);
        let flood = Flood::at(&p, 0, 0, 0).unwrap();
        flood.paint(&mut p, Color::rgb(255, 0, 0));
        let d = p.data();
        assert_eq!(&d[0..4], &[255, 0, 0, 255]);
        assert_eq!(d[7], 255);
        assert!((120..=135).contains(&d[4]), "{:?}", &d[4..8]);
        assert_eq!(&d[8..12], &[0, 0, 0, 255]);
    }
}
