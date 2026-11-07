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
use bgpaint_core::geom::{DirtyRect, Point};
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
}

impl Default for Colors {
    fn default() -> Self {
        Self { primary: Color::BLACK, secondary: Color::WHITE, editing_secondary: false, custom: [None; 10] }
    }
}

/// Transient UI state.
pub struct UiState {
    pub open_menu: Option<MenuKind>,
    /// Screen rect of the button that opened the menu (clicking it again closes the menu).
    pub menu_anchor: egui::Rect,
    pub show_layers: bool,
    pub settings_open: bool,
    pub fullscreen: bool,
    pub thumbnail: bool,
    pub modal: Option<Modal>,
    /// Auto-hidden toolbar temporarily revealed.
    pub ribbon_revealed: bool,
    /// The user manually collapsed the toolbar with the chevron.
    pub ribbon_collapsed: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            open_menu: None,
            menu_anchor: egui::Rect::NOTHING,
            show_layers: false,
            settings_open: false,
            fullscreen: false,
            thumbnail: false,
            modal: None,
            ribbon_revealed: false,
            ribbon_collapsed: false,
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
    pub thumbs: crate::ui::layers::Thumbnails,
    #[cfg(feature = "automation")]
    automation: Option<crate::automation::Automation>,
}

impl PaintApp {
    pub fn new(cc: &eframe::CreationContext<'_>, settings: Settings, open: Option<std::path::PathBuf>, started: Instant) -> Self {
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
            thumbs: Default::default(),
            #[cfg(feature = "automation")]
            automation: crate::automation::Automation::from_env(),
        };
        app.sync_theme(&cc.egui_ctx);
        if let Some(path) = open {
            app.open_path(&cc.egui_ctx, path);
        }
        app
    }

    /// Picks light or dark tokens from the setting and the system theme.
    fn sync_theme(&mut self, ctx: &egui::Context) {
        let dark = match self.settings.theme {
            ThemeMode::Light => false,
            ThemeMode::Dark => true,
            ThemeMode::System => ctx.system_theme().is_some_and(|t| t == egui::Theme::Dark),
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
        if self.ui.modal.is_some() || self.ui.settings_open || ctx.egui_wants_keyboard_input() {
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
                    egui::Event::Paste(_) => fired.push(Command::Paste),
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
            if self.modified() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.confirm_unsaved(crate::files::Pending::Exit);
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
        self.sync_theme(&ctx);
        self.poll_async(&ctx);
        self.handle_close_request(&ctx);
        self.handle_dropped_files(&ctx);
        self.selection_keys(&ctx);
        self.handle_shortcuts(&ctx);

        if self.ui.settings_open {
            self.settings_page(ui);
        } else {
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
        }
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

/// Helper used by tools to convert a pointer position to canvas coordinates.
pub fn canvas_point(view: &View, p: egui::Pos2) -> Point {
    view.to_canvas(p)
}
