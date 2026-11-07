//! Colors: straight (non premultiplied) RGBA plus HSV helpers.

/// A straight-alpha sRGB color.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}

impl Color {
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn from_hex(hex: u32) -> Self {
        Self::rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }

    pub fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// Premultiplied components.
    pub fn premultiplied(&self) -> [u8; 4] {
        let a = u16::from(self.a);
        [mul_div255(u16::from(self.r), a), mul_div255(u16::from(self.g), a), mul_div255(u16::from(self.b), a), self.a]
    }

    /// Builds a color from premultiplied components.
    pub fn from_premultiplied(p: [u8; 4]) -> Self {
        let a = p[3];
        if a == 0 {
            return Color::TRANSPARENT;
        }
        let un = |c: u8| ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8;
        Color::rgba(un(p[0]), un(p[1]), un(p[2]), a)
    }

    pub fn to_skia(&self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba8(self.r, self.g, self.b, self.a)
    }

    /// `#RRGGBB` (alpha is ignored).
    pub fn to_hex_string(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// Parses `#RGB`, `#RRGGBB` or `#RRGGBBAA` (the `#` is optional).
    pub fn parse_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        if !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
        match s.len() {
            3 => {
                let n = |i: usize| u8::from_str_radix(&s[i..=i], 16).ok().map(|v| v * 17);
                Some(Color::rgb(n(0)?, n(1)?, n(2)?))
            }
            6 => Some(Color::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Color::rgba(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }

    /// Inverts RGB, keeping alpha.
    pub fn inverted(&self) -> Self {
        Color::rgba(255 - self.r, 255 - self.g, 255 - self.b, self.a)
    }

    /// Hue in degrees [0, 360), saturation and value in [0, 1].
    pub fn to_hsv(&self) -> (f32, f32, f32) {
        let r = f32::from(self.r) / 255.0;
        let g = f32::from(self.g) / 255.0;
        let b = f32::from(self.b) / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let d = max - min;
        let h = if d == 0.0 {
            0.0
        } else if max == r {
            60.0 * ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        let s = if max == 0.0 { 0.0 } else { d / max };
        (h, s, max)
    }

    pub fn from_hsv(h: f32, s: f32, v: f32) -> Self {
        let h = h.rem_euclid(360.0);
        let s = s.clamp(0.0, 1.0);
        let v = v.clamp(0.0, 1.0);
        let c = v * s;
        let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
        let m = v - c;
        let (r, g, b) = match (h / 60.0) as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let to = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        Color::rgb(to(r), to(g), to(b))
    }

    /// Largest per channel difference between two colors (alpha included).
    pub fn distance(&self, o: &Color) -> u8 {
        let d = |a: u8, b: u8| a.abs_diff(b);
        d(self.r, o.r).max(d(self.g, o.g)).max(d(self.b, o.b)).max(d(self.a, o.a))
    }
}

/// `a * b / 255` with rounding.
#[inline]
pub fn mul_div255(a: u16, b: u16) -> u8 {
    let t = u32::from(a) * u32::from(b) + 128;
    (((t >> 8) + t) >> 8) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let c = Color::parse_hex("#3F48CC").unwrap();
        assert_eq!(c, Color::rgb(0x3F, 0x48, 0xCC));
        assert_eq!(c.to_hex_string(), "#3F48CC");
        assert_eq!(Color::parse_hex("fff"), Some(Color::WHITE));
        assert_eq!(Color::parse_hex("zzz"), None);
        assert_eq!(Color::parse_hex("#12345"), None);
    }

    #[test]
    fn hsv_roundtrip() {
        for c in [Color::rgb(255, 0, 0), Color::rgb(12, 200, 99), Color::rgb(128, 128, 128), Color::rgb(0, 0, 255)] {
            let (h, s, v) = c.to_hsv();
            assert!(Color::from_hsv(h, s, v).distance(&c) <= 1, "{c:?}");
        }
    }

    #[test]
    fn premultiply_roundtrip() {
        let c = Color::rgba(200, 100, 50, 128);
        let back = Color::from_premultiplied(c.premultiplied());
        assert!(back.distance(&c) <= 2);
        assert_eq!(mul_div255(255, 255), 255);
        assert_eq!(mul_div255(0, 255), 0);
    }
}
