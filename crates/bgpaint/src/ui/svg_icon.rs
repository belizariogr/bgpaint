//! Icons drawn from the app's own SVGs (for the few the icon font has no glyph for). They are
//! rasterized with resvg at the display's scale and cached, so they stay sharp at any DPI.

use bgpaint_core::tiny_skia::{Pixmap, Transform};
use egui::{Color32, Id, Rect, TextureHandle, TextureOptions, pos2};

/// The arrow pointer (after the Adwaita cursor), in a 24 x 24 box.
pub const POINTER: &str = include_str!("../../assets/icons/pointer.svg");

/// Renders `svg` into a `w` x `h` pixel image (also used for the tool cursors).
pub fn rasterize(svg: &str, w: u32, h: u32) -> Option<egui::ColorImage> {
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = Pixmap::new(w, h)?;
    let (sx, sy) = (w as f32 / tree.size().width(), h as f32 / tree.size().height());
    resvg::render(&tree, Transform::from_scale(sx, sy), &mut pixmap.as_mut());
    // tiny-skia stores premultiplied RGBA, which is what egui's Color32 holds too.
    let pixels = pixmap.pixels().iter().map(|p| Color32::from_rgba_premultiplied(p.red(), p.green(), p.blue(), p.alpha())).collect();
    Some(egui::ColorImage::new([w as usize, h as usize], pixels))
}

/// Paints `svg` (cached under `name`) over `rect`, snapped to whole physical pixels.
pub fn paint(ctx: &egui::Context, painter: &egui::Painter, rect: Rect, name: &'static str, svg: &str) {
    let ppp = ctx.pixels_per_point();
    let (w, h) = ((rect.width() * ppp).round().max(1.0) as u32, (rect.height() * ppp).round().max(1.0) as u32);
    let id = Id::unique(("svg-icon", name, w, h));
    let cached: Option<TextureHandle> = ctx.data(|d| d.get_temp(id));
    let tex = match cached {
        Some(t) => t,
        None => {
            let Some(img) = rasterize(svg, w, h) else { return };
            let t = ctx.load_texture(format!("svg-icon-{name}-{w}x{h}"), img, TextureOptions::LINEAR);
            ctx.data_mut(|d| d.insert_temp(id, t.clone()));
            t
        }
    };
    let min = pos2((rect.min.x * ppp).round() / ppp, (rect.min.y * ppp).round() / ppp);
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    painter.image(tex.id(), Rect::from_min_size(min, egui::vec2(w as f32, h as f32) / ppp), uv, Color32::WHITE);
}
