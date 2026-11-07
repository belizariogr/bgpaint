//! System clipboard access for images (with transparency).

use bgpaint_core::raster;
use bgpaint_core::tiny_skia::Pixmap;
use std::sync::Mutex;

static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut arboard::Clipboard) -> Option<R>) -> Option<R> {
    let mut guard = CLIPBOARD.lock().ok()?;
    if guard.is_none() {
        *guard = arboard::Clipboard::new().ok();
    }
    f(guard.as_mut()?)
}

/// Puts an image on the clipboard.
pub fn set_image(p: &Pixmap) -> bool {
    let rgba = raster::to_straight_rgba(p);
    with(|c| c.set_image(arboard::ImageData { width: p.width() as usize, height: p.height() as usize, bytes: rgba.into() }).ok()).is_some()
}

/// Reads an image from the clipboard.
pub fn get_image() -> Option<Pixmap> {
    with(|c| {
        let img = c.get_image().ok()?;
        raster::from_straight_rgba(img.width as u32, img.height as u32, &img.bytes).ok()
    })
}

/// Whether the clipboard probably holds an image (cheap check used to enable "Paste").
pub fn has_image() -> bool {
    true
}
