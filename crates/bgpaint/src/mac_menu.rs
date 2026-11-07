//! The macOS menu bar. On a Mac the app's commands live in the global menu at the top of the
//! screen, as in every native app, instead of the File/Edit/View buttons drawn in the window.
//!
//! Menu items map to the same [`Command`]s as the rest of the UI. Item state (enabled, checked)
//! follows the app every frame, and only changes are sent to AppKit.

use crate::app::PaintApp;
use crate::commands::{Command, WallpaperMode};
use crate::settings::Settings;
use crate::ui::settings_dialog::REPO_URL;
use bgpaint_core::io::SaveFormat;
use bgpaint_core::raster::QuarterTurn;
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

enum Action {
    Run(Command),
    OpenUrl(&'static str),
}

enum Dynamic {
    /// Enabled when the predicate holds.
    Enabled(MenuItem, fn(&PaintApp) -> bool),
    /// Checked when the predicate holds.
    Checked(CheckMenuItem, fn(&PaintApp) -> bool),
}

pub struct NativeMenu {
    /// The menu must stay alive while it is installed.
    _menu: Menu,
    actions: HashMap<MenuId, Action>,
    dynamic: Vec<(Dynamic, Option<bool>)>,
    recent: Submenu,
    recent_shown: Option<Vec<PathBuf>>,
    recent_ids: Vec<MenuId>,
    next_id: usize,
    rx: Receiver<MenuEvent>,
}

/// Strings that only the Mac menu uses.
struct MacStrings {
    about: &'static str,
    settings: &'static str,
    services: &'static str,
    hide: &'static str,
    hide_others: &'static str,
    show_all: &'static str,
    quit: &'static str,
    image: &'static str,
    window: &'static str,
    help: &'static str,
    close_tab: &'static str,
    minimize: &'static str,
    zoom_window: &'static str,
    bring_all: &'static str,
    website: &'static str,
}

const MAC_EN: MacStrings = MacStrings {
    about: "About BGPaint",
    settings: "Settings…",
    services: "Services",
    hide: "Hide BGPaint",
    hide_others: "Hide Others",
    show_all: "Show All",
    quit: "Quit BGPaint",
    image: "Image",
    window: "Window",
    help: "Help",
    close_tab: "Close Tab",
    minimize: "Minimize",
    zoom_window: "Zoom",
    bring_all: "Bring All to Front",
    website: "BGPaint on GitHub",
};

const MAC_PT: MacStrings = MacStrings {
    about: "Sobre o BGPaint",
    settings: "Ajustes…",
    services: "Serviços",
    hide: "Ocultar BGPaint",
    hide_others: "Ocultar Outros",
    show_all: "Mostrar Tudo",
    quit: "Encerrar BGPaint",
    image: "Imagem",
    window: "Janela",
    help: "Ajuda",
    close_tab: "Fechar Aba",
    minimize: "Minimizar",
    zoom_window: "Zoom",
    bring_all: "Trazer Todas para a Frente",
    website: "BGPaint no GitHub",
};

fn cmd(code: Code) -> Option<Accelerator> {
    Some(Accelerator::new(Modifiers::META, code))
}

fn cmd_shift(code: Code) -> Option<Accelerator> {
    Some(Accelerator::new(Modifiers::META | Modifiers::SHIFT, code))
}

/// Collects items and what they do while the menu is built.
struct Builder {
    actions: HashMap<MenuId, Action>,
    dynamic: Vec<(Dynamic, Option<bool>)>,
    next_id: usize,
}

impl Builder {
    fn id(&mut self) -> MenuId {
        self.next_id += 1;
        MenuId::new(format!("bgpaint-{}", self.next_id))
    }

    fn item(&mut self, text: &str, command: Command, accelerator: Option<Accelerator>) -> MenuItem {
        let id = self.id();
        self.actions.insert(id.clone(), Action::Run(command));
        MenuItem::with_id(id, text, true, accelerator)
    }

    fn item_when(&mut self, text: &str, command: Command, accelerator: Option<Accelerator>, when: fn(&PaintApp) -> bool) -> MenuItem {
        let item = self.item(text, command, accelerator);
        self.dynamic.push((Dynamic::Enabled(item.clone(), when), None));
        item
    }

    fn check(&mut self, text: &str, command: Command, checked: fn(&PaintApp) -> bool) -> CheckMenuItem {
        let id = self.id();
        self.actions.insert(id.clone(), Action::Run(command));
        let item = CheckMenuItem::with_id(id, text, true, false, None);
        self.dynamic.push((Dynamic::Checked(item.clone(), checked), None));
        item
    }

    fn url(&mut self, text: &str, url: &'static str) -> MenuItem {
        let id = self.id();
        self.actions.insert(id.clone(), Action::OpenUrl(url));
        MenuItem::with_id(id, text, true, None)
    }
}

fn submenu(text: &str, items: &[&dyn IsMenuItem]) -> Submenu {
    Submenu::with_items(text, true, items).unwrap_or_else(|_| Submenu::new(text, true))
}

impl NativeMenu {
    /// Builds the menu bar and installs it in the application. Call on the main thread, once
    /// the application exists (from the app's constructor).
    pub fn install(app: &PaintApp, ctx: &egui::Context) -> Self {
        let tr = app.tr;
        let m = if std::ptr::eq(tr, &crate::i18n::PT_BR) { &MAC_PT } else { &MAC_EN };
        let mut b = Builder { actions: HashMap::new(), dynamic: Vec::new(), next_id: 0 };
        let sep = PredefinedMenuItem::separator;
        let has_sel: fn(&PaintApp) -> bool = PaintApp::has_selection;

        let about = b.item(m.about, Command::OpenSettings, None);
        let settings = b.item(m.settings, Command::OpenSettings, cmd(Code::Comma));
        let quit = b.item(m.quit, Command::Exit, cmd(Code::KeyQ));
        let services = Submenu::new(m.services, true);
        let app_menu = submenu(
            "BGPaint",
            &[
                &about,
                &sep(),
                &settings,
                &sep(),
                &services,
                &sep(),
                &PredefinedMenuItem::hide(Some(m.hide)),
                &PredefinedMenuItem::hide_others(Some(m.hide_others)),
                &PredefinedMenuItem::show_all(Some(m.show_all)),
                &sep(),
                &quit,
            ],
        );

        let new = b.item(tr.new, Command::New, cmd(Code::KeyN));
        let open = b.item(&format!("{}…", tr.open), Command::Open, cmd(Code::KeyO));
        let recent = Submenu::new(tr.recent, true);
        let import = b.item(&format!("{}…", tr.import_to_canvas), Command::ImportFromFile, None);
        let close_tab = b.item(m.close_tab, Command::CloseTab(usize::MAX), cmd(Code::KeyW));
        let save = b.item(tr.save, Command::Save, cmd(Code::KeyS));
        let formats = [
            (tr.png_image, SaveFormat::Png),
            (tr.jpeg_image, SaveFormat::Jpeg),
            (tr.bmp_image, SaveFormat::Bmp),
            (tr.gif_image, SaveFormat::Gif),
            (tr.webp_image, SaveFormat::Webp),
        ]
        .map(|(text, f)| b.item(text, Command::SaveAs(Some(f)), None));
        let other = b.item(tr.other_format, Command::SaveAs(None), None);
        let mut save_as_items: Vec<&dyn IsMenuItem> = formats.iter().map(|i| i as &dyn IsMenuItem).collect();
        save_as_items.push(&other);
        let save_as = submenu(tr.save_as, &save_as_items);
        let save_project = b.item(tr.save_as_project, Command::SaveProject, cmd_shift(Code::KeyS));
        let page_setup = b.item(&format!("{}…", tr.page_setup), Command::PageSetup, cmd_shift(Code::KeyP));
        let print = b.item(&format!("{}…", tr.print), Command::Print, cmd(Code::KeyP));
        let share = b.item(tr.share, Command::Share, None);
        let wallpaper = [
            (tr.wallpaper_fill, WallpaperMode::Fill),
            (tr.wallpaper_tile, WallpaperMode::Tile),
            (tr.wallpaper_center, WallpaperMode::Center),
        ]
        .map(|(text, mode)| b.item(text, Command::SetWallpaper(mode), None));
        let wallpaper = submenu(tr.set_as_wallpaper, &[&wallpaper[0], &wallpaper[1], &wallpaper[2]]);
        let properties = b.item(&format!("{}…", tr.image_properties), Command::ImageProperties, cmd(Code::KeyE));
        let file_menu = submenu(
            tr.menu_file,
            &[
                &new,
                &open,
                &recent,
                &import,
                &sep(),
                &close_tab,
                &save,
                &save_as,
                &save_project,
                &sep(),
                &page_setup,
                &print,
                &sep(),
                &share,
                &wallpaper,
                &sep(),
                &properties,
            ],
        );

        // Editing shortcuts (⌘Z, ⌘C, ⌘A…) are left to the window, so text fields and the
        // text tool keep them.
        let undo = b.item_when(tr.undo, Command::Undo, None, |a| a.history.can_undo() || !a.active.is_idle());
        let redo = b.item_when(tr.redo, Command::Redo, None, |a| a.history.can_redo());
        let cut = b.item_when(tr.cut, Command::Cut, None, has_sel);
        let copy = b.item_when(tr.copy, Command::Copy, None, has_sel);
        let paste = b.item(tr.paste, Command::Paste, None);
        let copy_visible = b.item(tr.copy_visible_layers, Command::CopyVisible, None);
        let select_all = b.item(tr.select_all, Command::SelectAll, None);
        let invert_sel = b.item_when(tr.invert_selection, Command::InvertSelection, None, has_sel);
        let delete = b.item_when(tr.delete, Command::DeleteSelection, None, |a| {
            a.has_selection() || matches!(a.active, crate::tools::interaction::Active::Shape(_))
        });
        let transparent = b.check(tr.transparent_selection, Command::ToggleTransparentSelection, |a| a.tools.transparent_selection);
        let edit_menu = submenu(
            tr.menu_edit,
            &[&undo, &redo, &sep(), &cut, &copy, &paste, &copy_visible, &sep(), &select_all, &invert_sel, &delete, &transparent],
        );

        let crop = b.item_when(tr.crop, Command::Crop, None, has_sel);
        let resize = b.item(&format!("{}…", tr.resize_and_skew), Command::ResizeSkew, None);
        let rotate = [
            b.item(tr.rotate_right, Command::Rotate(QuarterTurn::Cw90), None),
            b.item(tr.rotate_left, Command::Rotate(QuarterTurn::Ccw90), None),
            b.item(tr.rotate_180, Command::Rotate(QuarterTurn::Half), None),
            b.item(&format!("{}…", tr.rotate_custom), Command::RotateCustom, None),
        ];
        let rotate = submenu(tr.rotate, &[&rotate[0], &rotate[1], &rotate[2], &rotate[3]]);
        let flip_v = b.item(tr.flip_vertical, Command::FlipVertical, None);
        let flip_h = b.item(tr.flip_horizontal, Command::FlipHorizontal, None);
        let flip = submenu(tr.flip, &[&flip_v, &flip_h]);
        let invert = b.item(tr.invert_color, Command::InvertColors, cmd_shift(Code::KeyI));
        let edit_colors = b.item(&format!("{}…", tr.edit_colors), Command::EditColors, None);
        let image_menu = submenu(m.image, &[&crop, &resize, &rotate, &flip, &invert, &sep(), &edit_colors]);

        let zoom_in = b.item(tr.zoom_in, Command::ZoomIn, cmd(Code::Equal));
        let zoom_out = b.item(tr.zoom_out, Command::ZoomOut, cmd(Code::Minus));
        let zoom_100 = b.item(tr.zoom_100, Command::Zoom100, cmd(Code::Digit0));
        let zoom_fit = b.item(tr.fit_to_window, Command::ZoomFit, cmd(Code::Digit1));
        let rulers = b.check(tr.rulers, Command::ToggleRulers, |a| a.settings.show_rulers);
        let grid = b.check(tr.gridlines, Command::ToggleGrid, |a| a.settings.show_grid);
        let status = b.check(tr.status_bar, Command::ToggleStatusBar, |a| a.settings.show_status_bar);
        let auto_hide = b.check(tr.auto_hide_toolbar, Command::SetAutoHideToolbar(true), |a| a.settings.auto_hide_toolbar);
        let always = b.check(tr.always_show_toolbar, Command::SetAutoHideToolbar(false), |a| !a.settings.auto_hide_toolbar);
        let thumbnail = b.check(tr.thumbnail, Command::ToggleThumbnail, |a| a.ui.thumbnail);
        let layers = b.check(tr.layers, Command::ToggleLayersPanel, |a| a.ui.show_layers);
        let view_menu = submenu(
            tr.menu_view,
            &[
                &zoom_in,
                &zoom_out,
                &zoom_100,
                &zoom_fit,
                &sep(),
                &rulers,
                &grid,
                &status,
                &sep(),
                &always,
                &auto_hide,
                &sep(),
                &thumbnail,
                &layers,
                &sep(),
                &PredefinedMenuItem::fullscreen(None),
            ],
        );

        let window_menu = submenu(
            m.window,
            &[
                &PredefinedMenuItem::minimize(Some(m.minimize)),
                &PredefinedMenuItem::maximize(Some(m.zoom_window)),
                &sep(),
                &PredefinedMenuItem::bring_all_to_front(Some(m.bring_all)),
            ],
        );
        let website = b.url(m.website, REPO_URL);
        let help_menu = submenu(m.help, &[&website]);

        let menu = Menu::new();
        for sub in [&app_menu, &file_menu, &edit_menu, &image_menu, &view_menu, &window_menu, &help_menu] {
            let _ = menu.append(sub);
        }
        menu.init_for_nsapp();
        window_menu.set_as_windows_menu_for_nsapp();
        help_menu.set_as_help_menu_for_nsapp();

        // Menu clicks arrive while the app may be idle: wake it up.
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = tx.send(e);
            ctx.request_repaint();
        }));

        Self {
            _menu: menu,
            actions: b.actions,
            dynamic: b.dynamic,
            recent,
            recent_shown: None,
            recent_ids: Vec::new(),
            next_id: b.next_id,
            rx,
        }
    }

    /// Runs the commands picked in the menu and brings item states up to date.
    pub fn update(&mut self, app: &mut PaintApp, ctx: &egui::Context) {
        while let Ok(e) = self.rx.try_recv() {
            // A dialog is modal for the menu too.
            if app.ui.modal.is_some() {
                continue;
            }
            match self.actions.get(e.id()) {
                Some(Action::Run(c)) => app.execute(ctx, c.clone()),
                Some(Action::OpenUrl(url)) => crate::platform::open_url(url),
                None => {}
            }
        }
        for (d, last) in &mut self.dynamic {
            let now = match d {
                Dynamic::Enabled(_, f) | Dynamic::Checked(_, f) => f(app),
            };
            if *last != Some(now) {
                *last = Some(now);
                match d {
                    Dynamic::Enabled(item, _) => item.set_enabled(now),
                    Dynamic::Checked(item, _) => item.set_checked(now),
                }
            }
        }
        self.sync_recent(&app.settings, app.tr);
    }

    fn sync_recent(&mut self, settings: &Settings, tr: &crate::i18n::Strings) {
        if self.recent_shown.as_ref() == Some(&settings.recent_files) {
            return;
        }
        for id in self.recent_ids.drain(..) {
            self.actions.remove(&id);
        }
        while self.recent.remove_at(0).is_some() {}
        if settings.recent_files.is_empty() {
            let _ = self.recent.append(&MenuItem::new(tr.no_recent, false, None));
        }
        for path in &settings.recent_files {
            self.next_id += 1;
            let id = MenuId::new(format!("bgpaint-recent-{}", self.next_id));
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            self.actions.insert(id.clone(), Action::Run(Command::OpenRecent(path.clone())));
            self.recent_ids.push(id.clone());
            let _ = self.recent.append(&MenuItem::with_id(id, name, true, None));
        }
        self.recent_shown = Some(settings.recent_files.clone());
    }
}
