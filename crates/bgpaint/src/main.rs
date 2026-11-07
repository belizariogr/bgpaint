//! BGPaint: a fast clone of Windows 11 Paint for Linux and macOS.
#![forbid(unsafe_code)]
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
mod mac_titlebar;
mod platform;
mod settings;
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
    let open: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(std::path::PathBuf::from).collect();
    let (w, h) = settings.window_size.unwrap_or((1280.0, 800.0));
    let tr = i18n::detect();
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png")).ok();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(format!("{} - {}", tr.untitled, tr.app_name))
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
    eframe::run_native("bgpaint", options, Box::new(move |cc| Ok(Box::new(app::PaintApp::new(cc, settings, open, started)))))
}
