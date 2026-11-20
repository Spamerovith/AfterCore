//! Palettes, egui style and fonts.

use std::sync::Arc;

use ac_core::config::{Rgba, UiSettings, UiThemeKind};
use eframe::egui::{
    self, style::HandleShape, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke,
    TextStyle, Visuals,
};

#[derive(Clone, Copy, Debug)]
pub struct Pal {
    pub dark: bool,
    pub bg: Color32,
    pub sidebar: Color32,
    pub surface: Color32,
    pub surface2: Color32,
    pub surface3: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent2: Color32,
    pub on_accent: Color32,
    pub accent_soft: Color32,
    pub good: Color32,
    pub warn: Color32,
    pub bad: Color32,
    pub glow: bool,
}

pub fn c(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub fn rgba(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

pub fn to_rgba(c: Color32) -> Rgba {
    c.to_srgba_unmultiplied()
}

pub fn alpha(c: Color32, a: u8) -> Color32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

impl Pal {
    pub fn new(s: &UiSettings) -> Self {
        let accent = Color32::from_rgb(s.accent[0], s.accent[1], s.accent[2]);
        let (dark, bg, sidebar, surface, surface2, surface3, border, text, muted, faint) = match s.theme {
            UiThemeKind::Midnight => (
                true,
                c(0x0A0B11),
                c(0x0D0F16),
                c(0x12141D),
                c(0x181B26),
                c(0x20242F),
                c(0x232736),
                c(0xECEEF6),
                c(0x8C91A5),
                c(0x585D70),
            ),
            UiThemeKind::Graphite => (
                true,
                c(0x111113),
                c(0x141416),
                c(0x1A1A1D),
                c(0x222226),
                c(0x2A2A2F),
                c(0x2D2D33),
                c(0xEDEDF0),
                c(0x9A9AA3),
                c(0x63636B),
            ),
            UiThemeKind::Amoled => (
                true,
                c(0x000000),
                c(0x030304),
                c(0x0A0A0C),
                c(0x121215),
                c(0x1A1A1E),
                c(0x1D1D22),
                c(0xF4F4F6),
                c(0x8E8E98),
                c(0x55555D),
            ),
            UiThemeKind::Light => (
                false,
                c(0xF3F4F8),
                c(0xEBEDF3),
                c(0xFFFFFF),
                c(0xF4F5F8),
                c(0xEBEDF2),
                c(0xE1E4EB),
                c(0x13151C),
                c(0x5F6676),
                c(0x9AA0AE),
            ),
        };
        let lum = 0.299 * accent.r() as f32 + 0.587 * accent.g() as f32 + 0.114 * accent.b() as f32;
        let accent2 = mix(accent, c(0x22D3EE), 0.45);
        Self {
            dark,
            bg,
            sidebar,
            surface,
            surface2,
            surface3,
            border,
            text,
            muted,
            faint,
            accent,
            accent2,
            on_accent: if lum > 165.0 { c(0x0B0B10) } else { Color32::WHITE },
            accent_soft: alpha(accent, if dark { 38 } else { 30 }),
            good: c(0x34D399),
            warn: c(0xFBBF24),
            bad: c(0xF87171),
            glow: s.glow,
        }
    }
}

pub const BOLD: &str = "bold";
pub const ICONS: &str = "icons";

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD.into()))
}

pub fn icon(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICONS.into()))
}

pub fn body(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn setup_fonts(ctx: &egui::Context, ui_font_path: &str) {
    let mut fonts = FontDefinitions::default();
    let dir = ac_overlay::fonts::windows_fonts_dir();

    let mut load = |key: &str, path: &std::path::Path| -> bool {
        match std::fs::read(path) {
            Ok(bytes) => {
                fonts.font_data.insert(key.to_string(), Arc::new(FontData::from_owned(bytes)));
                true
            }
            Err(_) => false,
        }
    };

    let custom = !ui_font_path.is_empty() && load("ui", std::path::Path::new(ui_font_path));
    let regular_ok = custom || load("ui", &dir.join("segoeui.ttf"));
    let bold_ok = if custom {
        load("ui-bold", std::path::Path::new(ui_font_path))
    } else {
        load("ui-bold", &dir.join("seguisb.ttf")) || load("ui-bold", &dir.join("segoeuib.ttf"))
    };
    let icons_ok = ac_overlay::fonts::icon_font_path().map(|p| load("icons", &p)).unwrap_or(false);
    let mono_ok = ac_overlay::fonts::mono_font_path().map(|p| load("mono", &p)).unwrap_or(false);
    // Symbols / emoji fallback from the system.
    let sym_ok = load("symbols", &dir.join("seguisym.ttf"));

    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    let mut front = Vec::new();
    if regular_ok {
        front.push("ui".to_string());
    }
    if icons_ok {
        front.push("icons".to_string());
    }
    if sym_ok {
        front.push("symbols".to_string());
    }
    for (i, f) in front.iter().enumerate() {
        prop.insert(i, f.clone());
    }
    let prop_list = prop.clone();

    let mut bold_list = Vec::new();
    if bold_ok {
        bold_list.push("ui-bold".to_string());
    }
    bold_list.extend(prop_list.iter().cloned());
    fonts.families.insert(FontFamily::Name(BOLD.into()), bold_list);

    let mut icon_list = Vec::new();
    if icons_ok {
        icon_list.push("icons".to_string());
    }
    icon_list.extend(prop_list.iter().cloned());
    fonts.families.insert(FontFamily::Name(ICONS.into()), icon_list);

    if mono_ok {
        fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "mono".into());
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context, pal: &Pal, s: &UiSettings) {
    ctx.set_zoom_factor(s.scale);
    let mut style = (*ctx.style()).clone();
    let mut v = if pal.dark { Visuals::dark() } else { Visuals::light() };

    v.panel_fill = pal.bg;
    v.window_fill = pal.surface;
    v.window_stroke = Stroke::new(1.0_f32, pal.border);
    v.window_corner_radius = CornerRadius::same(14);
    v.menu_corner_radius = CornerRadius::same(12);
    v.extreme_bg_color = pal.surface2;
    v.faint_bg_color = pal.surface2;
    v.code_bg_color = pal.surface2;
    v.hyperlink_color = pal.accent;
    v.override_text_color = Some(pal.text);
    v.selection.bg_fill = alpha(pal.accent, 120);
    v.selection.stroke = Stroke::new(1.0_f32, pal.accent);
    v.slider_trailing_fill = true;
    v.handle_shape = HandleShape::Circle;
    v.striped = false;
    v.indent_has_left_vline = false;
    v.window_shadow = egui::epaint::Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(if pal.dark { 120 } else { 40 }) };
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(if pal.dark { 110 } else { 36 }) };
    v.text_cursor.stroke = Stroke::new(2.0_f32, pal.accent);

    let r = CornerRadius::same(10);
    v.widgets.noninteractive.bg_fill = pal.surface;
    v.widgets.noninteractive.weak_bg_fill = pal.surface;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, pal.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, pal.text);
    v.widgets.noninteractive.corner_radius = r;

    v.widgets.inactive.bg_fill = pal.surface3;
    v.widgets.inactive.weak_bg_fill = pal.surface2;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, pal.border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, pal.text);
    v.widgets.inactive.corner_radius = r;
    v.widgets.inactive.expansion = 0.0;

    v.widgets.hovered.bg_fill = mix(pal.surface3, pal.accent, 0.18);
    v.widgets.hovered.weak_bg_fill = mix(pal.surface2, pal.accent, 0.10);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, alpha(pal.accent, 150));
    v.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, pal.text);
    v.widgets.hovered.corner_radius = r;
    v.widgets.hovered.expansion = 0.0;

    v.widgets.active.bg_fill = pal.accent;
    v.widgets.active.weak_bg_fill = mix(pal.surface2, pal.accent, 0.25);
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, pal.accent);
    v.widgets.active.fg_stroke = Stroke::new(2.0_f32, pal.text);
    v.widgets.active.corner_radius = r;
    v.widgets.active.expansion = 0.0;

    v.widgets.open = v.widgets.hovered;

    style.visuals = v;
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(14.0, 7.0);
    style.spacing.interact_size = egui::vec2(40.0, 32.0);
    style.spacing.slider_width = 220.0;
    style.spacing.slider_rail_height = 6.0;
    style.spacing.combo_width = 220.0;
    style.spacing.text_edit_width = 240.0;
    style.spacing.menu_margin = Margin::same(8);
    style.spacing.window_margin = Margin::same(14);
    style.spacing.scroll = egui::style::ScrollStyle::floating();
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.icon_width = 18.0;
    style.spacing.icon_spacing = 8.0;
    style.interaction.selectable_labels = false;
    style.animation_time = if s.animations { 0.16 } else { 0.0 };

    style.text_styles = [
        (TextStyle::Heading, bold(26.0)),
        (TextStyle::Body, body(14.5)),
        (TextStyle::Button, body(14.0)),
        (TextStyle::Small, body(12.0)),
        (TextStyle::Monospace, FontId::monospace(13.0)),
    ]
    .into();
    ctx.set_style(style);
}
