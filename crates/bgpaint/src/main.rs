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
mod platform;
mod settings;
mod system;
mod theme;
mod tools;
mod ui;

use std::time::Instant;

fn main() -> eframe::Result {
    let started = Instant::now();
    let settings = settings::Settings::load();
    let open = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let (w, h) = settings.window_size.unwrap_or((1280.0, 800.0));
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png")).ok();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(format!("{} - {}", i18n::detect().untitled, i18n::detect().app_name))
        .with_app_id("io.github.belizariogr.bgpaint")
        .with_inner_size([w.max(640.0), h.max(480.0)])
        .with_min_inner_size([720.0, 480.0])
        .with_maximized(settings.maximized)
        .with_drag_and_drop(true);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        centered: true,
        system_font_fallback: false,
        ..Default::default()
    };
    eframe::run_native("bgpaint", options, Box::new(move |cc| Ok(Box::new(app::PaintApp::new(cc, settings, open, started)))))
}
