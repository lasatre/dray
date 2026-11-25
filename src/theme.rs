//! The Dray theme: palettes, spacing, radii and type, plus their mapping onto egui's
//! `Style`/`Visuals`. Values come from `brand/tokens.json` (the source of truth; the test at
//! the bottom checks every color against it). UI code takes colors from here only.

#![allow(dead_code)] // the full token set is kept, even values the app does not use yet

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke,
    TextStyle, Theme, Visuals,
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub surface: Color32,
    pub surface_raised: Color32,
    pub surface_sunken: Color32,
    pub ink: Color32,
    pub ink_muted: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub brand: Color32,
    pub brand_strong: Color32,
    pub on_brand: Color32,
    pub brand_subtle: Color32,
    pub patina: Color32,
    pub on_patina: Color32,
    pub focus_ring: Color32,
    pub success: Color32,
    pub info: Color32,
    pub warning: Color32,
    pub danger: Color32,
    /// `shadow-popover`, for floating layers only (menus, popovers, dialogs).
    pub shadow_popover: Shadow,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const LIGHT: Palette = Palette {
    surface: rgb(246, 243, 238),
    surface_raised: rgb(255, 253, 249),
    surface_sunken: rgb(236, 231, 223),
    ink: rgb(27, 25, 22),
    ink_muted: rgb(92, 86, 78),
    border: rgb(214, 207, 196),
    border_strong: rgb(138, 130, 119),
    brand: rgb(176, 62, 13),
    brand_strong: rgb(138, 47, 7),
    on_brand: rgb(255, 253, 249),
    brand_subtle: rgb(251, 235, 226),
    patina: rgb(15, 107, 102),
    on_patina: rgb(255, 253, 249),
    focus_ring: rgb(15, 107, 102),
    success: rgb(15, 107, 102),
    info: rgb(31, 95, 168),
    warning: rgb(138, 90, 0),
    danger: rgb(156, 28, 22),
    // 0 8px 24px rgba(27,25,22,0.14). egui draws one shadow per shape, so the second
    // 1 px contact layer of the token is approximated by the 1 px `border` window stroke.
    shadow_popover: Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_rgba_unmultiplied_const(27, 25, 22, 36),
    },
};

pub const DARK: Palette = Palette {
    surface: rgb(21, 19, 17),
    surface_raised: rgb(31, 28, 25),
    surface_sunken: rgb(14, 13, 11),
    ink: rgb(241, 236, 228),
    ink_muted: rgb(168, 160, 148),
    border: rgb(53, 48, 42),
    border_strong: rgb(122, 114, 102),
    brand: rgb(238, 122, 70),
    brand_strong: rgb(255, 148, 102),
    on_brand: rgb(27, 18, 16),
    brand_subtle: rgb(58, 34, 23),
    patina: rgb(79, 195, 185),
    on_patina: rgb(14, 26, 25),
    focus_ring: rgb(79, 195, 185),
    success: rgb(79, 195, 185),
    info: rgb(106, 168, 240),
    warning: rgb(224, 165, 38),
    danger: rgb(255, 123, 114),
    // 0 8px 24px rgba(0,0,0,0.5)
    shadow_popover: Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_rgba_unmultiplied_const(0, 0, 0, 128) },
};

// Spacing (px)
pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_3: f32 = 12.0;
pub const SPACE_4: f32 = 16.0;
pub const SPACE_6: f32 = 24.0;
pub const SPACE_8: f32 = 32.0;
pub const SPACE_12: f32 = 48.0;
pub const ROW_COMPACT: f32 = 28.0;
pub const ROW_DEFAULT: f32 = 36.0;

// Radii (px)
pub const RADIUS_SM: f32 = 4.0;
pub const RADIUS_MD: f32 = 6.0;
pub const RADIUS_LG: f32 = 10.0;
pub const RADIUS_FULL: f32 = 999.0;

// Type (px): size, line height. Interface text uses the OS UI font; mono is JetBrains Mono.
pub const FONT_DISPLAY_XL: (f32, f32) = (56.0, 56.0);
pub const FONT_DISPLAY: (f32, f32) = (36.0, 40.0);
pub const FONT_TITLE: (f32, f32) = (22.0, 28.0);
pub const FONT_BODY: (f32, f32) = (16.0, 24.0);
pub const FONT_BODY_STRONG: (f32, f32) = (16.0, 24.0);
pub const FONT_SMALL: (f32, f32) = (13.0, 18.0);
pub const FONT_UI_TITLE: (f32, f32) = (15.0, 20.0);
pub const FONT_UI_ROW: (f32, f32) = (13.0, 18.0);
pub const FONT_UI_LABEL: (f32, f32) = (13.0, 18.0);
pub const FONT_UI_SMALL: (f32, f32) = (12.0, 16.0);
pub const FONT_MONO: (f32, f32) = (13.0, 18.0);
pub const FONT_MONO_SMALL: (f32, f32) = (12.0, 16.0);

// Component metrics derived from the brand rules.
/// Buttons and single-line inputs.
pub const CONTROL_H: f32 = 28.0;
/// Status pills.
pub const PILL_H: f32 = 24.0;
/// Interface icons (toolbar icons use `ICON_TOOLBAR`).
pub const ICON: f32 = 16.0;
pub const ICON_TOOLBAR: f32 = 20.0;
/// Keyboard focus: 2 px solid ring, 2 px outside the control.
pub const FOCUS_RING_WIDTH: f32 = 2.0;
pub const FOCUS_RING_OFFSET: f32 = 2.0;

/// Family for weight-600 interface text (`ui-label`, `ui-title`).
pub const SEMIBOLD: &str = "ui-semibold";
/// Text style for `mono-small` (dates, sizes, status bar numbers).
pub const MONO_SMALL: &str = "mono-small";

pub fn semibold() -> FontFamily {
    FontFamily::Name(SEMIBOLD.into())
}

pub fn ui_font(size: (f32, f32)) -> FontId {
    FontId::new(size.0, FontFamily::Proportional)
}

pub fn semibold_font(size: (f32, f32)) -> FontId {
    FontId::new(size.0, semibold())
}

pub fn mono_font(size: (f32, f32)) -> FontId {
    FontId::new(size.0, FontFamily::Monospace)
}

/// Square left end, fully rounded right end: the D of the mark.
pub fn d_shape(left: f32) -> CornerRadius {
    let l = left as u8;
    CornerRadius { nw: l, sw: l, ne: u8::MAX, se: u8::MAX }
}

pub fn radius(r: f32) -> CornerRadius {
    CornerRadius::same(r.min(u8::MAX as f32) as u8)
}

/// The palette for the theme currently in effect.
pub fn of(ui: &egui::Ui) -> &'static Palette {
    if ui.visuals().dark_mode { &DARK } else { &LIGHT }
}

pub fn of_ctx(ctx: &egui::Context) -> &'static Palette {
    if ctx.global_style().visuals.dark_mode { &DARK } else { &LIGHT }
}

/// Install fonts and both styles. egui then follows the OS light/dark setting unless the user
/// picked one in Settings (`ctx.set_theme`).
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    for (theme, p) in [(Theme::Light, &LIGHT), (Theme::Dark, &DARK)] {
        let mut style = (*ctx.style_of(theme)).clone();
        apply(&mut style, p, theme == Theme::Dark);
        ctx.set_style_of(theme, style);
    }
}

fn apply(style: &mut egui::Style, p: &Palette, dark: bool) {
    use TextStyle::*;
    style.text_styles = [
        (Small, ui_font(FONT_UI_SMALL)),
        (Body, ui_font(FONT_UI_ROW)),
        (Button, semibold_font(FONT_UI_LABEL)),
        (Heading, semibold_font(FONT_UI_TITLE)),
        (Monospace, mono_font(FONT_MONO)),
        (Name(MONO_SMALL.into()), mono_font(FONT_MONO_SMALL)),
    ]
    .into();
    style.drag_value_text_style = Monospace; // numbers with units ("15 s") are mono
    style.animation_time = 0.1; // hover and press: 100 ms
    style.interaction.selectable_labels = false;

    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(SPACE_2, SPACE_1);
    s.button_padding = egui::vec2(SPACE_3, SPACE_1);
    s.interact_size = egui::vec2(CONTROL_H, CONTROL_H);
    s.window_margin = Margin::same(SPACE_8 as i8);
    s.menu_margin = Margin::same(SPACE_1 as i8);
    s.indent = SPACE_4;
    s.icon_width = ICON;
    s.icon_width_inner = SPACE_2;
    s.icon_spacing = SPACE_2;
    s.menu_spacing = SPACE_1;

    style.visuals = visuals(p, dark);
}

pub fn visuals(p: &Palette, dark: bool) -> Visuals {
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    let md = radius(RADIUS_MD);
    let hair = Stroke::new(1.0, p.border);
    let edge = Stroke::new(1.0, p.border_strong);
    let text = Stroke::new(1.0, p.ink);

    v.override_text_color = None;
    v.weak_text_color = Some(p.ink_muted);
    v.hyperlink_color = p.brand;
    v.faint_bg_color = p.surface;
    v.extreme_bg_color = p.surface_raised;
    v.text_edit_bg_color = Some(p.surface_raised);
    v.code_bg_color = p.surface_sunken;
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.danger;
    v.panel_fill = p.surface;
    v.window_fill = p.surface_raised;
    v.window_stroke = hair;
    v.window_corner_radius = radius(RADIUS_LG);
    v.window_shadow = p.shadow_popover;
    v.window_highlight_topmost = false;
    v.menu_corner_radius = radius(RADIUS_LG);
    v.popup_shadow = p.shadow_popover;
    v.striped = false;
    v.indent_has_left_vline = false;
    v.selection.bg_fill = p.brand_subtle;
    v.selection.stroke = text;
    v.text_cursor.stroke = Stroke::new(2.0, p.ink);

    let w = &mut v.widgets;
    // Labels, separators, frames.
    w.noninteractive.bg_fill = p.surface_raised;
    w.noninteractive.weak_bg_fill = p.surface_raised;
    w.noninteractive.bg_stroke = hair;
    w.noninteractive.fg_stroke = text;
    // Secondary controls at rest: surface-raised with a border-strong edge.
    w.inactive.bg_fill = p.surface_raised;
    w.inactive.weak_bg_fill = p.surface_raised;
    w.inactive.bg_stroke = edge;
    w.inactive.fg_stroke = text;
    // Hover and open: surface-sunken.
    w.hovered.bg_fill = p.surface_sunken;
    w.hovered.weak_bg_fill = p.surface_sunken;
    w.hovered.bg_stroke = edge;
    w.hovered.fg_stroke = text;
    w.open = w.hovered;
    // Pressed (and focused; the focus ring itself is drawn separately).
    w.active.bg_fill = p.surface_sunken;
    w.active.weak_bg_fill = p.surface_sunken;
    w.active.bg_stroke = edge;
    w.active.fg_stroke = text;
    for wv in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        wv.corner_radius = md;
        wv.expansion = 0.0;
    }
    v
}

// ---------------------------------------------------------------------------------- fonts

const JETBRAINS_MONO: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");

/// Interface text uses the OS UI font, loaded from the system at runtime (nothing is bundled);
/// egui's built-in font stays as the fallback for missing glyphs or an unknown system.
/// Monospace is the bundled JetBrains Mono (OFL 1.1, see assets/fonts/OFL.txt).
fn fonts() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let fallback_prop = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let fallback_mono = defs.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();

    defs.font_data.insert("jetbrains-mono".into(), Arc::new(FontData::from_static(JETBRAINS_MONO)));
    defs.families.insert(FontFamily::Monospace, [vec!["jetbrains-mono".to_string()], fallback_mono].concat());

    let (regular, semibold_face) = system_ui_fonts();
    let mut prop = Vec::new();
    let mut semi = Vec::new();
    if let Some(data) = regular {
        defs.font_data.insert("ui".into(), Arc::new(data));
        prop.push("ui".to_string());
    }
    if let Some(data) = semibold_face {
        defs.font_data.insert("ui-semibold".into(), Arc::new(data));
        semi.push("ui-semibold".to_string());
    }
    semi.extend(prop.iter().cloned());
    prop.extend(fallback_prop.iter().cloned());
    semi.extend(fallback_prop);
    defs.families.insert(FontFamily::Proportional, prop);
    defs.families.insert(semibold(), semi);
    defs
}

fn load(path: &std::path::Path, weight: Option<f32>) -> Option<FontData> {
    let bytes = std::fs::read(path).ok()?;
    let mut data = FontData::from_owned(bytes);
    if let Some(w) = weight {
        // Variable fonts (SF on macOS, many Linux defaults): pick the weight by axis.
        data.tweak.coords = egui::epaint::text::VariationCoords::new([("wght", w)]);
    }
    Some(data)
}

/// (regular, semibold) system UI faces.
fn system_ui_fonts() -> (Option<FontData>, Option<FontData>) {
    #[cfg(windows)]
    {
        let dir = std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()))
            .join("Fonts");
        (load(&dir.join("segoeui.ttf"), None), load(&dir.join("seguisb.ttf"), None))
    }
    #[cfg(target_os = "macos")]
    {
        let sf = std::path::Path::new("/System/Library/Fonts/SFNS.ttf");
        (load(sf, Some(400.0)), load(sf, Some(600.0)))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Ask fontconfig for the desktop's sans-serif face.
        let fc = |pattern: &str| -> Option<std::path::PathBuf> {
            let out = std::process::Command::new("fc-match").args(["-f", "%{file}", pattern]).output().ok()?;
            let path = String::from_utf8(out.stdout).ok()?;
            (!path.is_empty()).then(|| path.into())
        };
        let regular = fc("sans-serif:weight=regular");
        let semi = fc("sans-serif:weight=semibold");
        let semi_data = match (&regular, &semi) {
            // Same file for both weights: a variable font (or no semibold face at all).
            (Some(r), Some(s)) if r == s => load(s, Some(600.0)),
            (_, Some(s)) => load(s, None),
            _ => None,
        };
        (regular.and_then(|r| load(&r, None)), semi_data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(c: Color32) -> String {
        format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
    }

    #[test]
    fn palettes_match_tokens_json() {
        let json: serde_json::Value =
            serde_json::from_str(include_str!("../brand/tokens.json")).expect("tokens.json parses");
        let tokens = json["color"]["tokens"].as_array().unwrap();
        let lookup = |name: &str, theme: &str| -> String {
            let mut value = tokens.iter().find(|t| t["name"] == name).unwrap()["value"][theme].as_str().unwrap().to_string();
            // `{patina}` style references.
            while let Some(r) = value.strip_prefix('{').and_then(|v| v.strip_suffix('}')) {
                value = tokens.iter().find(|t| t["name"] == r).unwrap()["value"][theme].as_str().unwrap().to_string();
            }
            value
        };
        for (theme, p) in [("light", &LIGHT), ("dark", &DARK)] {
            let fields = [
                ("surface", p.surface),
                ("surface-raised", p.surface_raised),
                ("surface-sunken", p.surface_sunken),
                ("ink", p.ink),
                ("ink-muted", p.ink_muted),
                ("border", p.border),
                ("border-strong", p.border_strong),
                ("brand", p.brand),
                ("brand-strong", p.brand_strong),
                ("on-brand", p.on_brand),
                ("brand-subtle", p.brand_subtle),
                ("patina", p.patina),
                ("on-patina", p.on_patina),
                ("focus-ring", p.focus_ring),
                ("success", p.success),
                ("info", p.info),
                ("warning", p.warning),
                ("danger", p.danger),
            ];
            assert_eq!(fields.len(), tokens.len(), "every color token is mapped");
            for (name, c) in fields {
                assert_eq!(hex(c), lookup(name, theme), "{theme} {name}");
            }
        }
    }

    fn luminance(c: Color32) -> f64 {
        let lin = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// Every text/ground pairing the UI uses holds 4.5:1; control edges and the focus ring 3:1.
    #[test]
    fn contrast_holds() {
        for (theme, p) in [("light", &LIGHT), ("dark", &DARK)] {
            let text_on = [
                ("ink", p.ink, vec![p.surface, p.surface_raised, p.surface_sunken, p.brand_subtle]),
                ("ink-muted", p.ink_muted, vec![p.surface, p.surface_raised, p.surface_sunken, p.brand_subtle]),
                ("brand", p.brand, vec![p.surface, p.surface_raised, p.brand_subtle]),
                ("on-brand", p.on_brand, vec![p.brand, p.brand_strong]),
                ("success", p.success, vec![p.surface, p.surface_raised]),
                ("info", p.info, vec![p.surface, p.surface_raised]),
                ("warning", p.warning, vec![p.surface, p.surface_raised]),
                ("danger", p.danger, vec![p.surface, p.surface_raised]),
            ];
            for (name, fg, grounds) in text_on {
                for g in grounds {
                    let c = contrast(fg, g);
                    assert!(c >= 4.5, "{theme}: {name} on {} is {c:.2}:1", hex(g));
                }
            }
            for (name, fg) in [("focus-ring", p.focus_ring), ("border-strong", p.border_strong)] {
                for g in [p.surface, p.surface_raised] {
                    let c = contrast(fg, g);
                    assert!(c >= 3.0, "{theme}: {name} on {} is {c:.2}:1", hex(g));
                }
            }
            let c = contrast(p.focus_ring, p.surface_sunken);
            assert!(c >= 3.0, "{theme}: focus ring on surface-sunken is {c:.2}:1");
        }
    }
}
