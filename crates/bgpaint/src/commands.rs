//! Every user action, so menus, the ribbon and keyboard shortcuts share one code path.

use crate::tools::{SelectMode, Tool};
use bgpaint_core::brush::BrushKind;
use bgpaint_core::io::SaveFormat;
use bgpaint_core::raster::QuarterTurn;
use bgpaint_core::shapes::{ShapeKind, ShapeStyle};
use egui::{Key, KeyboardShortcut, Modifiers};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallpaperMode {
    Fill,
    Tile,
    Center,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    New,
    Open,
    OpenRecent(PathBuf),
    ImportFromFile,
    Save,
    SaveAs(Option<SaveFormat>),
    SaveProject,
    Print,
    PageSetup,
    PrintPreview,
    Share,
    SetWallpaper(WallpaperMode),
    ImageProperties,
    Exit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    CopyVisible,
    SelectAll,
    InvertSelection,
    DeleteSelection,
    ToggleTransparentSelection,
    SetSelectMode(SelectMode),
    Crop,
    ResizeSkew,
    Rotate(QuarterTurn),
    RotateCustom,
    FlipHorizontal,
    FlipVertical,
    InvertColors,
    SetTool(Tool),
    SetBrush(BrushKind),
    SetShape(ShapeKind),
    SetOutline(ShapeStyle),
    SetFill(ShapeStyle),
    SetSize(f32),
    GrowSize(bool),
    ZoomIn,
    ZoomOut,
    Zoom100,
    ZoomFit,
    SetZoom(f32),
    ToggleRulers,
    ToggleGrid,
    ToggleStatusBar,
    SetAutoHideToolbar(bool),
    ToggleFullscreen,
    ToggleThumbnail,
    ToggleLayersPanel,
    OpenSettings,
    CloseSettings,
    EditColors,
    AddLayer,
    DuplicateLayer(usize),
    MergeDown(usize),
    MoveLayer(usize, bool),
    DeleteLayer(usize),
    ToggleLayerVisible(usize),
    SelectLayer(usize),
    ToggleBackground,
    ChangeBackgroundColor,
    Escape,
}

/// Keyboard shortcuts. `COMMAND` is Ctrl on Linux and ⌘ on macOS.
pub fn shortcuts() -> Vec<(KeyboardShortcut, Command)> {
    let c = Modifiers::COMMAND;
    let cs = Modifiers::COMMAND | Modifiers::SHIFT;
    let k = KeyboardShortcut::new;
    vec![
        (k(c, Key::N), Command::New),
        (k(c, Key::O), Command::Open),
        (k(cs, Key::S), Command::SaveProject),
        (k(c, Key::S), Command::Save),
        (k(Modifiers::NONE, Key::F12), Command::SaveAs(None)),
        (k(c, Key::P), Command::Print),
        (k(c, Key::E), Command::ImageProperties),
        (k(cs, Key::Z), Command::Redo),
        (k(c, Key::Z), Command::Undo),
        (k(c, Key::Y), Command::Redo),
        (k(cs, Key::C), Command::CopyVisible),
        (k(c, Key::A), Command::SelectAll),
        (k(Modifiers::NONE, Key::Delete), Command::DeleteSelection),
        (k(c, Key::W), Command::ResizeSkew),
        (k(cs, Key::X), Command::Crop),
        (k(cs, Key::I), Command::InvertColors),
        (k(c, Key::R), Command::ToggleRulers),
        (k(c, Key::G), Command::ToggleGrid),
        (k(Modifiers::NONE, Key::F11), Command::ToggleFullscreen),
        // More specific (Shift) shortcuts must come first: egui ignores extra Shift when matching.
        (k(cs, Key::PageUp), Command::MoveLayer(usize::MAX, true)),
        (k(cs, Key::PageDown), Command::MoveLayer(usize::MAX, false)),
        (k(c, Key::PageUp), Command::ZoomIn),
        (k(c, Key::PageDown), Command::ZoomOut),
        (k(c, Key::Num0), Command::Zoom100),
        (k(c, Key::Num1), Command::ZoomFit),
        (k(c, Key::Plus), Command::GrowSize(true)),
        (k(c, Key::Equals), Command::GrowSize(true)),
        (k(c, Key::Minus), Command::GrowSize(false)),
        (k(cs, Key::H), Command::ToggleBackground),
        (k(c, Key::H), Command::ToggleLayerVisible(usize::MAX)),
        (k(cs, Key::B), Command::ChangeBackgroundColor),
        (k(cs, Key::Delete), Command::DeleteLayer(usize::MAX)),
        (k(Modifiers::NONE, Key::Escape), Command::Escape),
    ]
}

/// The shortcut label shown in menus for a command, formatted for the platform.
pub fn shortcut_label(ctx: &egui::Context, cmd: &Command) -> Option<String> {
    let fixed = match cmd {
        Command::Cut => Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::X)),
        Command::Copy => Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::C)),
        Command::Paste => Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::V)),
        // Platform convention: ⇧⌘Z on macOS, Ctrl+Y elsewhere.
        Command::Redo if cfg!(target_os = "macos") => Some(KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)),
        Command::Redo => Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::Y)),
        _ => None,
    };
    let sc = fixed.or_else(|| shortcuts().into_iter().find(|(_, c)| c == cmd).map(|(s, _)| s))?;
    Some(ctx.format_shortcut(&sc))
}
