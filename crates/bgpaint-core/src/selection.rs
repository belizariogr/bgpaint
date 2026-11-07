//! Selection masks: which pixels of the canvas a selection covers.

use crate::color::Color;
use crate::geom::{PixelRect, Point};
use crate::raster;
use tiny_skia::{FillRule, Mask, PathBuilder, Pixmap, Transform};

/// Coverage of a selection inside its bounding rectangle (255 = selected).
#[derive(Clone, Debug, PartialEq)]
pub struct SelectionMask {
    pub rect: PixelRect,
    /// `None` means every pixel of `rect` is selected.
    data: Option<Vec<u8>>,
}

impl SelectionMask {
    /// A rectangular selection.
    pub fn rect(rect: PixelRect) -> Self {
        Self { rect, data: None }
    }

    /// A free-form (lasso) selection, clipped to the canvas. Edges are aliased, as in Paint.
    pub fn polygon(points: &[Point], canvas_w: u32, canvas_h: u32) -> Option<Self> {
        if points.len() < 3 {
            return None;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in points {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        let rect = PixelRect::enclosing(x0, y0, x1, y1).clamp_to(canvas_w, canvas_h);
        if rect.is_empty() {
            return None;
        }
        let mut pb = PathBuilder::new();
        pb.move_to(points[0].x - rect.x0 as f32, points[0].y - rect.y0 as f32);
        for p in &points[1..] {
            pb.line_to(p.x - rect.x0 as f32, p.y - rect.y0 as f32);
        }
        pb.close();
        let path = pb.finish()?;
        let mut mask = Mask::new(rect.width(), rect.height())?;
        mask.fill_path(&path, FillRule::EvenOdd, false, Transform::identity());
        let data = mask.data().to_vec();
        if data.iter().all(|&v| v == 0) {
            return None;
        }
        Some(Self { rect, data: Some(data) })
    }

    pub fn is_rect(&self) -> bool {
        self.data.is_none()
    }

    /// Coverage at a canvas pixel.
    pub fn get(&self, x: i32, y: i32) -> u8 {
        if !self.rect.contains(x, y) {
            return 0;
        }
        match &self.data {
            None => 255,
            Some(d) => d[((y - self.rect.y0) as u32 * self.rect.width() + (x - self.rect.x0) as u32) as usize],
        }
    }

    /// Everything on the canvas except this selection.
    pub fn inverted(&self, canvas_w: u32, canvas_h: u32) -> Option<Self> {
        let full = PixelRect::from_size(canvas_w, canvas_h);
        let mut data = vec![255u8; full.area() as usize];
        let r = self.rect.intersect(&full);
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                data[(y as u32 * canvas_w + x as u32) as usize] = 255 - self.get(x, y);
            }
        }
        if data.iter().all(|&v| v == 0) {
            return None;
        }
        Some(Self { rect: full, data: Some(data) })
    }

    /// The selected pixels of `layer` (unselected pixels transparent), sized like `rect`.
    pub fn extract(&self, layer: &Pixmap) -> Option<Pixmap> {
        let mut out = raster::copy_region(layer, self.rect)?;
        let r = self.rect.intersect(&raster::bounds(layer));
        if self.data.is_some() {
            let w = out.width() as usize;
            for (i, px) in out.data_mut().chunks_exact_mut(4).enumerate() {
                let (x, y) = (r.x0 + (i % w) as i32, r.y0 + (i / w) as i32);
                let m = u16::from(self.get(x, y));
                if m != 255 {
                    for c in px.iter_mut() {
                        *c = crate::color::mul_div255(u16::from(*c), m);
                    }
                }
            }
        }
        Some(out)
    }

    /// Makes the selected pixels of `layer` transparent.
    pub fn clear(&self, layer: &mut Pixmap) {
        let r = self.rect.intersect(&raster::bounds(layer));
        if self.data.is_none() {
            raster::clear_region(layer, r);
            return;
        }
        let w = layer.width() as usize;
        let data = layer.data_mut();
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                let m = self.get(x, y);
                if m == 0 {
                    continue;
                }
                let i = (y as usize * w + x as usize) * 4;
                let keep = 255 - u16::from(m);
                for c in &mut data[i..i + 4] {
                    *c = crate::color::mul_div255(u16::from(*c), keep);
                }
            }
        }
    }

    /// Fills the selected pixels of `layer` with `color` (used when Paint fills a moved hole).
    pub fn fill(&self, layer: &mut Pixmap, color: Color) {
        let r = self.rect.intersect(&raster::bounds(layer));
        let p = color.premultiplied();
        let w = layer.width() as usize;
        let data = layer.data_mut();
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                if self.get(x, y) >= 128 {
                    let i = (y as usize * w + x as usize) * 4;
                    data[i..i + 4].copy_from_slice(&p);
                }
            }
        }
    }
}

/// Makes every pixel equal to `key` (ignoring alpha) fully transparent ("Transparent selection").
pub fn key_out(p: &mut Pixmap, key: Color) {
    for px in p.data_mut().chunks_exact_mut(4) {
        let c = Color::from_premultiplied([px[0], px[1], px[2], px[3]]);
        if px[3] != 0 && c.r == key.r && c.g == key.g && c.b == key.b {
            px.fill(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_extract_and_clear() {
        let mut layer = raster::pixmap(10, 10);
        raster::fill_region(&mut layer, PixelRect::from_size(10, 10), Color::BLACK);
        let sel = SelectionMask::rect(PixelRect::new(2, 2, 5, 6));
        let part = sel.extract(&layer).unwrap();
        assert_eq!((part.width(), part.height()), (3, 4));
        sel.clear(&mut layer);
        assert_eq!(layer.pixel(3, 3).unwrap().alpha(), 0);
        assert_eq!(layer.pixel(5, 3).unwrap().alpha(), 255);
    }

    #[test]
    fn polygon_and_invert() {
        let pts = [Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(0.0, 10.0)];
        let sel = SelectionMask::polygon(&pts, 20, 20).unwrap();
        assert!(!sel.is_rect());
        assert_eq!(sel.get(1, 1), 255);
        assert_eq!(sel.get(9, 9), 0);
        let inv = sel.inverted(20, 20).unwrap();
        assert_eq!(inv.get(1, 1), 0);
        assert_eq!(inv.get(15, 15), 255);
        assert!(SelectionMask::polygon(&pts[..2], 20, 20).is_none());
    }

    #[test]
    fn key_out_color() {
        let mut p = raster::pixmap(2, 1);
        raster::fill_region(&mut p, PixelRect::new(0, 0, 1, 1), Color::WHITE);
        raster::fill_region(&mut p, PixelRect::new(1, 0, 2, 1), Color::BLACK);
        key_out(&mut p, Color::WHITE);
        assert_eq!(p.pixel(0, 0).unwrap().alpha(), 0);
        assert_eq!(p.pixel(1, 0).unwrap().alpha(), 255);
    }
}
