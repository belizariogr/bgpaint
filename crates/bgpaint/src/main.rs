//! BGPaint: a fast clone of Windows 11 Paint for Linux and macOS.
// Denied everywhere; only `mac_open` (Objective-C) allows it.
#![deny(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app;
#[cfg(feature = "automation")]
mod automation;
mod canvas;
mod clipboard;
mod commands;
mod files;
mod i18n;
#[cfg(target_os = "macos")]
mod mac_menu;
#[cfg(target_os = "macos")]
mod mac_open;
#[cfg(target_os = "macos")]
mod mac_titlebar;
mod platform;
mod settings;
mod single_instance;
mod system;
mod tabs;
mod theme;
mod tools;
mod ui;
mod update;

use std::time::Instant;

fn main() -> eframe::Result {
    let started = Instant::now();
    // Linux only, read while the window is created.
    platform::refresh_desktop_theme();
    let settings = settings::Settings::load();
    // Files opened from Finder arrive as Apple events, not arguments.
    #[cfg(target_os = "macos")]
    mac_open::install();
    let open: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(std::path::PathBuf::from).collect();
    // A running BGPaint opens the files in new tabs instead.
    let instance = match single_instance::start(&open) {
        single_instance::Start::HandedOff => return Ok(()),
        single_instance::Start::Primary(listener) => listener,
    };
    let (w, h) = settings.window_size.unwrap_or((1280.0, 800.0));
    #[cfg_attr(target_os = "macos", allow(unused_variables))]
    let tr = i18n::resolve(settings.language.as_deref());
    // On macOS the icon carries the transparent margin of Apple's icon grid, so the Dock icon is
    // not oversized.
    #[cfg(target_os = "macos")]
    let icon_png: &[u8] = include_bytes!("../assets/icon-macos-256.png");
    #[cfg(not(target_os = "macos"))]
    let icon_png: &[u8] = include_bytes!("../assets/icon-256.png");
    let icon = eframe::icon_data::from_png_bytes(icon_png).ok();
    // eframe copies the initial title into the macOS app menu, which must read just "BGPaint";
    // the window title is updated with the document name afterwards.
    #[cfg(target_os = "macos")]
    let initial_title = "BGPaint".to_string();
    #[cfg(not(target_os = "macos"))]
    let initial_title = format!("{} - {}", tr.untitled, tr.app_name);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(initial_title)
        .with_app_id("io.github.belizariogr.bgpaint")
        .with_inner_size([w.max(640.0), h.max(480.0)])
        .with_min_inner_size([720.0, 480.0])
        .with_maximized(settings.maximized)
        .with_drag_and_drop(true);
    // On macOS the tab row doubles as the title bar: the content runs under a transparent title
    // bar and AppKit's own traffic lights sit at the start of the row, which is sized around them
    // (see `mac_titlebar`).
    #[cfg(target_os = "macos")]
    {
        viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    }
    if let Some(icon) = icon {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        centered: true,
        system_font_fallback: false,
        // On macOS the app installs its own menu bar (see `mac_menu`).
        #[cfg(target_os = "macos")]
        event_loop_builder: Some(Box::new(|builder| {
            use winit::platform::macos::EventLoopBuilderExtMacOS;
            builder.with_default_menu(false);
        })),
        ..Default::default()
    };
    eframe::run_native("bgpaint", options, Box::new(move |cc| Ok(Box::new(app::PaintApp::new(cc, settings, open, instance, started)))))
}
