//! Theme registration + helpers: load the bundled themes, apply a saved theme,
//! switch themes/mode at runtime, and derive a terminal color palette from the
//! active gpui-component theme.

use gpui::{Action, App, Global, Hsla, Rgba, SharedString, px};
use gpui_component::{ActiveTheme, Theme, ThemeConfig, ThemeRegistry};
use muxel_core::ambience::{AMBIENCE_MAX, ambience_blend};
use muxel_terminal::TerminalPalette;
use rust_embed::RustEmbed;
use std::rc::Rc;

/// Fallback interface font size (gpui-component's `Root` drives the window
/// `rem_size` from `theme.font_size`, so this sizes all non-terminal UI text +
/// spacing). Overridable per-user via [`UiFontSize`].
const DEFAULT_UI_FONT_SIZE: f32 = 16.0;

/// Global UI scale (zoom) factor for the whole app.
pub struct UiScale(pub f32);
impl Global for UiScale {}

/// Interface (non-terminal) base font size. Independent of the terminal font.
pub struct UiFontSize(pub f32);
impl Global for UiFontSize {}

/// Warm/cool ambience cast (-50 cool .. +50 warm; 0 = theme as authored).
pub struct Ambience(pub i8);
impl Global for Ambience {}

fn ambience_cast(cx: &App) -> i8 {
    cx.try_global::<Ambience>().map(|a| a.0).unwrap_or(0)
}

/// Set the ambience cast (clamped). Callers re-apply the theme (which tints
/// every color) and refresh terminal palettes.
pub fn set_ambience(cast: i8, cx: &mut App) {
    cx.set_global(Ambience(cast.clamp(-AMBIENCE_MAX, AMBIENCE_MAX)));
}

fn ui_scale(cx: &App) -> f32 {
    cx.try_global::<UiScale>().map(|s| s.0).unwrap_or(1.0)
}

fn ui_font_size(cx: &App) -> f32 {
    cx.try_global::<UiFontSize>()
        .map(|s| s.0)
        .unwrap_or(DEFAULT_UI_FONT_SIZE)
}

/// Rewrite a theme config with every color tinted by `strength` (signed
/// warm/cool blend; 0 returns the config unchanged). Backgrounds, chrome and
/// text all drift together - like a monitor color filter - so relative
/// contrast is preserved. Syntax-highlight colors are untouched.
fn ambience_config(config: &Rc<ThemeConfig>, strength: f32) -> Rc<ThemeConfig> {
    if strength == 0.0 {
        return config.clone();
    }
    let Ok(mut colors) = serde_json::to_value(&config.colors) else {
        return config.clone();
    };
    let Some(map) = colors.as_object_mut() else {
        return config.clone();
    };
    for value in map.values_mut() {
        if let Some(s) = value.as_str()
            && let Some(tinted) = tint_color(s, strength)
        {
            *value = serde_json::Value::String(tinted);
        }
    }
    let Ok(new_colors) = serde_json::from_value(colors) else {
        return config.clone();
    };
    let mut out = (**config).clone();
    out.colors = new_colors;
    Rc::new(out)
}

/// Blend a theme color toward the warm or cool pole by `strength` (signed; see
/// [`ambience_blend`] for the range). Alpha is preserved. Handles the `#rrggbb`
/// / `#rrggbbaa` forms the bundled themes use; `None` for anything else (left
/// unchanged).
fn tint_color(color: &str, strength: f32) -> Option<String> {
    let hex = color.trim().strip_prefix('#')?;
    let (r, g, b, a) = match hex.len() {
        6 => (
            hex_byte(hex, 0)?,
            hex_byte(hex, 2)?,
            hex_byte(hex, 4)?,
            255u8,
        ),
        8 => (
            hex_byte(hex, 0)?,
            hex_byte(hex, 2)?,
            hex_byte(hex, 4)?,
            hex_byte(hex, 6)?,
        ),
        _ => return None,
    };
    // Warm (#ffb35c) and cool (#78b2ff) poles, 0..=1.
    let (tr, tg, tb) = if strength >= 0.0 {
        (1.0, 0.70, 0.36)
    } else {
        (0.47, 0.70, 1.0)
    };
    let k = strength.abs();
    let blend = |c: u8, t: f32| -> u8 {
        let c = f32::from(c) / 255.0;
        ((c + (t - c) * k) * 255.0).round().clamp(0.0, 255.0) as u8
    };
    Some(format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        blend(r, tr),
        blend(g, tg),
        blend(b, tb),
        a
    ))
}

fn hex_byte(hex: &str, at: usize) -> Option<u8> {
    u8::from_str_radix(hex.get(at..at + 2)?, 16).ok()
}

/// Re-apply the interface font size × zoom to the active theme's font size.
/// Called after every theme/mode change (which resets `font_size`) and whenever
/// the UI font size or zoom changes. The terminal font is sized separately.
fn apply_scale(cx: &mut App) {
    let size = ui_font_size(cx) * ui_scale(cx);
    Theme::global_mut(cx).font_size = px(size);
}

/// Set the whole-app UI scale (zoom) and refresh.
pub fn set_ui_scale(scale: f32, cx: &mut App) {
    cx.set_global(UiScale(scale));
    apply_scale(cx);
    cx.refresh_windows();
}

/// Set the interface (non-terminal) base font size and refresh.
pub fn set_ui_font_size(size: f32, cx: &mut App) {
    cx.set_global(UiFontSize(size));
    apply_scale(cx);
    cx.refresh_windows();
}

/// Dispatched by the theme-switcher menu items; applies the named theme.
#[derive(Action, Clone, PartialEq)]
#[action(namespace = muxel, no_json)]
pub struct SwitchTheme(pub SharedString);

/// The bundled theme JSON files (vendored from gpui-component's `themes/`).
#[derive(RustEmbed)]
#[folder = "assets/themes"]
struct ThemeAssets;

/// Load all bundled theme sets into the global registry.
pub fn register_bundled_themes(cx: &mut App) {
    let mut loaded = 0;
    for file in ThemeAssets::iter() {
        let Some(embedded) = ThemeAssets::get(&file) else {
            continue;
        };
        let Ok(text) = std::str::from_utf8(&embedded.data) else {
            continue;
        };
        match ThemeRegistry::global_mut(cx).load_themes_from_str(text) {
            Ok(()) => loaded += 1,
            Err(e) => log::warn!("failed to load theme {file}: {e}"),
        }
    }
    log::info!("loaded {loaded} bundled theme file(s)");
}

/// All available theme names, sorted for display in the switcher.
pub fn theme_names(cx: &App) -> Vec<SharedString> {
    ThemeRegistry::global(cx)
        .sorted_themes()
        .iter()
        .map(|c| c.name.clone())
        .collect()
}

/// Apply the saved theme by name at startup (falls back to the default dark theme).
pub fn apply_initial_theme(name: &str, cx: &mut App) {
    let name = canonical_theme_name(name);
    let config = {
        let registry = ThemeRegistry::global(cx);
        registry
            .themes()
            .get(name)
            .cloned()
            .unwrap_or_else(|| registry.default_dark_theme().clone())
    };
    let config = ambience_config(&config, ambience_blend(ambience_cast(cx)));
    Theme::global_mut(cx).apply_config(&config);
    apply_scale(cx);
    apply_mono_font(cx);
}

/// Apply a theme by name at runtime and refresh open windows.
pub fn apply_theme(name: &str, cx: &mut App) {
    let name = canonical_theme_name(name);
    let config = ThemeRegistry::global(cx).themes().get(name).cloned();
    if let Some(config) = config {
        let config = ambience_config(&config, ambience_blend(ambience_cast(cx)));
        Theme::global_mut(cx).apply_config(&config);
        apply_scale(cx);
        apply_mono_font(cx);
        cx.refresh_windows();
    }
}

/// Preserve saved choices when variants are consolidated. Removed palettes
/// still use the startup fallback, without reappearing in the picker.
fn canonical_theme_name(name: &str) -> &str {
    match name {
        "Hacker" => "Mainframe",
        "Tokyo Night" | "Tokyo Storm" | "Tokyo Moon" => "Tokyo Dark",
        "Twilight" => "Twilight Grey",
        "Catppuccin Macchiato" | "Catppuccin Mocha" => "Catppuccin Frappe",
        _ => name,
    }
}

pub fn is_spaceglass(cx: &App) -> bool {
    cx.theme().theme_name().as_ref() == "Liquid Spaceglass"
}

/// Re-point gpui-component's global monospace family at an installed,
/// verifiably fixed-pitch face. gpui silently substitutes a proportional UI
/// face for families that aren't installed (the Arch/Omarchy spaced-text bug),
/// which would otherwise leak into code/diff views.
pub fn apply_mono_font(cx: &mut App) {
    let resolved = muxel_terminal::resolve_mono_family(cx.text_system(), "");
    Theme::global_mut(cx).mono_font_family = resolved;
}

fn hsla_to_u32(c: Hsla) -> u32 {
    let rgba: Rgba = c.into();
    let r = (rgba.r.clamp(0.0, 1.0) * 255.0).round() as u32;
    let g = (rgba.g.clamp(0.0, 1.0) * 255.0).round() as u32;
    let b = (rgba.b.clamp(0.0, 1.0) * 255.0).round() as u32;
    (r << 16) | (g << 8) | b
}

/// Build a terminal color palette from the active theme.
///
/// gpui-component themes expose the six ANSI hues (+ `_light` brights) plus
/// background/foreground; black/white are derived from muted/foreground.
pub fn palette_from_theme(cx: &App) -> TerminalPalette {
    let t = cx.theme();
    // Mainframe keeps its chrome neutral while terminal text uses phosphor green.
    let foreground = if t.theme_name().as_ref() == "Mainframe" {
        t.green
    } else {
        t.foreground
    };
    TerminalPalette {
        spaceglass: is_spaceglass(cx),
        background: hsla_to_u32(t.background),
        foreground: hsla_to_u32(foreground),
        cursor: hsla_to_u32(t.caret),
        selection: hsla_to_u32(t.selection),
        ansi: [
            hsla_to_u32(t.muted),            // 0  black (~ subtle bg)
            hsla_to_u32(t.red),              // 1  red
            hsla_to_u32(t.green),            // 2  green
            hsla_to_u32(t.yellow),           // 3  yellow
            hsla_to_u32(t.blue),             // 4  blue
            hsla_to_u32(t.magenta),          // 5  magenta
            hsla_to_u32(t.cyan),             // 6  cyan
            hsla_to_u32(t.foreground),       // 7  white (~ fg)
            hsla_to_u32(t.muted_foreground), // 8  bright black
            hsla_to_u32(t.red_light),        // 9  bright red
            hsla_to_u32(t.green_light),      // 10 bright green
            hsla_to_u32(t.yellow_light),     // 11 bright yellow
            hsla_to_u32(t.blue_light),       // 12 bright blue
            hsla_to_u32(t.magenta_light),    // 13 bright magenta
            hsla_to_u32(t.cyan_light),       // 14 bright cyan
            hsla_to_u32(t.foreground),       // 15 bright white
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::{ThemeAssets, canonical_theme_name};
    use gpui_component::ThemeSet;
    use std::collections::BTreeSet;

    #[test]
    fn bundled_themes_load_and_match_the_curated_collection() {
        let mut names = BTreeSet::new();
        for file in ThemeAssets::iter() {
            let asset = ThemeAssets::get(&file).unwrap();
            let set: ThemeSet = serde_json::from_slice(&asset.data).unwrap();
            for theme in set.themes {
                assert!(names.insert(theme.name.to_string()), "duplicate theme");
            }
        }
        // Default Light/Dark are supplied by gpui-component itself.
        let expected: BTreeSet<_> = [
            "Alduin",
            "Ayu Dark",
            "Ayu Light",
            "Catppuccin Frappe",
            "Catppuccin Latte",
            "Ember Observatory",
            "Everforest Dark",
            "Fahrenheit",
            "Gruvbox Dark",
            "Gruvbox Light",
            "Mainframe",
            "Liquid Spaceglass",
            "Molokai Dark",
            "Porcelain",
            "Spaceduck",
            "Tokyo Dark",
            "Twilight Grey",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        assert_eq!(names, expected);
        for old in [
            "Hacker",
            "Tokyo Night",
            "Tokyo Storm",
            "Tokyo Moon",
            "Twilight",
            "Catppuccin Macchiato",
            "Catppuccin Mocha",
        ] {
            assert!(names.contains(canonical_theme_name(old)));
        }
        for name in &names {
            assert_eq!(canonical_theme_name(name), name);
        }
    }
}

#[cfg(test)]
mod ambience_tests {
    use super::{ambience_config, tint_color};
    use gpui_component::ThemeConfig;
    use std::rc::Rc;

    fn rgb_of(color: &str) -> (u8, u8, u8) {
        let hex = color.trim().strip_prefix('#').expect("hex color");
        (
            u8::from_str_radix(&hex[0..2], 16).unwrap(),
            u8::from_str_radix(&hex[2..4], 16).unwrap(),
            u8::from_str_radix(&hex[4..6], 16).unwrap(),
        )
    }

    #[test]
    fn warm_tint_shifts_toward_amber() {
        // #808080 at +0.15: red up, blue down, alpha preserved.
        assert_eq!(tint_color("#808080", 0.15), Some("#93887bff".to_string()));
    }

    #[test]
    fn cool_tint_shifts_toward_steel_blue() {
        assert_eq!(tint_color("#808080", -0.15), Some("#7f8893ff".to_string()));
    }

    #[test]
    fn tint_preserves_alpha_and_leaves_unparsable_values_alone() {
        assert!(tint_color("#80808080", 0.15).unwrap().ends_with("80"));
        assert_eq!(tint_color("nope", 0.15), None);
        assert_eq!(tint_color("#ff00", 0.15), None);
    }

    #[test]
    fn extremes_stay_readable() {
        // The cap keeps dark surfaces dark and light surfaces light.
        let (r, g, b) = rgb_of(&tint_color("#000000", 0.15).unwrap());
        assert!(r <= 39 && g <= 39 && b <= 39, "black stays near-black");
        let (r, g, b) = rgb_of(&tint_color("#ffffff", -0.15).unwrap());
        assert!(r >= 216 && g >= 216 && b >= 216, "white stays near-white");
    }

    #[test]
    fn ambience_config_is_identity_at_zero() {
        let config = Rc::new(
            serde_json::from_value::<ThemeConfig>(serde_json::json!({
                "is_default": false,
                "name": "T",
                "mode": "dark",
                "colors": { "background": "#101010" }
            }))
            .expect("valid theme config"),
        );
        let same = ambience_config(&config, 0.0);
        let colors = serde_json::to_value(&same.colors).expect("colors to json");
        assert_eq!(colors["background"], "#101010");
    }

    #[test]
    fn ambience_config_tints_background_and_foreground_together() {
        let config = Rc::new(
            serde_json::from_value::<ThemeConfig>(serde_json::json!({
                "is_default": false,
                "name": "T",
                "mode": "dark",
                "colors": { "background": "#101010", "foreground": "#eeeeee" }
            }))
            .expect("valid theme config"),
        );
        let tinted = ambience_config(&config, 0.15);
        let colors = serde_json::to_value(&tinted.colors).expect("colors to json");
        assert_ne!(colors["background"], "#101010");
        assert_ne!(colors["foreground"], "#eeeeee");
    }
}
