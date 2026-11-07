//! Tool cursors (pencil, paint bucket, eyedropper) drawn on the canvas. They are SVGs in
//! `assets/cursors`, rasterized with resvg at the display's scale and cached as textures, so they
//! stay sharp at any DPI. `{PAINT}` in a file is replaced with the active color.

use bgpaint_core::color::Color;
use bgpaint_core::tiny_skia::{Pixmap, Transform};
use egui::{Color32, Id, Pos2, Rect, TextureHandle, TextureOptions, Vec2, pos2, vec2};

/// Side of the cursor box in points, equal to the SVGs' viewBox (a 20 unit drawing area with a
/// 2 unit margin all around, so the white halo is never cut off).
const SIZE: f32 = 24.0;
/// Top-left corner of the viewBox.
const ORIGIN: Vec2 = vec2(-2.0, -2.0);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolCursor {
    Pencil,
    Fill,
    Picker,
}

impl ToolCursor {
    fn svg(self) -> &'static str {
        match self {
            Self::Pencil => include_str!("../../assets/cursors/pencil.svg"),
            Self::Fill => include_str!("../../assets/cursors/fill.svg"),
            Self::Picker => include_str!("../../assets/cursors/picker.svg"),
        }
    }

    /// Where the tool acts, in drawing units (points).
    fn hotspot(self) -> Vec2 {
        match self {
            Self::Pencil | Self::Picker => vec2(1.5, 18.5),
            Self::Fill => vec2(5.2, 18.0),
        }
    }
}

/// One texture per cursor, re-rasterized when the scale or the paint color changes.
#[derive(Clone, Default)]
struct Cache(Vec<(ToolCursor, u32, [u8; 3], TextureHandle)>);

fn rasterize(c: ToolCursor, px: u32, paint: [u8; 3]) -> Option<egui::ColorImage> {
    let [r, g, b] = paint;
    let svg = c.svg().replace("{PAINT}", &format!("#{r:02x}{g:02x}{b:02x}"));
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = Pixmap::new(px, px)?;
    let scale = px as f32 / tree.size().width();
    resvg::render(&tree, Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    // tiny-skia stores premultiplied RGBA, which is what egui's Color32 holds too.
    let pixels = pixmap.pixels().iter().map(|p| Color32::from_rgba_premultiplied(p.red(), p.green(), p.blue(), p.alpha())).collect();
    Some(egui::ColorImage::new([px as usize, px as usize], pixels))
}

/// Paints `c` with its hotspot at `pos`.
pub fn paint(ctx: &egui::Context, painter: &egui::Painter, pos: Pos2, c: ToolCursor, paint: Color) {
    let ppp = ctx.pixels_per_point();
    let px = (SIZE * ppp).round().max(1.0) as u32;
    let rgb = [paint.r, paint.g, paint.b];
    let id = Id::unique("tool-cursor-cache");
    let mut cache: Cache = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    let tex = match cache.0.iter().find(|(k, ..)| *k == c) {
        Some((_, p, col, tex)) if *p == px && *col == rgb => tex.clone(),
        _ => {
            let Some(img) = rasterize(c, px, rgb) else { return };
            let tex = ctx.load_texture(format!("tool-cursor-{px}"), img, TextureOptions::LINEAR);
            cache.0.retain(|(k, ..)| *k != c);
            cache.0.push((c, px, rgb, tex.clone()));
            ctx.data_mut(|d| d.insert_temp(id, cache));
            tex
        }
    };
    // Snap to the pixel grid so the 1 px outlines stay crisp.
    let min = pos - (c.hotspot() - ORIGIN);
    let min = pos2((min.x * ppp).round() / ppp, (min.y * ppp).round() / ppp);
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    painter.image(tex.id(), Rect::from_min_size(min, Vec2::splat(SIZE)), uv, Color32::WHITE);
}
