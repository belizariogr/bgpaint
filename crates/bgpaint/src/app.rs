//! Application state and the per-frame layout.

use crate::canvas::{tiles::Checker, tiles::Tiles, view::View};
use crate::commands::{self, Command};
use crate::files::FileState;
use crate::i18n::{self, Strings};
use crate::settings::{Settings, ThemeMode};
use crate::theme::Theme;
use crate::tools::{Tool, ToolSettings};
use crate::ui::dialogs::Modal;
use crate::ui::menubar::MenuKind;
use bgpaint_core::color::Color;
use bgpaint_core::document::{DEFAULT_HEIGHT, DEFAULT_WIDTH, Document};
use bgpaint_core::geom::DirtyRect;
use bgpaint_core::history::History;
use std::time::Instant;

/// The two active colors and the palette's custom slots.
#[derive(Clone, Debug)]
pub struct Colors {
    pub primary: Color,
    pub secondary: Color,
    /// Which swatch the palette writes to (false = Color 1).
    pub editing_secondary: bool,
    pub custom: [Option<Color>; 10],
    /// The palette swatch last clicked. A selected custom slot follows the active color,
    /// so it can be redefined with "Edit colors" or the color picker.
    pub selected: Option<Swatch>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Swatch {
    Palette(usize),
    Custom(usize),
}

impl Default for Colors {
    fn default() -> Self {
        Self { primary: Color::BLACK, secondary: Color::WHITE, editing_secondary: false, custom: [None; 10], selected: None }
    }
}

impl Colors {
    pub fn active(&self) -> Color {
        if self.editing_secondary { self.secondary } else { self.primary }
    }
}

/// Transient UI state.
pub struct UiState {
    pub open_menu: Option<MenuKind>,
    /// Screen rect of the button that opened the menu (clicking it again closes the menu).
    pub menu_anchor: egui::Rect,
    pub show_layers: bool,
    pub fullscreen: bool,
    pub thumbnail: bool,
    pub modal: Option<Modal>,
    /// Auto-hidden toolbar temporarily revealed.
    pub ribbon_revealed: bool,
    /// The user manually collapsed the toolbar with the chevron.
    pub ribbon_collapsed: bool,
    /// Horizontal extent of the active tab, where the toolbar's top border is left open so
    /// the tab and the toolbar read as one surface.
    pub active_tab_span: Option<egui::Rangef>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            open_menu: None,
            menu_anchor: egui::Rect::NOTHING,
            show_layers: false,
            fullscreen: false,
            thumbnail: false,
            modal: None,
            ribbon_revealed: false,
            ribbon_collapsed: false,
            active_tab_span: None,
        }
    }
}

/// What the status bar shows.
#[derive(Default, Clone, Copy)]
pub struct Status {
    pub cursor: Option<(i32, i32)>,
    pub selection: Option<(u32, u32)>,
}

pub struct PaintApp {
    /// The active document (see [`crate::tabs`]).
    pub doc: Document,
    pub history: History,
    pub file: FileState,
    pub view: View,
    pub tiles: Tiles,
    pub checker: Checker,
    pub tool: Tool,
    /// Tool to return to after a one-shot tool (color picker).
    pub previous_tool: Tool,
    pub tools: ToolSettings,
    pub colors: Colors,
    pub settings: Settings,
    pub tr: &'static Strings,
    pub theme: Theme,
    theme_dark: Option<bool>,
    pub ui: UiState,
    pub status: Status,
    /// Canvas area touched by tool previews since the last frame.
    pub preview_dirty: DirtyRect,
    pub toast: Option<(String, Instant)>,
    pub allow_close: bool,
    pub started: Instant,
    pub(crate) files: crate::files::Async,
    pub active: crate::tools::interaction::Active,
    pub text_settings: crate::tools::text::TextSettings,
    pub print_settings: crate::platform::PrintSettings,
    pub thumbs: crate::ui::layers::Thumbnails,
    /// The other open documents.
    pub tabs: Vec<crate::tabs::DocTab>,
    /// Position of the active document in the tab strip.
    pub current_tab: usize,
    /// Stable id of the active document (ids survive reordering and closing other tabs).
    pub tab_id: u64,
    pub next_tab_id: u64,
    /// Tabs whose changes the user chose to discard during the current exit.
    /// (tab id → history state discarded); a tab edited again afterwards is asked about again.
    pub exit_discarded: std::collections::HashMap<u64, u64>,
    /// Tab to go back to after closing a background tab.
    pub return_to_tab: Option<u64>,
    pub updater: crate::update::Updater,
    #[cfg(feature = "automation")]
    automation: Option<crate::automation::Automation>,
    /// The global menu bar on macOS.
    #[cfg(target_os = "macos")]
    native_menu: Option<crate::mac_menu::NativeMenu>,
}

impl PaintApp {
    pub fn new(cc: &eframe::CreationContext<'_>, settings: Settings, open: Vec<std::path::PathBuf>, started: Instant) -> Self {
        crate::theme::install_fonts(&cc.egui_ctx);
        let mut colors = Colors::default();
        for (slot, hex) in colors.custom.iter_mut().zip(&settings.custom_colors) {
            *slot = Color::parse_hex(hex);
        }
        let doc = Document::new(DEFAULT_WIDTH, DEFAULT_HEIGHT).expect("default canvas size is valid");
        let mut app = Self {
            doc,
            history: History::default(),
            file: FileState::default(),
            view: View::default(),
            tiles: Tiles::default(),
            checker: Checker::default(),
            tool: Tool::Brush,
            previous_tool: Tool::Brush,
            tools: ToolSettings::default(),
            colors,
            settings,
            tr: i18n::detect(),
            theme: Theme::light(),
            theme_dark: None,
            ui: UiState::default(),
            status: Status::default(),
            preview_dirty: DirtyRect::default(),
            toast: None,
            allow_close: false,
            started,
            files: crate::files::Async::default(),
            active: Default::default(),
            text_settings: Default::default(),
            print_settings: Default::default(),
            thumbs: Default::default(),
            tabs: Vec::new(),
            current_tab: 0,
            tab_id: 0,
            next_tab_id: 1,
            exit_discarded: Default::default(),
            return_to_tab: None,
            updater: Default::default(),
            #[cfg(feature = "automation")]
            automation: crate::automation::Automation::from_env(),
            #[cfg(target_os = "macos")]
            native_menu: None,
        };
        #[cfg(target_os = "macos")]
        {
            app.native_menu = Some(crate::mac_menu::NativeMenu::install(&app, &cc.egui_ctx));
        }
        crate::platform::repaint_on_desktop_theme_change(&cc.egui_ctx);
        app.sync_theme(&cc.egui_ctx);
        crate::tools::text::preload_fonts();
        for path in open {
            app.open_path(&cc.egui_ctx, path);
        }
        if app.settings.check_updates && crate::update::automatic_checks_allowed() {
            app.updater.check(&cc.egui_ctx, false);
        }
        app
    }

    /// Picks light or dark tokens from the setting and the system theme (reported by winit on
    /// macOS, read from the desktop settings on Linux; light when neither says).
    fn sync_theme(&mut self, ctx: &egui::Context) {
        let dark = match self.settings.theme {
            ThemeMode::Light => false,
            ThemeMode::Dark => true,
            ThemeMode::System => ctx.system_theme().map(|t| t == egui::Theme::Dark).or_else(crate::platform::desktop_dark).unwrap_or(false),
        };
        if self.theme_dark != Some(dark) {
            self.theme_dark = Some(dark);
            self.theme = if dark { Theme::dark() } else { Theme::light() };
            ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
            self.theme.apply(ctx);
            ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(match self.settings.theme {
                ThemeMode::Light => egui::SystemTheme::Light,
                ThemeMode::Dark => egui::SystemTheme::Dark,
                ThemeMode::System => egui::SystemTheme::SystemDefault,
            }));
        }
    }

    pub fn set_theme_mode(&mut self, ctx: &egui::Context, mode: ThemeMode) {
        self.settings.theme = mode;
        self.theme_dark = None;
        self.sync_theme(ctx);
        self.settings.save();
    }

    pub fn modified(&self) -> bool {
        self.history.state_id() != self.file.saved_state
    }

    /// Closing would lose something: there are changes, or the file was deleted from disk.
    pub fn unsaved(&self) -> bool {
        self.modified() || self.file.missing_on_disk()
    }

    pub fn show_toast(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    /// Replaces the document (new/open), resetting history and view.
    pub fn set_document(&mut self, doc: Document) {
        self.cancel_interaction();
        self.doc = doc;
        self.history.clear();
        self.file.saved_state = self.history.state_id();
        self.view.reset();
        self.view.request_fit_if_large(self.doc.width(), self.doc.height());
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if self.ui.modal.is_some() || ctx.egui_wants_keyboard_input() {
            return;
        }
        let mut fired = Vec::new();
        ctx.input_mut(|i| {
            for (sc, cmd) in commands::shortcuts() {
                if i.consume_shortcut(&sc) {
                    fired.push(cmd);
                }
            }
            for e in &i.events {
                match e {
                    egui::Event::Copy => fired.push(Command::Copy),
                    egui::Event::Cut => fired.push(Command::Cut),
                    // egui-winit sends `PasteImage` when the clipboard holds an image but no text.
                    egui::Event::Paste(_) | egui::Event::PasteImage(_) => fired.push(Command::Paste),
                    _ => {}
                }
            }
        });
        for cmd in fired {
            self.execute(ctx, cmd);
        }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.any_unsaved() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.begin_exit();
            } else {
                self.allow_close = true;
            }
        }
    }

    fn update_title(&self, ctx: &egui::Context) {
        let name = self.file.display_name(self.tr);
        let title = format!("{}{name} - {}", if self.modified() { "*" } else { "" }, self.tr.app_name);
        let current = ctx.input(|i| i.viewport().title.clone());
        if current.as_deref() != Some(title.as_str()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
    }

    fn remember_window(&mut self, ctx: &egui::Context) {
        let (size, maximized) = ctx.input(|i| (i.viewport().inner_rect.map(|r| r.size()), i.viewport().maximized.unwrap_or(false)));
        if let Some(s) = size
            && !maximized
            && !self.ui.fullscreen
        {
            self.settings.window_size = Some((s.x, s.y));
        }
        self.settings.maximized = maximized;
    }

    /// Shows the update dialog when the updater has news (over Settings, not over other dialogs).
    fn poll_updater(&mut self) {
        self.updater.poll();
        if self.updater.wants_dialog() && matches!(self.ui.modal, None | Some(Modal::Settings)) {
            self.ui.modal = Some(Modal::Update);
        }
    }

    fn paint_toast(&mut self, ctx: &egui::Context) {
        let Some((text, at)) = &self.toast else { return };
        let age = at.elapsed().as_secs_f32();
        if age > 3.0 {
            self.toast = None;
            return;
        }
        let t = &self.theme;
        egui::Area::new(egui::Id::unique("toast"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -56.0))
            .interactable(false)
            .show(ctx, |ui| {
                crate::ui::widgets::panel_frame(t).inner_margin(egui::Margin::symmetric(16, 10)).show(ui, |ui| {
                    ui.label(egui::RichText::new(text.as_str()).color(t.text).size(14.0));
                });
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}

impl View {
    /// Large images open zoomed out to fit, as in Paint.
    pub fn request_fit_if_large(&mut self, w: u32, h: u32) {
        if w > 1600 || h > 1000 {
            self.request_fit();
        } else {
            self.zoom = 1.0;
        }
    }
}

impl eframe::App for PaintApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Linux desktops don't notify winit of theme changes: read it again on coming back.
        if ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(true)))) {
            crate::platform::refresh_desktop_theme();
        }
        self.sync_theme(&ctx);
        self.handle_dropped_files(&ctx);
        #[cfg(target_os = "macos")]
        if let Some(mut menu) = self.native_menu.take() {
            menu.update(self, &ctx);
            self.native_menu = Some(menu);
        }
        self.poll_async(&ctx);
        self.handle_close_request(&ctx);
        self.selection_keys(&ctx);
        self.text_keyboard(&ctx);
        self.handle_shortcuts(&ctx);

        self.menubar(ui);
        self.ribbon_panel(ui);
        self.tool_bars(&ctx);
        if self.settings.show_status_bar {
            self.statusbar(ui);
        }
        if self.ui.show_layers {
            self.layers_panel(ui);
        }
        self.canvas_panel(ui);
        self.open_menus(&ctx);
        self.poll_updater();
        self.modals(&ctx);
        self.paint_toast(&ctx);
        self.update_title(&ctx);
        self.remember_window(&ctx);
        if self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.settings.custom_colors = self.colors.custom.iter().map(|c| c.map(|c| c.to_hex_string()).unwrap_or_default()).collect();
        self.settings.save();
        if self.updater.relaunch {
            // The open files that are on disk come back in the new version.
            let files: Vec<std::path::PathBuf> = std::iter::once(&self.file.path)
                .chain(self.tabs.iter().map(|t| &t.file.path))
                .flatten()
                .filter(|p| p.exists())
                .cloned()
                .collect();
            self.updater.relaunch(&files);
        }
    }

    #[cfg(feature = "automation")]
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(a) = &mut self.automation {
            a.hook(ctx, raw_input);
            if a.quit {
                self.file.saved_state = self.history.state_id();
                self.allow_close = true;
            }
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.theme.window_bg.to_normalized_gamma_f32()
    }
}
