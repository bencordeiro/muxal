//! Theme registration + helpers: load the bundled themes, apply a saved theme,
//! switch themes/mode at runtime, and derive a terminal color palette from the
//! active gpui-component theme.

use gpui::{Action, App, Global, Hsla, Rgba, SharedString, px};
use gpui_component::{ActiveTheme, Theme, ThemeConfig, ThemeRegistry};
use muxal_core::ambience::{AMBIENCE_MAX, ambience_blend};
use muxal_terminal::TerminalPalette;
use rust_embed::RustEmbed;
use serde::Deserialize;
use std::collections::HashMap;
use std::rc::Rc;

/// Fallback interface font size (gpui-component's `Root` drives the window
/// `rem_size` from `theme.font_size`, so this sizes all non-terminal UI text +
/// spacing). Overridable per-user via [`UiFontSize`].
const DEFAULT_UI_FONT_SIZE: f32 = 16.0;

// GPUI's registry keeps its built-in entries and won't replace a duplicate
// name. Use a private registry key for our palette, retaining "Default Dark"
// in the picker and saved settings. Registry reloads then keep the right colors.
const DEFAULT_DARK_THEME: &str = "Muxal Default Dark";

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
#[action(namespace = muxal, no_json)]
pub struct SwitchTheme(pub SharedString);

/// Bundled theme JSON files: curated upstream themes and original muxal palettes.
#[derive(RustEmbed)]
#[folder = "assets/themes"]
struct ThemeAssets;

/// Optional per-theme colors for the areas GPUI's theme schema has no key for.
/// Every value is optional: anything a theme omits keeps its pre-zone color.
#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct ZoneSpec {
    floor: Option<String>,
    divider: Option<String>,
    focused_header: Option<String>,
    focused_header_text: Option<String>,
    idle_header: Option<String>,
    idle_header_text: Option<String>,
    idle_border: Option<String>,
    terminal: TerminalSpec,
    status: Option<StatusSpec>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct TerminalSpec {
    background: Option<String>,
    foreground: Option<String>,
    cursor: Option<String>,
    selection: Option<String>,
    ansi: Option<[String; 16]>,
}

/// Status colors. `working` / `input` / `done` set the status color itself
/// (icons, tab underline, badge tint), for a palette whose accent, warning,
/// and success colors don't tell the three apart; omitted, they stay the
/// theme's primary / warning / success. The `*_text` values are the text on
/// soft, tinted badges; with neither kind set, badges stay solid.
#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct StatusSpec {
    working: Option<String>,
    input: Option<String>,
    done: Option<String>,
    working_text: Option<String>,
    input_text: Option<String>,
    done_text: Option<String>,
}

#[derive(Deserialize)]
struct ZoneFile {
    themes: Vec<ZoneTheme>,
}

#[derive(Deserialize)]
struct ZoneTheme {
    name: String,
    muxal: Option<ZoneSpec>,
}

struct ZoneSpecs(HashMap<String, ZoneSpec>);
impl Global for ZoneSpecs {}

/// One color per status, resolved for the active theme (the status colors
/// themselves, or the text on soft badges).
#[derive(Clone, Copy)]
pub struct SoftStatus {
    pub working: Hsla,
    pub input: Hsla,
    pub done: Hsla,
}

/// Resolved colors for each independently themed area of the window.
#[derive(Clone, Copy)]
pub struct Zones {
    pub floor: Hsla,
    pub divider: Hsla,
    pub focused_header: Hsla,
    pub focused_header_text: Hsla,
    pub idle_header: Hsla,
    pub idle_header_text: Hsla,
    pub idle_border: Hsla,
    pub terminal_background: Hsla,
    pub terminal_foreground: Hsla,
    pub terminal_cursor: Hsla,
    pub terminal_selection: Hsla,
    pub terminal_ansi: [Hsla; 16],
    /// True when the terminal's light/dark sense is opposite the app's, so
    /// elements drawn on it can't reuse the app's text colors.
    pub terminal_inverted: bool,
    /// The color of each status: icons, tab underlines, badge tints.
    pub status: SoftStatus,
    pub soft_status: Option<SoftStatus>,
}
impl Global for Zones {}

impl Zones {
    /// The muted tone for text drawn directly on the terminal surface.
    pub fn terminal_muted(&self) -> Hsla {
        self.terminal_ansi[8]
    }
}

/// The app-theme colors each zone falls back to.
struct ZoneBase {
    background: Hsla,
    foreground: Hsla,
    caret: Hsla,
    selection: Hsla,
    border: Hsla,
    title_bar: Hsla,
    secondary: Hsla,
    muted_foreground: Hsla,
    primary: Hsla,
    warning: Hsla,
    success: Hsla,
    ansi: [Hsla; 16],
    is_dark: bool,
}

fn zone_base(cx: &App) -> ZoneBase {
    let t = cx.theme();
    ZoneBase {
        background: t.background,
        foreground: t.foreground,
        caret: t.caret,
        selection: t.selection,
        border: t.border,
        title_bar: t.title_bar,
        secondary: t.secondary,
        muted_foreground: t.muted_foreground,
        primary: t.primary,
        warning: t.warning,
        success: t.success,
        ansi: [
            t.muted,            // 0  black (~ subtle bg)
            t.red,              // 1  red
            t.green,            // 2  green
            t.yellow,           // 3  yellow
            t.blue,             // 4  blue
            t.magenta,          // 5  magenta
            t.cyan,             // 6  cyan
            t.foreground,       // 7  white (~ fg)
            t.muted_foreground, // 8  bright black
            t.red_light,        // 9  bright red
            t.green_light,      // 10 bright green
            t.yellow_light,     // 11 bright yellow
            t.blue_light,       // 12 bright blue
            t.magenta_light,    // 13 bright magenta
            t.cyan_light,       // 14 bright cyan
            t.foreground,       // 15 bright white
        ],
        is_dark: t.mode.is_dark(),
    }
}

/// Parse a theme color, applying the ambience cast the rest of the theme gets.
fn zone_color(value: &str, strength: f32) -> Option<Hsla> {
    let tinted = (strength != 0.0)
        .then(|| tint_color(value, strength))
        .flatten();
    let rgba = Rgba::try_from(tinted.as_deref().unwrap_or(value)).ok()?;
    Some(rgba.into())
}

fn resolve_zones(spec: Option<&ZoneSpec>, base: &ZoneBase, strength: f32) -> Zones {
    let default_spec = ZoneSpec::default();
    let spec = spec.unwrap_or(&default_spec);
    let pick = |value: &Option<String>, fallback: Hsla| {
        value
            .as_deref()
            .and_then(|value| zone_color(value, strength))
            .unwrap_or(fallback)
    };
    let terminal_background = pick(&spec.terminal.background, base.background);
    let terminal_ansi = std::array::from_fn(|i| {
        spec.terminal
            .ansi
            .as_ref()
            .and_then(|ansi| zone_color(&ansi[i], strength))
            .unwrap_or(base.ansi[i])
    });
    let status_spec = spec.status.clone().unwrap_or_default();
    let status = SoftStatus {
        working: pick(&status_spec.working, base.primary),
        input: pick(&status_spec.input, base.warning),
        done: pick(&status_spec.done, base.success),
    };
    let text = |value: &Option<String>| value.as_deref().and_then(|v| zone_color(v, strength));
    let overrides_status =
        status_spec.working.is_some() || status_spec.input.is_some() || status_spec.done.is_some();
    // Badge text: the theme's own, else — when it replaced a status color, so
    // the component library's solid primary/warning/success badge would show
    // the wrong one — the status colors themselves on their tint.
    let soft_status = match (
        text(&status_spec.working_text),
        text(&status_spec.input_text),
        text(&status_spec.done_text),
    ) {
        (Some(working), Some(input), Some(done)) => Some(SoftStatus {
            working,
            input,
            done,
        }),
        _ => overrides_status.then_some(status),
    };
    Zones {
        floor: pick(&spec.floor, base.background),
        divider: pick(&spec.divider, base.border),
        focused_header: pick(&spec.focused_header, base.title_bar),
        focused_header_text: pick(&spec.focused_header_text, base.muted_foreground),
        idle_header: pick(&spec.idle_header, base.secondary),
        idle_header_text: pick(&spec.idle_header_text, base.muted_foreground),
        idle_border: pick(&spec.idle_border, base.border),
        terminal_background,
        terminal_foreground: pick(&spec.terminal.foreground, base.foreground),
        terminal_cursor: pick(&spec.terminal.cursor, base.caret),
        terminal_selection: pick(&spec.terminal.selection, base.selection),
        terminal_ansi,
        terminal_inverted: base.is_dark == (terminal_background.l > 0.5),
        status,
        soft_status,
    }
}

/// Recompute the zones after a theme or ambience change.
fn set_zones(theme_name: &str, cx: &mut App) {
    let strength = ambience_blend(ambience_cast(cx));
    let spec = cx
        .try_global::<ZoneSpecs>()
        .and_then(|specs| specs.0.get(theme_name).cloned());
    let zones = resolve_zones(spec.as_ref(), &zone_base(cx), strength);
    cx.set_global(zones);
}

/// The active theme's zone colors.
pub fn zones(cx: &App) -> Zones {
    cx.try_global::<Zones>()
        .copied()
        .unwrap_or_else(|| resolve_zones(None, &zone_base(cx), 0.0))
}

/// Load all bundled theme sets into the global registry.
pub fn register_bundled_themes(cx: &mut App) {
    let mut loaded = 0;
    let mut zone_specs = HashMap::new();
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
        match serde_json::from_str::<ZoneFile>(text) {
            Ok(set) => zone_specs.extend(
                set.themes
                    .into_iter()
                    .filter_map(|theme| Some((theme.name, theme.muxal?))),
            ),
            Err(e) => log::warn!("failed to read zones in theme {file}: {e}"),
        }
    }
    cx.set_global(ZoneSpecs(zone_specs));
    log::info!("loaded {loaded} bundled theme file(s)");
}

/// The colors that tell one theme from another at a glance, as authored (no
/// ambience cast): its chrome, a terminal, the focused pane header, the
/// accent, and the three status colors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThemeSwatch {
    pub chrome: Hsla,
    pub terminal: Hsla,
    pub text: Hsla,
    pub header: Hsla,
    pub accent: Hsla,
    pub working: Hsla,
    pub input: Hsla,
    pub done: Hsla,
    pub border: Hsla,
}

/// Resolve a theme's swatch the way the app resolves the theme itself: zone
/// colors first, then the theme's own, then the component library's defaults
/// for its light/dark mode.
fn theme_swatch(config: &ThemeConfig, zone: Option<&ZoneSpec>) -> ThemeSwatch {
    let defaults = if config.mode.is_dark() {
        gpui_component::ThemeColor::dark()
    } else {
        gpui_component::ThemeColor::light()
    };
    let own = |value: &Option<SharedString>, fallback: Hsla| {
        value
            .as_deref()
            .and_then(|value| zone_color(value, 0.0))
            .unwrap_or(fallback)
    };
    let zoned = |value: Option<&String>, fallback: Hsla| {
        value
            .and_then(|value| zone_color(value, 0.0))
            .unwrap_or(fallback)
    };
    let c = &config.colors;
    let background = own(&c.background, defaults.background);
    let status = zone.and_then(|z| z.status.as_ref());
    ThemeSwatch {
        chrome: own(&c.sidebar, background),
        terminal: zoned(
            zone.and_then(|z| z.terminal.background.as_ref()),
            background,
        ),
        text: zoned(
            zone.and_then(|z| z.terminal.foreground.as_ref()),
            own(&c.foreground, defaults.foreground),
        ),
        header: zoned(
            zone.and_then(|z| z.focused_header.as_ref()),
            own(&c.title_bar, defaults.title_bar),
        ),
        accent: own(&c.primary, defaults.primary),
        working: zoned(
            status.and_then(|s| s.working.as_ref()),
            own(&c.primary, defaults.primary),
        ),
        input: zoned(
            status.and_then(|s| s.input.as_ref()),
            own(&c.warning, defaults.warning),
        ),
        done: zoned(
            status.and_then(|s| s.done.as_ref()),
            own(&c.success, defaults.success),
        ),
        border: own(&c.border, defaults.border),
    }
}

/// One row of the theme picker: the name beside a miniature of the theme.
#[derive(Clone)]
pub struct ThemeChoice {
    name: SharedString,
    swatch: ThemeSwatch,
}

impl ThemeChoice {
    /// A tiny window — a strip of chrome, then a pane with its focused header
    /// over a terminal holding one line of text and one in the accent — and
    /// the working / input / done colors as dots.
    fn row(&self) -> gpui::Div {
        use gpui::{ParentElement, Styled, div};
        let s = self.swatch;
        let line = |color: Hsla, width: f32| div().h(px(2.0)).w(px(width)).rounded_full().bg(color);
        let dot = |color: Hsla| div().size(px(7.0)).rounded_full().flex_none().bg(color);
        div()
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .child(
                div()
                    .flex_none()
                    .flex()
                    .w(px(46.0))
                    .h(px(20.0))
                    .rounded(px(3.0))
                    .border_1()
                    .border_color(s.border)
                    .overflow_hidden()
                    .child(div().w(px(10.0)).h_full().bg(s.chrome))
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .flex()
                            .flex_col()
                            .child(div().h(px(5.0)).w_full().bg(s.header))
                            .child(
                                div()
                                    .flex_1()
                                    .px(px(4.0))
                                    .flex()
                                    .flex_col()
                                    .justify_center()
                                    .gap(px(2.0))
                                    .bg(s.terminal)
                                    .child(line(s.text, 22.0))
                                    .child(line(s.accent, 13.0)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(self.name.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(3.0))
                    .child(dot(s.working))
                    .child(dot(s.input))
                    .child(dot(s.done)),
            )
    }
}

impl gpui_component::select::SelectItem for ThemeChoice {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.name.clone()
    }

    fn display_title(&self) -> Option<gpui::AnyElement> {
        Some(gpui::IntoElement::into_any_element(self.row()))
    }

    fn render(&self, _: &mut gpui::Window, _: &mut App) -> impl gpui::IntoElement {
        self.row()
    }

    fn value(&self) -> &SharedString {
        &self.name
    }
}

/// All available themes with their swatches, sorted for the picker.
pub fn theme_choices(cx: &App) -> Vec<ThemeChoice> {
    let zones = cx.try_global::<ZoneSpecs>();
    ThemeRegistry::global(cx)
        .sorted_themes()
        .iter()
        .filter(|c| c.name.as_ref() != "Default Dark")
        .map(|c| ThemeChoice {
            name: display_theme_name(&c.name).to_string().into(),
            swatch: theme_swatch(c, zones.and_then(|z| z.0.get(c.name.as_ref()))),
        })
        .collect()
}

/// The public name stays stable even when a palette has a private registry key.
pub fn theme_name(cx: &App) -> SharedString {
    display_theme_name(cx.theme().theme_name())
        .to_string()
        .into()
}

fn display_theme_name(name: &str) -> &str {
    if name == DEFAULT_DARK_THEME {
        "Default Dark"
    } else {
        name
    }
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
            .or_else(|| registry.themes().get(DEFAULT_DARK_THEME).cloned())
            .unwrap_or_else(|| registry.default_dark_theme().clone())
    };
    let tinted = ambience_config(&config, ambience_blend(ambience_cast(cx)));
    Theme::global_mut(cx).apply_config(&tinted);
    set_zones(config.name.as_ref(), cx);
    apply_scale(cx);
    apply_mono_font(cx);
}

/// Apply a theme by name at runtime and refresh open windows.
pub fn apply_theme(name: &str, cx: &mut App) {
    let name = canonical_theme_name(name);
    let config = ThemeRegistry::global(cx).themes().get(name).cloned();
    if let Some(config) = config {
        let tinted = ambience_config(&config, ambience_blend(ambience_cast(cx)));
        Theme::global_mut(cx).apply_config(&tinted);
        set_zones(config.name.as_ref(), cx);
        apply_scale(cx);
        apply_mono_font(cx);
        cx.refresh_windows();
    }
}

/// Preserve saved choices when variants are consolidated or a palette is
/// replaced. Other removed palettes use the startup fallback, without
/// reappearing in the picker.
fn canonical_theme_name(name: &str) -> &str {
    match name {
        "" | "Default Dark" => DEFAULT_DARK_THEME,
        "Hacker" => "Mainframe",
        "Tokyo Night" | "Tokyo Storm" | "Tokyo Moon" => "Tokyo Dark",
        "Twilight" => "Twilight Grey",
        "Catppuccin Macchiato" | "Catppuccin Mocha" => "Catppuccin Frappe",
        // Flight Deck took Ember Observatory's place in the collection.
        "Ember Observatory" => "Flight Deck",
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
    let resolved = muxal_terminal::resolve_mono_family(cx.text_system(), "");
    Theme::global_mut(cx).mono_font_family = resolved;
}

fn hsla_to_u32(c: Hsla) -> u32 {
    let rgba: Rgba = c.into();
    let r = (rgba.r.clamp(0.0, 1.0) * 255.0).round() as u32;
    let g = (rgba.g.clamp(0.0, 1.0) * 255.0).round() as u32;
    let b = (rgba.b.clamp(0.0, 1.0) * 255.0).round() as u32;
    (r << 16) | (g << 8) | b
}

/// Build a terminal color palette from the active theme's terminal zone.
pub fn palette_from_theme(cx: &App) -> TerminalPalette {
    let z = zones(cx);
    TerminalPalette {
        spaceglass: is_spaceglass(cx),
        background: hsla_to_u32(z.terminal_background),
        foreground: hsla_to_u32(z.terminal_foreground),
        cursor: hsla_to_u32(z.terminal_cursor),
        selection: hsla_to_u32(z.terminal_selection),
        ansi: z.terminal_ansi.map(hsla_to_u32),
    }
}

/// WCAG relative luminance of a color (alpha ignored).
fn luminance(color: Hsla) -> f32 {
    let rgb: gpui::Rgba = color.into();
    let lin = |c: f32| {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(rgb.r) + 0.7152 * lin(rgb.g) + 0.0722 * lin(rgb.b)
}

/// WCAG contrast ratio between two colors, 1.0 (identical) to 21.0.
pub fn contrast(a: Hsla, b: Hsla) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The least contrast a status mark needs against the surface it sits on
/// (WCAG's 3:1 for graphics).
const MARK_CONTRAST: f32 = 3.0;

/// `color` made visible on `surface`. Status colors are chosen against the
/// app's own surfaces, but a mark can land on any zone — a "working" icon in
/// the accent color on a focused header painted in that same accent would
/// vanish. A color that already stands out is returned untouched; otherwise
/// its lightness moves away from the surface (hue and saturation kept) until
/// it does, ending at `fallback` — a color known to read on that surface.
pub fn legible_on(color: Hsla, surface: Hsla, fallback: Hsla) -> Hsla {
    if contrast(color, surface) >= MARK_CONTRAST {
        return color;
    }
    let lighten = luminance(fallback) >= luminance(surface);
    let mut c = color;
    for _ in 0..8 {
        c.l = if lighten {
            c.l + (1.0 - c.l) * 0.3
        } else {
            c.l * 0.7
        };
        if contrast(c, surface) >= MARK_CONTRAST {
            return c;
        }
    }
    fallback
}

#[cfg(test)]
mod swatch_tests {
    use super::{ThemeAssets, ZoneFile, theme_swatch};
    use gpui::{Hsla, Rgba};
    use gpui_component::ThemeSet;

    fn color(hex: &str) -> Hsla {
        Rgba::try_from(hex).unwrap().into()
    }

    fn swatch(file: &str, name: &str) -> super::ThemeSwatch {
        let data = ThemeAssets::get(file).unwrap().data;
        let set: ThemeSet = serde_json::from_slice(&data).unwrap();
        let zones: ZoneFile = serde_json::from_slice(&data).unwrap();
        let config = set.themes.iter().find(|t| t.name.as_ref() == name).unwrap();
        let zone = zones.themes.into_iter().find(|t| t.name == name).unwrap();
        theme_swatch(config, zone.muxal.as_ref())
    }

    #[test]
    fn a_zoned_theme_shows_its_own_terminal_header_and_statuses() {
        let s = swatch("flight-deck.json", "Flight Deck");
        assert_eq!(s.chrome, color("#363F49"));
        assert_eq!(s.terminal, color("#0A0C0F"));
        assert_eq!(s.header, color("#7B3A8C"));
        assert_eq!(s.accent, color("#DB7BEB"));
        assert_eq!(s.working, color("#45D9F0"));
        assert_ne!(s.working, s.accent);
    }

    #[test]
    fn a_theme_without_surface_zones_falls_back_to_its_own_colors() {
        // Gruvbox Dark names status colors only.
        let s = swatch("gruvbox.json", "Gruvbox Dark");
        assert_eq!(s.terminal, color("#1d2021"));
        assert_eq!(s.accent, color("#d79921"));
        assert_eq!(s.working, color("#83a598"));
    }
}

#[cfg(test)]
mod status_color_tests {
    use super::ThemeAssets;
    use gpui::{Hsla, Rgba};

    /// Working, input, and done must be told apart by color alone. Checked
    /// for every bundled theme that names all three (its `muxal.status` zone,
    /// else its own primary / warning / success); a theme that leaves one to
    /// the component library's default is skipped.
    #[test]
    fn every_theme_tells_its_three_statuses_apart() {
        const MIN_HUE_GAP: f32 = 20.0;
        let mut checked = 0;
        for file in ThemeAssets::iter() {
            let data = ThemeAssets::get(&file).unwrap().data;
            let set: serde_json::Value = serde_json::from_slice(&data).unwrap();
            for theme in set["themes"].as_array().unwrap() {
                let name = theme["name"].as_str().unwrap();
                let pick = |zone: &str, key: &str| -> Option<Hsla> {
                    let value = theme["muxal"]["status"][zone]
                        .as_str()
                        .or_else(|| theme["colors"][key].as_str())?;
                    Some(Rgba::try_from(value).ok()?.into())
                };
                let (Some(working), Some(input), Some(done)) = (
                    pick("working", "primary.background"),
                    pick("input", "warning.background"),
                    pick("done", "success.background"),
                ) else {
                    continue;
                };
                checked += 1;
                let gap = |a: Hsla, b: Hsla| {
                    let d = (a.h - b.h).abs() * 360.0;
                    d.min(360.0 - d)
                };
                for (what, a, b) in [
                    ("working/input", working, input),
                    ("working/done", working, done),
                    ("input/done", input, done),
                ] {
                    assert!(
                        gap(a, b) >= MIN_HUE_GAP,
                        "{name}: {what} are only {:.0}° of hue apart",
                        gap(a, b)
                    );
                }
            }
        }
        assert!(
            checked >= 14,
            "only {checked} themes named all three statuses"
        );
    }
}

#[cfg(test)]
mod legibility_tests {
    use super::{MARK_CONTRAST, contrast, legible_on};
    use gpui::{Hsla, Rgba};

    fn color(hex: &str) -> Hsla {
        Rgba::try_from(hex).unwrap().into()
    }

    #[test]
    fn a_color_that_already_reads_is_untouched() {
        let blue = color("#7aa2f7");
        let dark = color("#1a1b26");
        assert_eq!(legible_on(blue, dark, color("#ffffff")), blue);
    }

    #[test]
    fn an_accent_on_its_own_header_is_moved_until_it_reads() {
        // Clay Dark's terracotta accent on a terracotta focused header.
        let accent = color("#d97757");
        let header = color("#c96a4c");
        let text = color("#faf9f5");
        assert!(contrast(accent, header) < MARK_CONTRAST);
        let fixed = legible_on(accent, header, text);
        assert!(contrast(fixed, header) >= MARK_CONTRAST);
        // Still the same hue, only lighter.
        assert!((fixed.h - accent.h).abs() < 0.01);
        assert!(fixed.l > accent.l);
    }

    #[test]
    fn a_light_surface_darkens_the_color_instead() {
        let amber = color("#f5c542");
        let paper = color("#f4efe4");
        let ink = color("#2b2a27");
        let fixed = legible_on(amber, paper, ink);
        assert!(contrast(fixed, paper) >= MARK_CONTRAST);
        assert!(fixed.l < amber.l);
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_DARK_THEME, ThemeAssets, canonical_theme_name, display_theme_name};
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
        // Default Light is supplied by GPUI. Our Default Dark has a private
        // registry key so GPUI's duplicate-name guard cannot discard it.
        let expected: BTreeSet<_> = [
            "Alduin",
            "Ayu Dark",
            "Ayu Light",
            "Catppuccin Frappe",
            "Catppuccin Latte",
            "Everforest Dark",
            "Fahrenheit",
            "Flight Deck",
            "Gruvbox Dark",
            "Gruvbox Light",
            "Mainframe",
            "Muxal Default Dark",
            "Liquid Spaceglass",
            "Molokai Dark",
            "Porcelain",
            "Spaceduck",
            "Tokyo Dark",
            "Twilight Grey",
            "Clay Dark",
            "Verdigris",
            "Rosewood",
            "Ledger",
            "Ledger Dark",
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
            "Default Dark",
            "",
        ] {
            assert!(names.contains(canonical_theme_name(old)));
        }
        for name in &names {
            assert_eq!(canonical_theme_name(name), name);
        }
    }
    #[test]
    fn default_dark_keeps_its_public_name_and_maps_fresh_and_saved_choices() {
        assert_eq!(canonical_theme_name(""), DEFAULT_DARK_THEME);
        assert_eq!(canonical_theme_name("Default Dark"), DEFAULT_DARK_THEME);
        assert_eq!(display_theme_name(DEFAULT_DARK_THEME), "Default Dark");
        assert_eq!(display_theme_name("Default Light"), "Default Light");
        assert_eq!(canonical_theme_name("Ayu Dark"), "Ayu Dark");
    }
}

#[cfg(test)]
mod zone_tests {
    use super::{ThemeAssets, ZoneBase, ZoneFile, ZoneSpec, resolve_zones, zone_color};
    use gpui::{Hsla, Rgba};

    fn color(hex: &str) -> Hsla {
        Rgba::try_from(hex).unwrap().into()
    }

    fn base(is_dark: bool) -> ZoneBase {
        ZoneBase {
            background: color("#101010"),
            foreground: color("#eeeeee"),
            caret: color("#00ff00"),
            selection: color("#333366"),
            border: color("#444444"),
            title_bar: color("#202020"),
            secondary: color("#303030"),
            muted_foreground: color("#999999"),
            primary: color("#3366ff"),
            warning: color("#ffaa00"),
            success: color("#33cc66"),
            ansi: std::array::from_fn(|_| color("#777777")),
            is_dark,
        }
    }

    fn spec(terminal_background: &str) -> ZoneSpec {
        serde_json::from_value(serde_json::json!({
            "floor": "#0a0a0a",
            "terminal": { "background": terminal_background }
        }))
        .unwrap()
    }

    #[test]
    fn a_theme_without_zones_keeps_every_previous_color() {
        let z = resolve_zones(None, &base(true), 0.0);
        assert_eq!(z.floor, color("#101010"));
        assert_eq!(z.terminal_background, color("#101010"));
        assert_eq!(z.terminal_foreground, color("#eeeeee"));
        assert_eq!(z.focused_header, color("#202020"));
        assert_eq!(z.idle_header, color("#303030"));
        assert_eq!(z.idle_border, color("#444444"));
        assert_eq!(z.divider, color("#444444"));
        assert!(!z.terminal_inverted);
        assert!(z.soft_status.is_none());
    }

    #[test]
    fn zones_override_only_the_areas_a_theme_sets() {
        let z = resolve_zones(Some(&spec("#000000")), &base(true), 0.0);
        assert_eq!(z.floor, color("#0a0a0a"));
        assert_eq!(z.terminal_background, color("#000000"));
        assert_eq!(z.idle_header, color("#303030"));
    }

    #[test]
    fn a_terminal_opposite_the_app_mode_is_inverted() {
        assert!(resolve_zones(Some(&spec("#000000")), &base(false), 0.0).terminal_inverted);
        assert!(resolve_zones(Some(&spec("#ffffff")), &base(true), 0.0).terminal_inverted);
        assert!(!resolve_zones(Some(&spec("#000000")), &base(true), 0.0).terminal_inverted);
    }

    #[test]
    fn ambience_tints_zone_colors() {
        assert_ne!(zone_color("#808080", 0.15), zone_color("#808080", 0.0));
    }

    #[test]
    fn invalid_zone_values_fall_back() {
        let z = resolve_zones(Some(&spec("nope")), &base(true), 0.0);
        assert_eq!(z.terminal_background, color("#101010"));
    }

    #[test]
    fn every_bundled_zone_color_parses() {
        for file in ThemeAssets::iter() {
            let asset = ThemeAssets::get(&file).unwrap();
            let set: ZoneFile = serde_json::from_slice(&asset.data).unwrap();
            for theme in set.themes {
                let Some(spec) = theme.muxal else { continue };
                // Whatever status colors a theme names must parse.
                let status = spec.status.clone().unwrap_or_default();
                for value in [
                    &status.working,
                    &status.input,
                    &status.done,
                    &status.working_text,
                    &status.input_text,
                    &status.done_text,
                ]
                .into_iter()
                .flatten()
                {
                    assert!(zone_color(value, 0.0).is_some(), "{}: {value}", theme.name);
                }
                // A theme may use the zone for status colors alone; one that
                // themes any surface must theme all of them.
                if spec.floor.is_none() && spec.terminal.background.is_none() {
                    continue;
                }
                for value in [
                    &spec.floor,
                    &spec.divider,
                    &spec.focused_header,
                    &spec.focused_header_text,
                    &spec.idle_header,
                    &spec.idle_header_text,
                    &spec.idle_border,
                    &spec.terminal.background,
                    &spec.terminal.foreground,
                    &spec.terminal.cursor,
                    &spec.terminal.selection,
                ] {
                    let value = value.as_deref().expect(&theme.name);
                    assert!(zone_color(value, 0.0).is_some(), "{}: {value}", theme.name);
                }
                let ansi = spec.terminal.ansi.expect(&theme.name);
                assert!(ansi.iter().all(|c| zone_color(c, 0.0).is_some()));
                assert!(spec.status.is_some(), "{}", theme.name);
            }
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
