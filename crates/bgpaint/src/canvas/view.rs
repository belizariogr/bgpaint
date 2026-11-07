//! Zoom and scroll state of the canvas viewport.

use bgpaint_core::geom::Point;
use egui::{Pos2, Rect, Vec2, vec2};

/// Space kept around the canvas (room for the resize handles), in points.
pub const MARGIN: f32 = 24.0;
/// Minimum space around a canvas that is shown whole (and around "Fit to window"), in points.
pub const FIT_PADDING: f32 = 12.0;
pub const MIN_ZOOM: f32 = 0.01;
pub const MAX_ZOOM: f32 = 32.0;
/// Paint's zoom presets.
pub const PRESETS: [f32; 11] = [0.125, 0.25, 0.5, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];

#[derive(Clone, Debug)]
pub struct View {
    /// Physical pixels per image pixel (1.0 = 100%: one image pixel per screen pixel).
    pub zoom: f32,
    /// Position of the canvas origin relative to the viewport's top-left corner, in points.
    pub offset: Vec2,
    /// Screen rectangle of the viewport (updated each frame).
    pub viewport: Rect,
    pub ppp: f32,
    pub canvas: (u32, u32),
    fit_requested: bool,
    initialized: bool,
}

impl Default for View {
    fn default() -> Self {
        Self { zoom: 1.0, offset: Vec2::ZERO, viewport: Rect::NOTHING, ppp: 1.0, canvas: (1, 1), fit_requested: false, initialized: false }
    }
}

impl View {
    /// Size of one image pixel in points.
    pub fn scale(&self) -> f32 {
        self.zoom / self.ppp
    }

    pub fn canvas_size_points(&self) -> Vec2 {
        vec2(self.canvas.0 as f32, self.canvas.1 as f32) * self.scale()
    }

    pub fn origin(&self) -> Pos2 {
        self.viewport.min + self.offset
    }

    pub fn canvas_rect(&self) -> Rect {
        Rect::from_min_size(self.origin(), self.canvas_size_points())
    }

    pub fn to_screen(&self, p: Point) -> Pos2 {
        self.origin() + vec2(p.x, p.y) * self.scale()
    }

    pub fn to_canvas(&self, p: Pos2) -> Point {
        let v = (p - self.origin()) / self.scale();
        Point::new(v.x, v.y)
    }

    /// Re-centers the canvas the next time the layout runs.
    pub fn reset(&mut self) {
        self.initialized = false;
    }

    pub fn request_fit(&mut self) {
        self.fit_requested = true;
    }

    /// Updates the viewport and keeps the canvas in a valid position.
    pub fn layout(&mut self, viewport: Rect, canvas: (u32, u32), ppp: f32) {
        let size_changed = self.canvas != canvas;
        self.viewport = viewport;
        self.ppp = ppp;
        self.canvas = canvas;
        if self.fit_requested {
            self.fit_requested = false;
            self.zoom = self.fit_zoom();
            self.initialized = false;
        }
        if !self.initialized {
            self.initialized = true;
            self.offset = vec2(MARGIN, MARGIN);
            self.center_small_axes(true);
        } else {
            self.center_small_axes(size_changed);
        }
    }

    /// Zoom at which the canvas fills the viewport as much as possible, uncropped,
    /// with [`FIT_PADDING`] on every side.
    pub fn fit_zoom(&self) -> f32 {
        let avail = (self.viewport.size() - Vec2::splat(FIT_PADDING * 2.0)).max(Vec2::splat(16.0));
        let z = (avail.x / self.canvas.0 as f32).min(avail.y / self.canvas.1 as f32) * self.ppp;
        z.clamp(MIN_ZOOM, MAX_ZOOM)
    }

    /// Whether the canvas fits along `axis` and is therefore centered.
    pub fn fits(&self, axis: usize) -> bool {
        self.canvas_size_points()[axis] + FIT_PADDING * 2.0 <= self.viewport.size()[axis] + 0.01
    }

    fn center_small_axes(&mut self, force: bool) {
        let size = self.canvas_size_points();
        let vp = self.viewport.size();
        for axis in 0..2 {
            let (s, v) = (size[axis], vp[axis]);
            if self.fits(axis) {
                self.offset[axis] = ((v - s) / 2.0).round();
            } else {
                let min = v - s - MARGIN;
                if force && self.offset[axis] > MARGIN {
                    self.offset[axis] = MARGIN;
                }
                self.offset[axis] = self.offset[axis].clamp(min, MARGIN);
            }
        }
    }

    /// Scrolls by `delta` points.
    pub fn scroll(&mut self, delta: Vec2) {
        self.offset += delta;
        self.center_small_axes(false);
    }

    /// Sets the zoom keeping the canvas point under `anchor` (screen) fixed.
    pub fn zoom_at(&mut self, zoom: f32, anchor: Pos2) {
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let before = self.to_canvas(anchor);
        self.zoom = zoom;
        let after = self.to_screen(before);
        self.offset += anchor - after;
        self.center_small_axes(false);
    }

    pub fn zoom_center(&mut self, zoom: f32) {
        let anchor = if self.canvas_rect().intersects(self.viewport) {
            self.canvas_rect().intersect(self.viewport).center()
        } else {
            self.viewport.center()
        };
        self.zoom_at(zoom, anchor);
    }

    pub fn next_preset(&self, up: bool) -> f32 {
        if up {
            PRESETS.iter().copied().find(|&p| p > self.zoom + 1e-4).unwrap_or((self.zoom * 2.0).min(MAX_ZOOM))
        } else {
            PRESETS.iter().rev().copied().find(|&p| p < self.zoom - 1e-4).unwrap_or((self.zoom / 2.0).max(MIN_ZOOM))
        }
    }

    /// Scrolls so that canvas point `p` is visible.
    pub fn ensure_visible(&mut self, p: Point) {
        let s = self.to_screen(p);
        let vp = self.viewport.shrink(MARGIN);
        let mut d = Vec2::ZERO;
        if s.x < vp.left() {
            d.x = vp.left() - s.x;
        } else if s.x > vp.right() {
            d.x = vp.right() - s.x;
        }
        if s.y < vp.top() {
            d.y = vp.top() - s.y;
        } else if s.y > vp.bottom() {
            d.y = vp.bottom() - s.y;
        }
        if d != Vec2::ZERO {
            self.scroll(d);
        }
    }

    /// Visible part of the canvas, in canvas pixel coordinates.
    pub fn visible_canvas(&self) -> (Point, Point) {
        let a = self.to_canvas(self.viewport.min);
        let b = self.to_canvas(self.viewport.max);
        (a, b)
    }

    pub fn zoom_percent(&self) -> String {
        let p = self.zoom * 100.0;
        if (p - p.round()).abs() < 0.05 { format!("{}%", p.round() as i64) } else { format!("{p:.1}%") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    fn view(vw: f32, vh: f32, w: u32, h: u32) -> View {
        let mut v = View::default();
        v.layout(Rect::from_min_size(pos2(0.0, 0.0), vec2(vw, vh)), (w, h), 1.0);
        v
    }

    #[test]
    fn small_canvas_is_centered() {
        let v = view(1000.0, 800.0, 200, 100);
        assert_eq!(v.offset, vec2(400.0, 350.0));
        let p = v.to_canvas(pos2(500.0, 400.0));
        assert_eq!((p.x, p.y), (100.0, 50.0));
    }

    #[test]
    fn zoom_keeps_anchor() {
        let mut v = view(500.0, 500.0, 2000, 2000);
        let anchor = pos2(250.0, 250.0);
        let before = v.to_canvas(anchor);
        v.zoom_at(2.0, anchor);
        let after = v.to_canvas(anchor);
        assert!((before.x - after.x).abs() < 0.01 && (before.y - after.y).abs() < 0.01);
    }

    #[test]
    fn presets_step() {
        let mut v = view(500.0, 500.0, 100, 100);
        assert_eq!(v.next_preset(true), 2.0);
        v.zoom = 0.3;
        assert_eq!(v.next_preset(false), 0.25);
        assert_eq!(v.next_preset(true), 0.5);
    }

    #[test]
    fn fit_zoom_shows_everything() {
        let mut v = view(548.0, 348.0, 1000, 600);
        v.request_fit();
        v.layout(v.viewport, (1000, 600), 1.0);
        assert!((v.zoom - 0.524).abs() < 1e-3, "{}", v.zoom);
        assert!(v.fits(0) && v.fits(1));
        assert_eq!(v.offset.x, 12.0);
        // Small images are enlarged to fill the window.
        let mut v = view(548.0, 348.0, 100, 50);
        v.request_fit();
        v.layout(v.viewport, (100, 50), 1.0);
        assert!((v.zoom - 5.24).abs() < 1e-3, "{}", v.zoom);
    }
}
