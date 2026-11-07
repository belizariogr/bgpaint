//! Flood fill ("Fill with color").

use crate::brush::StrokeMask;
use tiny_skia::Pixmap;

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
}
