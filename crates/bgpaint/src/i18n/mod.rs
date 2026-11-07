//! User interface strings, one file per language: `en.rs`, `pt_br.rs` and `es.rs`.
//!
//! To add a language, copy `en.rs` to a new file, translate every string (the compiler refuses a
//! file with a missing or misspelled key), declare the module below and map its language code
//! in [`supported`] and [`LANGUAGES`]. Unless the user picks one in Settings, the language is
//! chosen from the system languages (European Portuguese uses the Brazilian strings; anything the
//! app doesn't speak falls back to English).

mod en;
mod es;
mod pt_br;

pub use en::STRINGS as EN;
pub use es::STRINGS as ES;
pub use pt_br::STRINGS as PT_BR;

/// Every translatable string of the interface.
pub struct Strings {
    pub app_name: &'static str,
    pub untitled: &'static str,
    // Menus
    pub menu_file: &'static str,
    pub menu_edit: &'static str,
    pub menu_view: &'static str,
    pub new: &'static str,
    pub open: &'static str,
    pub import_to_canvas: &'static str,
    pub from_file: &'static str,
    pub from_scanner: &'static str,
    pub recent: &'static str,
    pub no_recent: &'static str,
    pub save: &'static str,
    pub save_as: &'static str,
    pub save_as_project: &'static str,
    pub png_image: &'static str,
    pub jpeg_image: &'static str,
    pub bmp_image: &'static str,
    pub gif_image: &'static str,
    pub webp_image: &'static str,
    pub other_format: &'static str,
    pub print: &'static str,
    pub page_setup: &'static str,
    pub print_preview: &'static str,
    pub share: &'static str,
    pub set_as_wallpaper: &'static str,
    pub wallpaper_fill: &'static str,
    pub wallpaper_tile: &'static str,
    pub wallpaper_center: &'static str,
    pub image_properties: &'static str,
    pub exit: &'static str,
    pub undo: &'static str,
    pub redo: &'static str,
    pub cut: &'static str,
    pub copy: &'static str,
    pub paste: &'static str,
    pub copy_visible_layers: &'static str,
    pub zoom: &'static str,
    pub zoom_100: &'static str,
    pub zoom_in: &'static str,
    pub zoom_out: &'static str,
    pub fit_to_window: &'static str,
    pub rulers: &'static str,
    pub gridlines: &'static str,
    pub status_bar: &'static str,
    pub always_show_toolbar: &'static str,
    pub auto_hide_toolbar: &'static str,
    pub full_screen: &'static str,
    pub thumbnail: &'static str,
    pub settings: &'static str,
    // Ribbon groups
    pub group_selection: &'static str,
    pub group_image: &'static str,
    pub group_tools: &'static str,
    pub group_brushes: &'static str,
    pub group_shapes: &'static str,
    pub group_colors: &'static str,
    pub group_layers: &'static str,
    // Selection
    pub rectangle_selection: &'static str,
    pub freeform_selection: &'static str,
    pub select_all: &'static str,
    pub invert_selection: &'static str,
    pub transparent_selection: &'static str,
    pub delete: &'static str,
    // Image
    pub crop: &'static str,
    pub resize_and_skew: &'static str,
    pub rotate: &'static str,
    pub rotate_right: &'static str,
    pub rotate_left: &'static str,
    pub rotate_180: &'static str,
    pub rotate_custom: &'static str,
    pub flip: &'static str,
    pub flip_vertical: &'static str,
    pub flip_horizontal: &'static str,
    pub invert_color: &'static str,
    // Tools
    pub pencil: &'static str,
    pub fill: &'static str,
    pub text: &'static str,
    pub eraser: &'static str,
    pub color_picker: &'static str,
    pub magnifier: &'static str,
    // Brushes
    pub brush: &'static str,
    pub calligraphy_brush: &'static str,
    pub calligraphy_pen: &'static str,
    pub airbrush: &'static str,
    pub oil_brush: &'static str,
    pub crayon: &'static str,
    pub marker: &'static str,
    pub natural_pencil: &'static str,
    pub watercolor: &'static str,
    pub size: &'static str,
    // Shapes
    pub shape_line: &'static str,
    pub shape_curve: &'static str,
    pub shape_oval: &'static str,
    pub shape_rectangle: &'static str,
    pub shape_rounded_rectangle: &'static str,
    pub shape_polygon: &'static str,
    pub shape_triangle: &'static str,
    pub shape_right_triangle: &'static str,
    pub shape_diamond: &'static str,
    pub shape_pentagon: &'static str,
    pub shape_hexagon: &'static str,
    pub shape_right_arrow: &'static str,
    pub shape_left_arrow: &'static str,
    pub shape_up_arrow: &'static str,
    pub shape_down_arrow: &'static str,
    pub shape_star4: &'static str,
    pub shape_star5: &'static str,
    pub shape_star6: &'static str,
    pub shape_rounded_callout: &'static str,
    pub shape_oval_callout: &'static str,
    pub shape_cloud_callout: &'static str,
    pub shape_heart: &'static str,
    pub shape_lightning: &'static str,
    pub outline: &'static str,
    pub shape_fill: &'static str,
    pub no_outline: &'static str,
    pub solid_outline: &'static str,
    pub no_fill: &'static str,
    pub solid_fill: &'static str,
    // Colors
    pub color1: &'static str,
    pub color2: &'static str,
    pub edit_colors: &'static str,
    pub basic_colors: &'static str,
    pub custom_colors: &'static str,
    pub edit_custom_color: &'static str,
    pub define_custom_color: &'static str,
    pub red: &'static str,
    pub green: &'static str,
    pub blue: &'static str,
    pub hue: &'static str,
    pub saturation: &'static str,
    pub value: &'static str,
    // Layers
    pub layers: &'static str,
    pub add_layer: &'static str,
    pub hide_layer: &'static str,
    pub show_layer: &'static str,
    pub duplicate_layer: &'static str,
    pub merge_down: &'static str,
    pub move_up: &'static str,
    pub move_down: &'static str,
    pub delete_layer: &'static str,
    pub background: &'static str,
    pub hide_background: &'static str,
    pub show_background: &'static str,
    pub change_color: &'static str,
    pub layer_n: &'static str,
    // Text
    pub background_fill: &'static str,
    pub bold: &'static str,
    pub italic: &'static str,
    pub underline: &'static str,
    pub strikethrough: &'static str,
    pub align_left: &'static str,
    pub align_center: &'static str,
    pub align_right: &'static str,
    pub font_family: &'static str,
    pub font_size: &'static str,
    // Dialogs
    pub ok: &'static str,
    pub cancel: &'static str,
    pub resize: &'static str,
    pub percentage: &'static str,
    pub pixels: &'static str,
    pub horizontal: &'static str,
    pub vertical: &'static str,
    pub skew: &'static str,
    pub maintain_aspect_ratio: &'static str,
    pub image_properties_title: &'static str,
    pub file_attributes: &'static str,
    pub saved_at: &'static str,
    pub size_on_disk: &'static str,
    pub resolution: &'static str,
    pub not_available: &'static str,
    pub units: &'static str,
    pub inches: &'static str,
    pub centimeters: &'static str,
    pub image_size: &'static str,
    pub width: &'static str,
    pub height: &'static str,
    pub default: &'static str,
    pub angle: &'static str,
    pub unsaved_title: &'static str,
    pub unsaved_question: &'static str,
    pub dont_save: &'static str,
    // The macOS name for "Don't save" (the red button of AppKit's save alerts).
    pub discard_document: &'static str,
    pub error_title: &'static str,
    pub open_failed: &'static str,
    pub save_failed: &'static str,
    pub close: &'static str,
    pub all_images: &'static str,
    pub paint_project: &'static str,
    // Print
    pub printer: &'static str,
    pub copies: &'static str,
    pub orientation: &'static str,
    pub portrait: &'static str,
    pub landscape: &'static str,
    pub scaling: &'static str,
    pub fit_to_page: &'static str,
    pub actual_size: &'static str,
    pub margins: &'static str,
    pub center_on_page: &'static str,
    pub paper_size: &'static str,
    pub no_printers: &'static str,
    pub print_sent: &'static str,
    pub print_failed: &'static str,
    pub wallpaper_failed: &'static str,
    pub wallpaper_done: &'static str,
    pub share_failed: &'static str,
    // Settings page
    pub app_theme: &'static str,
    pub language: &'static str,
    pub theme_light: &'static str,
    pub theme_dark: &'static str,
    pub theme_system: &'static str,
    pub about: &'static str,
    pub about_text: &'static str,
    pub source_code: &'static str,
    pub license: &'static str,
    pub third_party: &'static str,
    // Updates ({version}: the new version; {manager}: a package manager)
    pub updates: &'static str,
    pub check_updates_automatically: &'static str,
    pub check_for_updates: &'static str,
    pub update_title: &'static str,
    pub update_checking: &'static str,
    pub update_up_to_date: &'static str,
    pub update_check_failed: &'static str,
    pub update_downloading: &'static str,
    pub update_ready: &'static str,
    pub update_ready_short: &'static str,
    pub update_admin_prompt: &'static str,
    pub update_password: &'static str,
    pub update_wrong_password: &'static str,
    pub update_installing: &'static str,
    pub update_installed: &'static str,
    pub update_available: &'static str,
    pub update_managed: &'static str,
    pub update_unsupported: &'static str,
    pub update_failed: &'static str,
    pub update_install: &'static str,
    pub update_restart: &'static str,
    pub update_later: &'static str,
    pub update_download_page: &'static str,
    pub update_whats_new: &'static str,
    // Status bar
    pub zoom_level: &'static str,
    pub canvas_size: &'static str,
    pub cursor_position: &'static str,
    pub selection_size: &'static str,
    pub expand_toolbar: &'static str,
    pub collapse_toolbar: &'static str,
}

/// Strings that only the macOS menu bar uses.
#[cfg(target_os = "macos")]
pub struct MacStrings {
    pub about: &'static str,
    pub check_updates: &'static str,
    pub settings: &'static str,
    pub services: &'static str,
    pub hide: &'static str,
    pub hide_others: &'static str,
    pub show_all: &'static str,
    pub quit: &'static str,
    pub image: &'static str,
    pub window: &'static str,
    pub help: &'static str,
    pub close_tab: &'static str,
    pub minimize: &'static str,
    pub zoom_window: &'static str,
    pub bring_all: &'static str,
    pub website: &'static str,
}

/// The macOS menu strings of the same language as `tr`.
#[cfg(target_os = "macos")]
pub fn mac_strings(tr: &'static Strings) -> &'static MacStrings {
    if std::ptr::eq(tr, &PT_BR) {
        &pt_br::MAC
    } else if std::ptr::eq(tr, &ES) {
        &es::MAC
    } else {
        &en::MAC
    }
}

/// The languages offered in Settings: the code saved in the settings file and the language's own
/// name, which is shown untranslated.
pub const LANGUAGES: [(&str, &str); 3] = [("en", "English"), ("pt-BR", "Português (Brasil)"), ("es", "Español")];

/// The strings for the language picked in Settings (a code from [`LANGUAGES`]), or the system's
/// when none is picked or the code is unknown.
pub fn resolve(language: Option<&str>) -> &'static Strings {
    language.and_then(supported).unwrap_or_else(detect)
}

/// Chooses the strings from the system's preferred languages: the first one the app speaks wins,
/// and English is used when none is (or when the system does not say).
pub fn detect() -> &'static Strings {
    sys_locale::get_locales().find_map(|code| supported(&code)).unwrap_or(&EN)
}

/// The strings for a locale code such as `pt-BR`, `es_419.UTF-8` or `en`, falling back to English.
#[cfg(test)]
pub fn from_code(code: &str) -> &'static Strings {
    supported(code).unwrap_or(&EN)
}

fn supported(code: &str) -> Option<&'static Strings> {
    let language = code.split(['-', '_', '.', '@']).next().unwrap_or_default().to_ascii_lowercase();
    match language.as_str() {
        "en" => Some(&EN),
        // Every Portuguese variant (pt-PT, pt-AO…) uses the Brazilian strings.
        "pt" => Some(&PT_BR),
        "es" => Some(&ES),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_selection() {
        assert_eq!(from_code("pt-BR").new, "Novo");
        assert_eq!(from_code("pt_PT.UTF-8").new, "Novo");
        assert_eq!(from_code("pt").new, "Novo");
        assert_eq!(from_code("es-ES").new, "Nuevo");
        assert_eq!(from_code("es_419").new, "Nuevo");
        assert_eq!(from_code("en-US").new, "New");
        assert_eq!(from_code("fr-FR").new, "New");
        assert_eq!(from_code("C").new, "New");
        assert_eq!(from_code("").new, "New");
    }

    #[test]
    fn every_offered_language_resolves_to_itself() {
        for (code, _) in LANGUAGES {
            assert!(supported(code).is_some(), "{code}");
        }
        assert_eq!(resolve(Some("pt-BR")).new, "Novo");
        assert_eq!(resolve(Some("es")).new, "Nuevo");
        assert_eq!(resolve(Some("en")).new, "New");
    }
}
