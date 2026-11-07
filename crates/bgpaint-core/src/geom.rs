//! Small integer/float geometry helpers used across the engine.

/// An axis aligned rectangle in pixel space. `x1`/`y1` are exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct PixelRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl PixelRect {
    pub const EMPTY: Self = Self { x0: 0, y0: 0, x1: 0, y1: 0 };

    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    pub const fn from_xywh(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x0: x, y0: y, x1: x + w as i32, y1: y + h as i32 }
    }

    pub const fn from_size(w: u32, h: u32) -> Self {
        Self::from_xywh(0, 0, w, h)
    }

    /// Smallest rectangle containing both corner points (inclusive pixels).
    pub fn from_points(a: (i32, i32), b: (i32, i32)) -> Self {
        Self { x0: a.0.min(b.0), y0: a.1.min(b.1), x1: a.0.max(b.0) + 1, y1: a.1.max(b.1) + 1 }
    }

    /// Bounding pixels of a float rectangle, grown outwards.
    pub fn enclosing(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self { x0: x0.min(x1).floor() as i32, y0: y0.min(y1).floor() as i32, x1: x0.max(x1).ceil() as i32, y1: y0.max(y1).ceil() as i32 }
    }

    pub fn width(&self) -> u32 {
        (self.x1 - self.x0).max(0) as u32
    }

    pub fn height(&self) -> u32 {
        (self.y1 - self.y0).max(0) as u32
    }

    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    pub fn area(&self) -> u64 {
        u64::from(self.width()) * u64::from(self.height())
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }

    pub fn intersect(&self, other: &Self) -> Self {
        let r = Self { x0: self.x0.max(other.x0), y0: self.y0.max(other.y0), x1: self.x1.min(other.x1), y1: self.y1.min(other.y1) };
        if r.is_empty() { Self::EMPTY } else { r }
    }

    /// Union of two rectangles; empty rectangles are ignored.
    pub fn union(&self, other: &Self) -> Self {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        Self { x0: self.x0.min(other.x0), y0: self.y0.min(other.y0), x1: self.x1.max(other.x1), y1: self.y1.max(other.y1) }
    }

    pub fn expand(&self, by: i32) -> Self {
        if self.is_empty() {
            return *self;
        }
        Self { x0: self.x0 - by, y0: self.y0 - by, x1: self.x1 + by, y1: self.y1 + by }
    }

    pub fn translate(&self, dx: i32, dy: i32) -> Self {
        Self { x0: self.x0 + dx, y0: self.y0 + dy, x1: self.x1 + dx, y1: self.y1 + dy }
    }

    pub fn clamp_to(&self, w: u32, h: u32) -> Self {
        self.intersect(&Self::from_size(w, h))
    }

    pub fn to_int_rect(&self) -> Option<tiny_skia::IntRect> {
        tiny_skia::IntRect::from_xywh(self.x0, self.y0, self.width(), self.height())
    }
}

/// Accumulates a dirty region as a single bounding rectangle.
#[derive(Clone, Copy, Debug, Default)]
pub struct DirtyRect(Option<PixelRect>);

impl DirtyRect {
    pub fn add(&mut self, r: PixelRect) {
        if r.is_empty() {
            return;
        }
        self.0 = Some(match self.0 {
            Some(cur) => cur.union(&r),
            None => r,
        });
    }

    pub fn take(&mut self) -> Option<PixelRect> {
        self.0.take()
    }

    pub fn get(&self) -> Option<PixelRect> {
        self.0
    }
}

/// A point in canvas (image pixel) coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn distance(&self, o: Point) -> f32 {
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2)).sqrt()
    }

    pub fn lerp(&self, o: Point, t: f32) -> Point {
        Point::new(self.x + (o.x - self.x) * t, self.y + (o.y - self.y) * t)
    }

    pub fn floor_i(&self) -> (i32, i32) {
        (self.x.floor() as i32, self.y.floor() as i32)
    }
}

impl std::ops::Sub for Point {
    type Output = Point;
    fn sub(self, o: Point) -> Point {
        Point::new(self.x - o.x, self.y - o.y)
    }
}

impl std::ops::Add for Point {
    type Output = Point;
    fn add(self, o: Point) -> Point {
        Point::new(self.x + o.x, self.y + o.y)
    }
}

/// A float rectangle (used for shape and selection boxes).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct RectF {
    pub min: Point,
    pub max: Point,
}

impl RectF {
    pub fn from_points(a: Point, b: Point) -> Self {
        Self { min: Point::new(a.x.min(b.x), a.y.min(b.y)), max: Point::new(a.x.max(b.x), a.y.max(b.y)) }
    }

    pub fn from_xywh(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self::from_points(Point::new(x, y), Point::new(x + w, y + h))
    }

    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }

    pub fn center(&self) -> Point {
        Point::new((self.min.x + self.max.x) * 0.5, (self.min.y + self.max.y) * 0.5)
    }

    pub fn translate(&self, dx: f32, dy: f32) -> Self {
        Self { min: Point::new(self.min.x + dx, self.min.y + dy), max: Point::new(self.max.x + dx, self.max.y + dy) }
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    pub fn to_pixel_rect(&self) -> PixelRect {
        PixelRect::enclosing(self.min.x, self.min.y, self.max.x, self.max.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_ops() {
        let a = PixelRect::new(0, 0, 10, 10);
        let b = PixelRect::new(5, 5, 20, 20);
        assert_eq!(a.intersect(&b), PixelRect::new(5, 5, 10, 10));
        assert_eq!(a.union(&b), PixelRect::new(0, 0, 20, 20));
        assert!(a.intersect(&PixelRect::new(10, 10, 12, 12)).is_empty());
        assert_eq!(PixelRect::EMPTY.union(&b), b);
        assert_eq!(PixelRect::from_points((3, 4), (1, 2)), PixelRect::new(1, 2, 4, 5));
    }

    #[test]
    fn dirty_accumulates() {
        let mut d = DirtyRect::default();
        d.add(PixelRect::new(0, 0, 2, 2));
        d.add(PixelRect::EMPTY);
        d.add(PixelRect::new(4, 4, 6, 6));
        assert_eq!(d.take(), Some(PixelRect::new(0, 0, 6, 6)));
        assert_eq!(d.take(), None);
    }
}
