//! Where AppKit puts the window's traffic lights (close, minimize, zoom), so the tab row that
//! doubles as the title bar can be laid out around them (see `ui::menubar`).
//!
//! The buttons are AppKit's own, centered in its title bar, whose size depends on the macOS
//! version and on the SDK the app was linked with: 14 pt buttons in a 32 pt bar with the macOS 26
//! SDK on macOS 26 and later, 12 pt buttons in a 28 pt bar otherwise. Reading the real layout keeps
//! the row centered on the buttons everywhere instead of assuming one of them.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSWindowButton, NSWindowStyleMask};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TitleBar {
    /// Height of the row: the buttons sit in its middle.
    pub height: f32,
    /// Where the row's content starts: past the zoom button by the close button's distance from
    /// the left edge, so the buttons have the same space on every side.
    pub content_left: f32,
}

/// The macOS 26 layout, used while the window cannot be measured.
const FALLBACK: TitleBar = TitleBar { height: 32.0, content_left: 78.0 };

static MEASURED: OnceLock<TitleBar> = OnceLock::new();

/// The layout of the app window's title bar. Measured once, on the first frame outside full
/// screen (AppKit lays the buttons out when it creates the window, before showing it, and does not
/// move them afterwards).
pub fn title_bar() -> TitleBar {
    if let Some(m) = MEASURED.get() {
        return *m;
    }
    measure().map_or(FALLBACK, |m| *MEASURED.get_or_init(|| m))
}

fn measure() -> Option<TitleBar> {
    let mtm = MainThreadMarker::new()?;
    // The app window is the one whose content runs under a transparent title bar (see `main`).
    let window = NSApplication::sharedApplication(mtm).windows().iter().find(|w| w.titlebarAppearsTransparent())?;
    // In full screen the buttons live in a title bar that slides in from the top of the screen.
    if window.styleMask().contains(NSWindowStyleMask::FullScreen) {
        return None;
    }
    let window_height = window.frame().size.height;
    // In window coordinates: origin at the bottom left, in points (as egui's, since the app keeps
    // egui's zoom at 1).
    let rect = |kind| window.standardWindowButton(kind).map(|b| b.convertRect_toView(b.bounds(), None));
    let close = rect(NSWindowButton::CloseButton)?;
    let zoom = rect(NSWindowButton::ZoomButton)?;
    let center_from_top = window_height - (close.origin.y + close.size.height / 2.0);
    let height = (2.0 * center_from_top) as f32;
    let content_left = (zoom.origin.x + zoom.size.width + close.origin.x) as f32;
    // Anything else means the title bar is not laid out yet.
    let plausible = close.size.height > 0.0 && (20.0..=64.0).contains(&height) && (40.0..=160.0).contains(&content_left);
    plausible.then_some(TitleBar { height, content_left })
}
