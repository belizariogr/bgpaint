//! Text rendering with system fonts (cosmic-text: shaping, wrapping, decorations).

use crate::color::{Color, mul_div255};
use crate::raster;
use cosmic_text::{Align, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, SwashCache, UnderlineStyle, Weight, Wrap};
use tiny_skia::Pixmap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

/// Formatting of a text box (Paint applies it to the whole box).
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub family: String,
    /// Size in points (rendered at 96 DPI, like Paint).
    pub size_pt: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub align: TextAlign,
    pub color: Color,
    /// Box background (Paint's "Background fill" uses Color 2).
    pub background: Option<Color>,
}

impl TextStyle {
    pub fn size_px(&self) -> f32 {
        self.size_pt * 96.0 / 72.0
    }

    pub fn metrics(&self) -> Metrics {
        let px = self.size_px().max(1.0);
        Metrics::new(px, (px * 1.22).ceil())
    }

    pub fn attrs(&self) -> Attrs<'_> {
        let c = self.color;
        let mut a = Attrs::new()
            .family(Family::Name(&self.family))
            .weight(if self.bold { Weight::BOLD } else { Weight::NORMAL })
            .style(if self.italic { Style::Italic } else { Style::Normal })
            .color(cosmic_text::Color::rgba(c.r, c.g, c.b, c.a));
        if self.underline {
            a = a.underline(UnderlineStyle::Single);
        }
        if self.strikethrough {
            a = a.strikethrough();
        }
        a
    }

    pub fn cosmic_align(&self) -> Align {
        match self.align {
            TextAlign::Left => Align::Left,
            TextAlign::Center => Align::Center,
            TextAlign::Right => Align::Right,
        }
    }
}

/// The font database and glyph cache. Loading scans the system fonts, so do it off the UI thread.
pub struct Fonts {
    pub system: FontSystem,
    pub cache: SwashCache,
    families: Vec<String>,
}

impl Fonts {
    pub fn load() -> Self {
        let system = FontSystem::new();
        let mut families: Vec<String> =
            system.db().faces().filter_map(|f| f.families.first().map(|(name, _)| name.clone())).filter(|n| !n.starts_with('.')).collect();
        families.sort_by_key(|f| f.to_lowercase());
        families.dedup();
        Self { system, cache: SwashCache::new(), families }
    }

    /// Installed font families, sorted.
    pub fn families(&self) -> &[String] {
        &self.families
    }

    /// A pleasant default available on this system.
    pub fn default_family(&self) -> String {
        const PREFERRED: [&str; 10] = [
            "Calibri",
            "Segoe UI",
            "Helvetica Neue",
            "Inter",
            "Noto Sans",
            "Cantarell",
            "DejaVu Sans",
            "Liberation Sans",
            "Arial",
            "Helvetica",
        ];
        PREFERRED
            .iter()
            .find(|p| self.families.iter().any(|f| f == *p))
            .map(|s| (*s).to_owned())
            .or_else(|| self.families.first().cloned())
            .unwrap_or_else(|| "sans-serif".to_owned())
    }

    /// Creates a wrapping buffer for a box `width` pixels wide.
    pub fn buffer(&mut self, text: &str, style: &TextStyle, width: f32) -> Buffer {
        let mut b = Buffer::new(&mut self.system, style.metrics());
        b.set_wrap(Wrap::WordOrGlyph);
        b.set_size(Some(width.max(1.0)), None);
        b.set_text(text, &style.attrs(), Shaping::Advanced, Some(style.cosmic_align()));
        b.shape_until_scroll(&mut self.system, false);
        b
    }
}

/// Height needed by the laid out text.
pub fn content_height(buffer: &Buffer) -> f32 {
    buffer.layout_runs().map(|r| r.line_top + r.line_height).fold(buffer.metrics().line_height, f32::max)
}

/// Renders `buffer` into a `width`×`height` pixmap, with the optional background fill.
pub fn render(fonts: &mut Fonts, buffer: &mut Buffer, style: &TextStyle, width: u32, height: u32) -> Pixmap {
    let mut out = raster::pixmap(width, height);
    if let Some(bg) = style.background {
        out.fill(bg.to_skia());
    }
    let c = style.color;
    let (w, h) = (width as i32, height as i32);
    let stride = width as usize * 4;
    let data = out.data_mut();
    buffer.draw(&mut fonts.system, &mut fonts.cache, cosmic_text::Color::rgba(c.r, c.g, c.b, c.a), |x, y, rw, rh, color| {
        let a = u16::from(color.a());
        if a == 0 {
            return;
        }
        let src =
            [mul_div255(u16::from(color.r()), a), mul_div255(u16::from(color.g()), a), mul_div255(u16::from(color.b()), a), color.a()];
        let inv = 255 - a;
        for py in y.max(0)..(y + rh as i32).min(h) {
            for px in x.max(0)..(x + rw as i32).min(w) {
                let i = py as usize * stride + px as usize * 4;
                for k in 0..4 {
                    data[i + k] = src[k].saturating_add(mul_div255(u16::from(data[i + k]), inv));
                }
            }
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(family: String) -> TextStyle {
        TextStyle {
            family,
            size_pt: 24.0,
            bold: false,
            italic: false,
            underline: true,
            strikethrough: false,
            align: TextAlign::Left,
            color: Color::BLACK,
            background: Some(Color::WHITE),
        }
    }

    #[test]
    fn renders_text_with_system_fonts() {
        let mut fonts = Fonts::load();
        if fonts.families().is_empty() {
            return; // No fonts on this machine (minimal CI image).
        }
        let s = style(fonts.default_family());
        let mut b = fonts.buffer("Hello world, this wraps", &s, 120.0);
        let h = content_height(&b);
        assert!(h > s.metrics().line_height, "text should wrap into several lines");
        let p = render(&mut fonts, &mut b, &s, 120, h.ceil() as u32);
        let dark = p.pixels().iter().filter(|c| c.red() < 128).count();
        assert!(dark > 50, "some glyph pixels must be drawn");
        assert!(p.pixels().iter().all(|c| c.alpha() == 255), "background fill covers the box");
    }
}
