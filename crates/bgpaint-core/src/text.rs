//! Text rendering with system fonts (cosmic-text: shaping, wrapping, decorations).

use crate::color::{Color, mul_div255};
use crate::geom::{Point, RectF};
use crate::raster;
use crate::shapes::{self, RasterShape};
use cosmic_text::{Align, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, SwashCache, UnderlineStyle, Weight, Wrap};
use std::sync::{Mutex, MutexGuard, OnceLock};
use tiny_skia::Pixmap;

static FONTS: OnceLock<Mutex<Fonts>> = OnceLock::new();

/// The shared font database (blocks while the system fonts are being scanned). Do not hold it
/// across document operations: re-rendering a text object takes it too.
pub fn fonts() -> MutexGuard<'static, Fonts> {
    let m = FONTS.get_or_init(|| Mutex::new(Fonts::load()));
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

impl TextAlign {
    /// The point of a box's width (0 to 1) that stays put when the box fits itself to its text.
    pub fn anchor(self) -> f32 {
        match self {
            TextAlign::Left => 0.0,
            TextAlign::Center => 0.5,
            TextAlign::Right => 1.0,
        }
    }
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
    /// Applied to the finished box, letters and background as one piece (0 to 1).
    pub opacity: f32,
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

/// Space kept between the sides of a text box and its text (and caret).
pub const BOX_PADDING: f32 = 4.0;

/// Width the text is wrapped to in a box `box_width` pixels wide.
pub fn layout_width(box_width: f32) -> f32 {
    (box_width - 2.0 * BOX_PADDING).max(8.0)
}

/// Measures `buffer` and lays it out without wrapping: the size of a box that just holds it.
pub fn fit_buffer(fonts: &mut Fonts, buffer: &mut Buffer) -> (f32, f32) {
    buffer.set_size(None, None);
    buffer.shape_until_scroll(&mut fonts.system, false);
    let natural = buffer.layout_runs().map(|r| r.line_w).fold(0.0, f32::max);
    // The extra pixel keeps rounding from wrapping the longest line.
    let width = (natural.ceil() + 1.0).max(buffer.metrics().line_height.ceil());
    buffer.set_size(Some(width), None);
    buffer.shape_until_scroll(&mut fonts.system, false);
    (width + 2.0 * BOX_PADDING, (content_height(buffer) + 4.0).ceil())
}

/// The size of a box that just holds `text` (see [`fit_buffer`]).
pub fn fit_box(text: &str, style: &TextStyle) -> (f32, f32) {
    let mut f = fonts();
    let mut buffer = f.buffer(text, style, 1.0);
    fit_buffer(&mut f, &mut buffer)
}

/// Puts text rendered by [`render`] in the box `rect` rotated by `angle` degrees, clipped to the
/// canvas. Text taller than the box goes on below it.
pub fn place_box(pixmap: &Pixmap, rect: RectF, angle: f32, canvas: (u32, u32)) -> Option<RasterShape> {
    let scale = rect.height() / rect.height().ceil().max(1.0);
    let full = RectF { min: rect.min, max: Point::new(rect.max.x, rect.min.y + pixmap.height() as f32 * scale) };
    shapes::render_box(pixmap, full, rect.center(), angle, canvas)
}

/// Lays out `text` in the box `rect` rotated by `angle` degrees and renders it, clipped to the
/// canvas.
pub fn rasterize_box(text: &str, style: &TextStyle, rect: RectF, angle: f32, canvas: (u32, u32)) -> Option<RasterShape> {
    let pixmap = {
        let mut f = fonts();
        let mut buffer = f.buffer(text, style, layout_width(rect.width()));
        render(&mut f, &mut buffer, style, (rect.width().ceil() as u32).max(1), (rect.height().ceil() as u32).max(1))
    };
    place_box(&pixmap, rect, angle, canvas)
}

/// Renders `buffer` for a `width`×`height` box, inside its side padding, with the optional
/// background fill. Text taller than the box makes the pixmap taller; the fill stays in the box.
pub fn render(fonts: &mut Fonts, buffer: &mut Buffer, style: &TextStyle, width: u32, height: u32) -> Pixmap {
    let full = height.max(content_height(buffer).ceil() as u32);
    let mut out = raster::pixmap(width, full);
    if let Some(bg) = style.background
        && let Some(area) = tiny_skia::Rect::from_xywh(0.0, 0.0, width as f32, height as f32)
    {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(bg.to_skia());
        out.fill_rect(area, &paint, tiny_skia::Transform::identity(), None);
    }
    let c = style.color;
    let (w, h) = (width as i32, full as i32);
    let pad = BOX_PADDING as i32;
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
        let x = x + pad;
        for py in y.max(0)..(y + rh as i32).min(h) {
            for px in x.max(0)..(x + rw as i32).min(w) {
                let i = py as usize * stride + px as usize * 4;
                for k in 0..4 {
                    data[i + k] = src[k].saturating_add(mul_div255(u16::from(data[i + k]), inv));
                }
            }
        }
    });
    shapes::fade(&mut out, style.opacity);
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
            opacity: 1.0,
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
        // A box fitted to the text holds it in one line, however long.
        let (fit_w, fit_h) = fit_box("Hello world, this wraps", &s);
        assert!(fit_w > 120.0, "{fit_w}");
        assert_eq!(content_height(&fonts.buffer("Hello world, this wraps", &s, layout_width(fit_w))), s.metrics().line_height);
        assert_eq!(fit_h, s.metrics().line_height + 4.0);
        assert!(p.pixels().iter().all(|c| c.alpha() == 255), "background fill covers the box");
        // Text taller than the box goes on below it; the fill stays in the box.
        let p = render(&mut fonts, &mut b, &s, 120, 10);
        assert_eq!(p.height(), h.ceil() as u32);
        assert!(p.pixel(0, 5).unwrap().alpha() == 255 && p.pixel(0, p.height() - 1).unwrap().alpha() == 0);
        // Opacity fades letters and background together.
        let half = TextStyle { opacity: 0.5, ..s };
        let p = render(&mut fonts, &mut b, &half, 120, h.ceil() as u32);
        assert!(p.pixels().iter().all(|c| (127..=128).contains(&c.alpha())));
    }
}
