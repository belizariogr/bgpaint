//! The top row (File/Edit/View menus and quick actions) and every dropdown menu of the app.

use super::icons::regular as ic;
use super::menu::{self, Entry, Preview};
use super::widgets::{self, IconButton};
use crate::app::PaintApp;
use crate::commands::{Command, WallpaperMode, shortcut_label};
use crate::tools::{SHAPE_WIDTHS, SelectMode};
use bgpaint_core::io::SaveFormat;
use bgpaint_core::raster::QuarterTurn;
use bgpaint_core::shapes::ShapeStyle;
use egui::{Align2, Id, Rect, Sense, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuKind {
    File,
    Edit,
    View,
    Selection,
    Rotate,
    Flip,
    Brushes,
    Outline,
    Fill,
    Size,
    Zoom,
    Layer(usize),
    Background,
    Canvas(egui::Pos2),
}

/// Height of the top row. On macOS the row is also the title bar, so it is shorter and as tall as
/// AppKit's title bar, which centers the traffic lights in it.
pub fn menubar_height() -> f32 {
    #[cfg(target_os = "macos")]
    return crate::mac_titlebar::title_bar().height;
    #[cfg(not(target_os = "macos"))]
    40.0
}

impl PaintApp {
    pub fn toggle_menu(&mut self, kind: MenuKind, anchor: Rect) {
        if self.ui.open_menu == Some(kind) {
            self.ui.open_menu = None;
        } else {
            self.ui.open_menu = Some(kind);
            self.ui.menu_anchor = anchor;
        }
    }

    pub fn menubar(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        egui::Panel::top("menubar")
            .exact_size(menubar_height())
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(t.window_bg))
            .show(ui, |ui| {
                let r = ui.max_rect();
                let cy = r.center().y;
                // Buttons fill the row, up to 32 pt (the row can be shorter on macOS).
                let bh = r.height().min(32.0);
                #[cfg(target_os = "macos")]
                let mut x = title_bar(ui, r);
                #[cfg(not(target_os = "macos"))]
                let mut x = r.left() + 4.0;
                // On macOS these menus live in the global menu bar (see `mac_menu`).
                let in_window_menus: &[(MenuKind, &str)] = if cfg!(target_os = "macos") {
                    &[]
                } else {
                    &[(MenuKind::File, self.tr.menu_file), (MenuKind::Edit, self.tr.menu_edit), (MenuKind::View, self.tr.menu_view)]
                };
                for &(kind, text) in in_window_menus {
                    let w = widgets::text_width(ui, text, egui::FontId::proportional(14.0)) + 20.0;
                    let rect = Rect::from_min_size(pos2(x, cy - bh / 2.0), vec2(w, bh));
                    let resp = ui.interact(rect, Id::unique(("menubar", text)), Sense::click());
                    let open = self.ui.open_menu == Some(kind);
                    let fill = if open { t.hover } else { widgets::state_fill(&t, &resp, true) };
                    ui.painter().rect_filled(rect, widgets::RADIUS, fill);
                    widgets::label(ui.painter(), rect.center(), Align2::CENTER_CENTER, text, 14.0, t.text);
                    if resp.clicked() {
                        self.toggle_menu(kind, rect);
                    } else if resp.hovered()
                        && self.ui.open_menu.is_some_and(|m| matches!(m, MenuKind::File | MenuKind::Edit | MenuKind::View))
                        && !open
                    {
                        // Moving across the menu bar switches menus, like a native menu bar.
                        self.ui.open_menu = Some(kind);
                        self.ui.menu_anchor = rect;
                    }
                    x += w + 2.0;
                }
                if !in_window_menus.is_empty() {
                    widgets::vseparator(ui.painter(), &t, x + 6.0, cy - 10.0, cy + 10.0);
                    x += 14.0;
                }
                let quick = [(ic::SAVE, Command::Save, self.tr.save, true), (ic::SHARE, Command::Share, self.tr.share, true)];
                for (glyph, cmd, tip, enabled) in quick {
                    let rect = Rect::from_min_size(pos2(x, cy - bh / 2.0), vec2(36.0, bh));
                    let tip = match shortcut_label(ui.ctx(), &cmd) {
                        Some(s) => format!("{tip} ({s})"),
                        None => tip.to_owned(),
                    };
                    if IconButton::new(glyph)
                        .size(18.0)
                        .enabled(enabled)
                        .tooltip(&tip)
                        .show(ui, &t, rect, Id::unique(("quick", glyph)))
                        .clicked()
                    {
                        self.execute(ui.ctx(), cmd);
                    }
                    x += 40.0;
                }
                widgets::vseparator(ui.painter(), &t, x + 2.0, cy - 10.0, cy + 10.0);
                x += 10.0;
                let strip = Rect::from_min_max(pos2(x, r.top()), pos2(r.right() - 52.0, r.bottom()));
                if let Some(cmd) = super::tab_strip::show(self, ui, &t, strip) {
                    self.execute(ui.ctx(), cmd);
                }
                let rect = Rect::from_min_size(pos2(r.right() - 44.0, cy - bh / 2.0), vec2(36.0, bh));
                if IconButton::new(ic::SETTINGS)
                    .size(18.0)
                    .tooltip(self.tr.settings)
                    .show(ui, &t, rect, Id::unique("settings-btn"))
                    .clicked()
                {
                    self.execute(ui.ctx(), Command::OpenSettings);
                }
            });
    }

    /// Renders the open dropdown menu (if any) and dispatches its action.
    pub fn open_menus(&mut self, ctx: &egui::Context) {
        let Some(kind) = self.ui.open_menu else { return };
        let anchor = self.ui.menu_anchor;
        let id = Id::unique(("menu", format!("{kind:?}")));
        let entries = self.menu_entries(ctx, kind);
        let pos = match kind {
            MenuKind::Canvas(p) => p,
            MenuKind::Layer(_) | MenuKind::Background => pos2(anchor.right() - 8.0, anchor.center().y),
            MenuKind::Zoom => pos2(anchor.left(), anchor.top() - 4.0 - menu_height(&entries)),
            _ => pos2(anchor.left(), anchor.bottom() + 4.0),
        };
        let out = menu::show(ctx, &self.theme, id, pos, &entries);
        if let Some(cmd) = out.action {
            self.ui.open_menu = None;
            menu::reset(ctx, id);
            self.remember_ribbon_default(kind, &cmd);
            self.execute(ctx, cmd);
            return;
        }
        let clicked_outside = ctx.input(|i| {
            (i.pointer.any_pressed() || i.pointer.any_click())
                && i.pointer.interact_pos().is_some_and(|p| !out.rects.iter().any(|r| r.contains(p)) && !anchor.contains(p))
        });
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if clicked_outside || escape {
            self.ui.open_menu = None;
            menu::reset(ctx, id);
        }
    }

    fn menu_entries(&self, ctx: &egui::Context, kind: MenuKind) -> Vec<Entry<Command>> {
        let tr = self.tr;
        let item = |icon: Option<char>, label: &str, cmd: Command| {
            let s = shortcut_label(ctx, &cmd);
            let e = Entry::item(icon, label, cmd);
            match s {
                Some(s) => e.shortcut(&s),
                None => e,
            }
        };
        let has_sel = self.has_selection();
        match kind {
            MenuKind::File => {
                let recent: Vec<Entry<Command>> = if self.settings.recent_files.is_empty() {
                    vec![Entry::item(None, tr.no_recent, Command::Escape).enabled(false)]
                } else {
                    self.settings
                        .recent_files
                        .iter()
                        .map(|p| {
                            Entry::item(
                                None,
                                &p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                                Command::OpenRecent(p.clone()),
                            )
                        })
                        .collect()
                };
                vec![
                    item(Some(ic::NEW), tr.new, Command::New),
                    item(Some(ic::OPEN), tr.open, Command::Open),
                    Entry::submenu(
                        Some(ic::IMPORT),
                        tr.import_to_canvas,
                        vec![
                            Entry::item(None, tr.from_file, Command::ImportFromFile),
                            Entry::item(None, tr.from_scanner, Command::Escape).enabled(false),
                        ],
                    ),
                    Entry::submenu(Some(ic::RECENT), tr.recent, recent).enabled(!self.settings.recent_files.is_empty()),
                    Entry::Separator,
                    item(Some(ic::SAVE), tr.save, Command::Save),
                    Entry::submenu(
                        Some(ic::SAVE_AS),
                        tr.save_as,
                        vec![
                            Entry::item(None, tr.png_image, Command::SaveAs(Some(SaveFormat::Png))),
                            Entry::item(None, tr.jpeg_image, Command::SaveAs(Some(SaveFormat::Jpeg))),
                            Entry::item(None, tr.bmp_image, Command::SaveAs(Some(SaveFormat::Bmp))),
                            Entry::item(None, tr.gif_image, Command::SaveAs(Some(SaveFormat::Gif))),
                            Entry::item(None, tr.webp_image, Command::SaveAs(Some(SaveFormat::Webp))),
                            Entry::item(None, tr.other_format, Command::SaveAs(None)),
                        ],
                    ),
                    item(Some(ic::SAVE_PROJECT), tr.save_as_project, Command::SaveProject),
                    Entry::Separator,
                    Entry::submenu(
                        Some(ic::PRINT),
                        tr.print,
                        vec![
                            item(None, tr.print, Command::Print),
                            Entry::item(None, tr.page_setup, Command::PageSetup),
                            Entry::item(None, tr.print_preview, Command::PrintPreview),
                        ],
                    ),
                    item(Some(ic::SHARE), tr.share, Command::Share),
                    Entry::Separator,
                    Entry::submenu(
                        Some(ic::WALLPAPER),
                        tr.set_as_wallpaper,
                        vec![
                            Entry::item(None, tr.wallpaper_fill, Command::SetWallpaper(WallpaperMode::Fill)),
                            Entry::item(None, tr.wallpaper_tile, Command::SetWallpaper(WallpaperMode::Tile)),
                            Entry::item(None, tr.wallpaper_center, Command::SetWallpaper(WallpaperMode::Center)),
                        ],
                    ),
                    Entry::Separator,
                    item(Some(ic::PROPERTIES), tr.image_properties, Command::ImageProperties),
                    Entry::Separator,
                    item(Some(ic::DISMISS), tr.exit, Command::Exit),
                ]
            }
            MenuKind::Edit => vec![
                item(Some(ic::CUT), tr.cut, Command::Cut).enabled(has_sel),
                item(Some(ic::COPY), tr.copy, Command::Copy).enabled(has_sel),
                item(Some(ic::PASTE), tr.paste, Command::Paste).enabled(crate::clipboard::has_image()),
                Entry::Separator,
                item(Some(ic::COPY_VISIBLE), tr.copy_visible_layers, Command::CopyVisible),
            ],
            MenuKind::View => vec![
                Entry::submenu(
                    Some(ic::ZOOM_IN),
                    tr.zoom,
                    vec![
                        item(None, tr.zoom_100, Command::Zoom100).enabled((self.view.zoom - 1.0).abs() > 1e-4),
                        item(None, tr.zoom_in, Command::ZoomIn),
                        item(None, tr.zoom_out, Command::ZoomOut),
                        item(None, tr.fit_to_window, Command::ZoomFit),
                    ],
                ),
                Entry::Separator,
                item(None, tr.rulers, Command::ToggleRulers).checked(self.settings.show_rulers),
                item(None, tr.gridlines, Command::ToggleGrid).checked(self.settings.show_grid),
                Entry::item(None, tr.status_bar, Command::ToggleStatusBar).checked(self.settings.show_status_bar),
                Entry::Separator,
                Entry::item(None, tr.always_show_toolbar, Command::SetAutoHideToolbar(false)).checked(!self.settings.auto_hide_toolbar),
                Entry::item(None, tr.auto_hide_toolbar, Command::SetAutoHideToolbar(true)).checked(self.settings.auto_hide_toolbar),
                Entry::Separator,
                item(Some(ic::FULLSCREEN), tr.full_screen, Command::ToggleFullscreen),
                Entry::item(Some(ic::THUMBNAIL), tr.thumbnail, Command::ToggleThumbnail).checked(self.ui.thumbnail),
            ],
            MenuKind::Selection => vec![
                Entry::item(Some(ic::SELECT), tr.rectangle_selection, Command::SetSelectMode(SelectMode::Rectangle))
                    .marked(self.tools.select_mode == SelectMode::Rectangle),
                Entry::item(Some(ic::LASSO), tr.freeform_selection, Command::SetSelectMode(SelectMode::FreeForm))
                    .marked(self.tools.select_mode == SelectMode::FreeForm),
                Entry::Separator,
                item(Some(ic::SELECT_ALL), tr.select_all, Command::SelectAll),
                item(Some(ic::INVERT_SELECTION), tr.invert_selection, Command::InvertSelection).enabled(has_sel),
                Entry::item(None, tr.transparent_selection, Command::ToggleTransparentSelection).checked(self.tools.transparent_selection),
                item(Some(ic::DELETE), tr.delete, Command::DeleteSelection).enabled(has_sel),
            ],
            MenuKind::Rotate => vec![
                Entry::item(Some(ic::ROTATE_RIGHT), tr.rotate_right, Command::Rotate(QuarterTurn::Cw90)),
                Entry::item(Some(ic::ROTATE_LEFT), tr.rotate_left, Command::Rotate(QuarterTurn::Ccw90)),
                Entry::item(Some(ic::ROTATE_CUSTOM), tr.rotate_180, Command::Rotate(QuarterTurn::Half)),
                Entry::item(Some(ic::RESET), tr.rotate_custom, Command::RotateCustom),
            ],
            MenuKind::Flip => vec![
                Entry::item(Some(ic::FLIP_V), tr.flip_vertical, Command::FlipVertical),
                Entry::item(Some(ic::FLIP_H), tr.flip_horizontal, Command::FlipHorizontal),
            ],
            MenuKind::Brushes => self.brush_menu_entries(ctx),
            MenuKind::Outline | MenuKind::Fill => {
                let outline = kind == MenuKind::Outline;
                let current = if outline { self.tools.outline } else { self.tools.fill };
                ShapeStyle::OPTIONS
                    .iter()
                    .map(|&s| {
                        let label = match (s, outline) {
                            (ShapeStyle::None, true) => tr.no_outline,
                            (ShapeStyle::None, false) => tr.no_fill,
                            (ShapeStyle::Solid, true) => tr.solid_outline,
                            (ShapeStyle::Solid, false) => tr.solid_fill,
                            (ShapeStyle::Textured(tex), _) => self.texture_name(tex),
                        };
                        let cmd = if outline { Command::SetOutline(s) } else { Command::SetFill(s) };
                        let preview = match s {
                            ShapeStyle::None => Preview::None,
                            ShapeStyle::Solid if outline => Preview::Line(3.0),
                            ShapeStyle::Solid => Preview::Solid,
                            ShapeStyle::Textured(tex) => self.texture_preview(ctx, tex),
                        };
                        Entry::item(None, label, cmd).marked(s == current).preview(preview)
                    })
                    .collect()
            }
            MenuKind::Size => SHAPE_WIDTHS
                .iter()
                .map(|&w| {
                    Entry::item(None, &format!("{w}px"), Command::SetSize(w))
                        .marked((self.tools.shape_width - w).abs() < 0.1)
                        .preview(Preview::Line(w))
                })
                .collect(),
            MenuKind::Zoom => {
                let mut v: Vec<Entry<Command>> = crate::canvas::view::PRESETS
                    .iter()
                    .rev()
                    .map(|&z| Entry::item(None, &format!("{}%", z * 100.0), Command::SetZoom(z)).marked((self.view.zoom - z).abs() < 1e-4))
                    .collect();
                v.push(Entry::Separator);
                v.push(Entry::item(None, tr.fit_to_window, Command::ZoomFit));
                v
            }
            MenuKind::Layer(i) => self.layer_menu_entries(ctx, i),
            MenuKind::Background => {
                let bg = self.doc.background();
                vec![
                    Entry::item(
                        Some(if bg.visible { ic::EYE_OFF } else { ic::EYE }),
                        if bg.visible { tr.hide_background } else { tr.show_background },
                        Command::ToggleBackground,
                    )
                    .shortcut(&shortcut_label(ctx, &Command::ToggleBackground).unwrap_or_default()),
                    Entry::item(Some(ic::COLOR), tr.change_color, Command::ChangeBackgroundColor)
                        .shortcut(&shortcut_label(ctx, &Command::ChangeBackgroundColor).unwrap_or_default()),
                ]
            }
            MenuKind::Canvas(_) => self.canvas_menu_entries(ctx),
        }
    }

    fn layer_menu_entries(&self, ctx: &egui::Context, i: usize) -> Vec<Entry<Command>> {
        let tr = self.tr;
        let n = self.doc.layers().len();
        let visible = self.doc.layers().get(i).is_some_and(|l| l.visible);
        let sc = |c: Command| shortcut_label(ctx, &c).unwrap_or_default();
        vec![
            Entry::item(
                Some(if visible { ic::EYE_OFF } else { ic::EYE }),
                if visible { tr.hide_layer } else { tr.show_layer },
                Command::ToggleLayerVisible(i),
            )
            .shortcut(&sc(Command::ToggleLayerVisible(usize::MAX))),
            Entry::item(Some(ic::DUPLICATE), tr.duplicate_layer, Command::DuplicateLayer(i)),
            Entry::item(Some(ic::MERGE), tr.merge_down, Command::MergeDown(i)).enabled(i > 0),
            Entry::item(Some(ic::ARROW_UP), tr.move_up, Command::MoveLayer(i, true))
                .enabled(i + 1 < n)
                .shortcut(&sc(Command::MoveLayer(usize::MAX, true))),
            Entry::item(Some(ic::ARROW_DOWN), tr.move_down, Command::MoveLayer(i, false))
                .enabled(i > 0)
                .shortcut(&sc(Command::MoveLayer(usize::MAX, false))),
            Entry::item(Some(ic::DELETE), tr.delete_layer, Command::DeleteLayer(i))
                .enabled(n > 1)
                .shortcut(&sc(Command::DeleteLayer(usize::MAX))),
        ]
    }

    fn canvas_menu_entries(&self, ctx: &egui::Context) -> Vec<Entry<Command>> {
        let tr = self.tr;
        let has_sel = self.has_selection();
        let item = |icon: Option<char>, label: &str, cmd: Command| {
            let s = shortcut_label(ctx, &cmd);
            let e = Entry::item(icon, label, cmd);
            match s {
                Some(s) => e.shortcut(&s),
                None => e,
            }
        };
        vec![
            item(Some(ic::CUT), tr.cut, Command::Cut).enabled(has_sel),
            item(Some(ic::COPY), tr.copy, Command::Copy).enabled(has_sel),
            item(Some(ic::PASTE), tr.paste, Command::Paste).enabled(crate::clipboard::has_image()),
            Entry::Separator,
            item(Some(ic::SELECT_ALL), tr.select_all, Command::SelectAll),
            item(Some(ic::INVERT_SELECTION), tr.invert_selection, Command::InvertSelection).enabled(has_sel),
            item(Some(ic::DELETE), tr.delete, Command::DeleteSelection).enabled(has_sel),
            Entry::Separator,
            Entry::submenu(
                Some(ic::ROTATE_RIGHT),
                tr.rotate,
                vec![
                    Entry::item(Some(ic::ROTATE_RIGHT), tr.rotate_right, Command::Rotate(QuarterTurn::Cw90)),
                    Entry::item(Some(ic::ROTATE_LEFT), tr.rotate_left, Command::Rotate(QuarterTurn::Ccw90)),
                    Entry::item(Some(ic::ROTATE_CUSTOM), tr.rotate_180, Command::Rotate(QuarterTurn::Half)),
                ],
            ),
            Entry::submenu(
                Some(ic::FLIP_V),
                tr.flip,
                vec![
                    Entry::item(Some(ic::FLIP_V), tr.flip_vertical, Command::FlipVertical),
                    Entry::item(Some(ic::FLIP_H), tr.flip_horizontal, Command::FlipHorizontal),
                ],
            ),
            item(Some(ic::RESIZE), tr.resize, Command::ResizeSkew),
            item(Some(ic::INVERT_COLOR), tr.invert_color, Command::InvertColors),
            Entry::Separator,
            item(Some(ic::CROP), tr.crop, Command::Crop).enabled(has_sel),
        ]
    }

    pub fn tool_tooltip(&self, ctx: &egui::Context, label: &str, cmd: Option<&Command>) -> String {
        match cmd.and_then(|c| shortcut_label(ctx, c)) {
            Some(s) => format!("{label} ({s})"),
            None => label.to_owned(),
        }
    }
}

/// Makes the row behave as the macOS title bar: the empty parts drag the window and a double
/// click zooms it. Must run before the row's widgets so they stay on top of it. Returns where the
/// row's content starts, past the traffic lights (hidden in full screen).
#[cfg(target_os = "macos")]
fn title_bar(ui: &mut egui::Ui, r: Rect) -> f32 {
    let resp = ui.interact(r, Id::unique("title-bar"), Sense::click_and_drag());
    let (fullscreen, maximized) = ui.ctx().input(|i| (i.viewport().fullscreen.unwrap_or(false), i.viewport().maximized.unwrap_or(false)));
    if resp.double_clicked() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    } else if resp.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    r.left() + if fullscreen { 8.0 } else { crate::mac_titlebar::title_bar().content_left }
}

fn menu_height(entries: &[Entry<Command>]) -> f32 {
    entries.iter().map(|e| if matches!(e, Entry::Separator) { 9.0 } else { 34.0 }).sum::<f32>() + 10.0
}
