//! Fluent (Windows 11) color tokens, sampled from Paint in light and dark mode.

use egui::{Color32, FontFamily, FontId};

#[derive(Clone, Debug)]
pub struct Theme {
    pub dark: bool,
    pub window_bg: Color32,
    pub ribbon_bg: Color32,
    pub ribbon_border: Color32,
    pub separator: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    pub text_disabled: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub on_accent: Color32,
    pub selected_border: Color32,
    pub menu_bg: Color32,
    pub menu_border: Color32,
    pub menu_hover: Color32,
    pub menu_separator: Color32,
    pub panel_bg: Color32,
    pub panel_border: Color32,
    pub dialog_bg: Color32,
    pub dialog_footer: Color32,
    pub input_bg: Color32,
    pub input_border: Color32,
    pub input_underline: Color32,
    pub button_bg: Color32,
    pub button_hover: Color32,
    pub button_border: Color32,
    pub canvas_area: Color32,
    pub canvas_shadow: Color32,
    pub swatch_ring: Color32,
    pub slider_track: Color32,
    pub slider_thumb: Color32,
    pub checker_a: Color32,
    pub checker_b: Color32,
    pub tool_icon_fill: Color32,
}

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

impl Theme {
    pub fn light() -> Self {
        Self {
            dark: false,
            window_bg: hex(0xF3F3F3),
            ribbon_bg: hex(0xF9F9F9),
            ribbon_border: hex(0xEBEBEB),
            separator: hex(0xE5E5E5),
            text: hex(0x1A1A1A),
            text_secondary: hex(0x5D5D5D),
            text_disabled: hex(0xA0A0A0),
            hover: Color32::from_black_alpha(10),
            pressed: Color32::from_black_alpha(6),
            accent: hex(0x0067C0),
            accent_hover: hex(0x1975C5),
            on_accent: Color32::WHITE,
            selected_border: hex(0x0067C0),
            menu_bg: hex(0xFAFAFA),
            menu_border: hex(0xE0E0E0),
            menu_hover: Color32::from_black_alpha(10),
            menu_separator: hex(0xE5E5E5),
            panel_bg: hex(0xFFFFFF),
            panel_border: hex(0xE5E5E5),
            dialog_bg: hex(0xFFFFFF),
            dialog_footer: hex(0xF3F3F3),
            input_bg: hex(0xFFFFFF),
            input_border: hex(0xE5E5E5),
            input_underline: hex(0x868686),
            button_bg: hex(0xFBFBFB),
            button_hover: hex(0xF6F6F6),
            button_border: hex(0xE0E0E0),
            canvas_area: hex(0xF3F3F3),
            canvas_shadow: Color32::from_black_alpha(28),
            swatch_ring: hex(0xC4C4C4),
            slider_track: hex(0x8A8A8A),
            slider_thumb: hex(0xFFFFFF),
            checker_a: hex(0xFFFFFF),
            checker_b: hex(0xE6E6E6),
            tool_icon_fill: hex(0x0067C0),
        }
    }

    pub fn dark() -> Self {
        Self {
            dark: true,
            window_bg: hex(0x202020),
            ribbon_bg: hex(0x272727),
            ribbon_border: hex(0x1C1C1C),
            separator: hex(0x3A3A3A),
            text: hex(0xFFFFFF),
            text_secondary: hex(0xCECECE),
            text_disabled: hex(0x787878),
            hover: Color32::from_white_alpha(15),
            pressed: Color32::from_white_alpha(9),
            accent: hex(0x4CC2FF),
            accent_hover: hex(0x48B1E8),
            on_accent: Color32::BLACK,
            selected_border: hex(0x439CC9),
            menu_bg: hex(0x2C2C2C),
            menu_border: hex(0x1F1F1F),
            menu_hover: hex(0x383838),
            menu_separator: hex(0x3D3D3D),
            panel_bg: hex(0x2C2C2C),
            panel_border: hex(0x1F1F1F),
            dialog_bg: hex(0x2B2B2B),
            dialog_footer: hex(0x202020),
            input_bg: hex(0x373737),
            input_border: hex(0x3C3C3C),
            input_underline: hex(0x9F9F9F),
            button_bg: hex(0x373737),
            button_hover: hex(0x3C3C3C),
            button_border: hex(0x434343),
            canvas_area: hex(0x202020),
            canvas_shadow: Color32::from_black_alpha(90),
            swatch_ring: hex(0x5E5E5E),
            slider_track: hex(0x9F9F9F),
            slider_thumb: hex(0x454545),
            checker_a: hex(0xFFFFFF),
            checker_b: hex(0xE6E6E6),
            tool_icon_fill: hex(0x4CC2FF),
        }
    }

    /// Applies the tokens to egui's style so built-in widgets (text edits, tooltips) match.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut style = (*ctx.global_style()).clone();
        let mut v = if self.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.panel_fill = self.window_bg;
        v.window_fill = self.menu_bg;
        v.window_stroke = egui::Stroke::new(1.0, self.menu_border);
        v.window_corner_radius = egui::CornerRadius::same(8);
        v.menu_corner_radius = egui::CornerRadius::same(8);
        v.popup_shadow =
            egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if self.dark { 90 } else { 36 }) };
        v.window_shadow =
            egui::Shadow { offset: [0, 8], blur: 32, spread: 0, color: Color32::from_black_alpha(if self.dark { 110 } else { 50 }) };
        v.extreme_bg_color = self.input_bg;
        v.text_cursor.stroke = egui::Stroke::new(1.5, self.text);
        v.selection.bg_fill = self.accent.gamma_multiply(0.45);
        v.selection.stroke = egui::Stroke::new(1.0, self.text);
        v.override_text_color = Some(self.text);
        v.hyperlink_color = self.accent;
        for w in
            [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.noninteractive, &mut v.widgets.open]
        {
            w.corner_radius = egui::CornerRadius::same(4);
            w.bg_stroke = egui::Stroke::new(1.0, self.input_border);
            w.fg_stroke.color = self.text;
        }
        v.widgets.inactive.bg_fill = self.input_bg;
        v.widgets.inactive.weak_bg_fill = self.input_bg;
        v.widgets.hovered.bg_fill = self.button_hover;
        v.widgets.hovered.weak_bg_fill = self.button_hover;
        v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, self.input_border);
        v.widgets.active.bg_stroke = egui::Stroke::new(1.0, self.accent);
        style.visuals = v;
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size.y = 32.0;
        style.spacing.tooltip_width = 320.0;
        style.interaction.tooltip_delay = 0.45;
        style.interaction.interact_radius = 0.0;
        style.interaction.show_tooltips_only_when_still = false;
        style.text_styles = [
            (egui::TextStyle::Small, FontId::proportional(12.0)),
            (egui::TextStyle::Body, FontId::proportional(14.0)),
            (egui::TextStyle::Button, FontId::proportional(14.0)),
            (egui::TextStyle::Monospace, FontId::monospace(13.0)),
            (egui::TextStyle::Heading, FontId::new(20.0, semibold())),
        ]
        .into();
        ctx.set_global_style(style);
    }
}

pub const ICONS: &str = "icons";
pub const ICONS_FILLED: &str = "icons-filled";
pub const SEMIBOLD: &str = "semibold";

pub fn icons() -> FontFamily {
    FontFamily::Name(ICONS.into())
}

pub fn icons_filled() -> FontFamily {
    FontFamily::Name(ICONS_FILLED.into())
}

pub fn semibold() -> FontFamily {
    FontFamily::Name(SEMIBOLD.into())
}

/// Installs the bundled fonts: Selawik (an open metric-compatible substitute of Segoe UI) and the
/// Fluent UI System Icons subsets. Fonts are embedded so start-up never scans the system.
pub fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions};
    use std::sync::Arc;
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add(&mut fonts, "selawik", include_bytes!("../assets/selawik-regular.ttf"));
    add(&mut fonts, "selawik-semibold", include_bytes!("../assets/selawik-semibold.ttf"));
    add(&mut fonts, ICONS, include_bytes!("../assets/icons-regular.ttf"));
    add(&mut fonts, ICONS_FILLED, include_bytes!("../assets/icons-filled.ttf"));
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, "selawik".to_owned());
    let fallbacks: Vec<String> = proportional.iter().skip(1).cloned().collect();
    let mut semibold_list = vec!["selawik-semibold".to_owned()];
    semibold_list.extend(fallbacks);
    fonts.families.insert(semibold(), semibold_list);
    fonts.families.insert(icons(), vec![ICONS.to_owned()]);
    fonts.families.insert(icons_filled(), vec![ICONS_FILLED.to_owned()]);
    ctx.set_fonts(fonts);
}
