//! Terminal font-family resolution: pick a family that is installed *and*
//! verifiably fixed-pitch.
//!
//! gpui's `TextSystem::resolve_font` silently substitutes its own proportional
//! UI fallback stack when a requested family is missing — it does not error.
//! The terminal then computes its cell width from the `'m'` advance of that
//! proportional face and force-spreads every glyph to it, which renders as
//! broken, spaced-out text. Distro images without DejaVu Sans Mono (Arch /
//! Omarchy ship JetBrainsMono Nerd Font instead) hit this with the old
//! hardcoded default.
//!
//! This module resolves a family that is (1) installed, (2) resolved exactly
//! (no silent substitution), and (3) fixed-pitch by an advance-width probe.
//! Ported from the field fix in the v0.3.0 Arch incident report.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use gpui::{Font, FontFeatures, FontStyle, FontWeight, SharedString, TextSystem, px};

/// Glyphs whose advance widths must agree for a face to count as fixed-pitch.
const PROBE_CHARS: [char; 6] = ['i', 'l', 'W', 'M', 'x', '0'];
const PROBE_SIZE: f32 = 14.0;

/// Per-platform preferred monospace families, most specific first. Nerd-font
/// names lead on Linux because distro images (Arch/Omarchy) ship them while
/// omitting DejaVu entirely.
fn platform_defaults() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    return &[
        "Menlo",
        "SF Mono",
        "Monaco",
        "JetBrains Mono",
        "DejaVu Sans Mono",
        "Liberation Mono",
        "Noto Sans Mono",
        "Source Code Pro",
        "Fira Code",
        "Cascadia Mono",
        "Hack",
        "Iosevka",
    ];
    #[cfg(target_os = "windows")]
    return &[
        "Consolas",
        "Cascadia Mono",
        "JetBrains Mono",
        "DejaVu Sans Mono",
        "Liberation Mono",
        "Noto Sans Mono",
        "Source Code Pro",
        "Fira Code",
        "Hack",
        "Iosevka",
    ];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return &[
        "JetBrainsMono Nerd Font",
        "JetBrainsMono NF",
        "JetBrains Mono",
        "DejaVu Sans Mono",
        "Liberation Mono",
        "Noto Sans Mono",
        "Ubuntu Mono",
        "Adwaita Mono",
        "Source Code Pro",
        "Fira Code",
        "Cascadia Mono",
        "Hack",
        "Iosevka",
    ];
}

/// Name heuristic: does this family name look like a terminal font? Only used
/// to shortlist scan candidates; the advance probe verifies for real.
fn looks_fixed_pitch(name: &str) -> bool {
    let n = name.to_lowercase();
    [
        "mono", "code", "console", "hack", "iosevka", "menlo", "consolas", "courier", "nerd",
        "term",
    ]
    .iter()
    .any(|k| n.contains(k))
}

/// True when every advance width is within ±2% of the mean — i.e. the face is
/// actually fixed-pitch, not a proportional face gpui silently substituted.
fn advances_uniform(widths: &[f32]) -> bool {
    if widths.is_empty() || widths.iter().any(|w| *w <= 0.0) {
        return false;
    }
    let mean = widths.iter().sum::<f32>() / widths.len() as f32;
    widths.iter().all(|w| (w - mean).abs() <= 0.02 * mean)
}

/// Candidate order: the requested family first, then platform defaults.
/// Deduped; blank entries dropped.
fn candidate_families(requested: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |f: &str| {
        let f = f.trim();
        if !f.is_empty() && !out.iter().any(|o| o == f) {
            out.push(f.to_string());
        }
    };
    push(requested);
    for d in platform_defaults() {
        push(d);
    }
    out
}

fn probe_font(family: &str) -> Font {
    Font {
        family: SharedString::from(family),
        features: FontFeatures::disable_ligatures(),
        fallbacks: None,
        weight: FontWeight::NORMAL,
        style: FontStyle::Normal,
    }
}

/// Is `family` installed, resolved exactly (no silent substitution), and
/// fixed-pitch by advance probe?
fn family_usable(ts: &TextSystem, family: &str) -> bool {
    if !ts.all_font_names().iter().any(|n| n == family) {
        return false;
    }
    let id = ts.resolve_font(&probe_font(family));
    let Some(resolved) = ts.get_font_for_id(id) else {
        return false;
    };
    if resolved.family.as_ref() != family {
        return false;
    }
    let widths: Vec<f32> = PROBE_CHARS
        .iter()
        .filter_map(|c| {
            ts.advance(id, px(PROBE_SIZE), *c)
                .ok()
                .map(|s| f32::from(s.width))
        })
        .collect();
    widths.len() == PROBE_CHARS.len() && advances_uniform(&widths)
}

fn resolve_uncached(ts: &TextSystem, requested: &str) -> String {
    for cand in candidate_families(requested) {
        if family_usable(ts, &cand) {
            if !requested.trim().is_empty() && requested.trim() != cand {
                log::warn!(
                    "terminal font \"{requested}\" is not an installed fixed-pitch family; using \"{cand}\""
                );
            }
            return cand;
        }
    }
    // Last resort: scan every installed face for a verifiably fixed-pitch one.
    let mut names = ts.all_font_names();
    names.sort();
    names.dedup();
    for name in names {
        if looks_fixed_pitch(&name) && family_usable(ts, &name) {
            log::warn!(
                "terminal font fallback scan selected \"{name}\" (requested \"{requested}\")"
            );
            return name;
        }
    }
    // Nothing verifiable: hand back the request and let gpui do what it can.
    if !requested.trim().is_empty() {
        requested.trim().to_string()
    } else {
        platform_defaults()[0].to_string()
    }
}

static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

/// The family the terminal should actually render with: `requested` (the user
/// setting; `""` = platform default) when it is installed and verifiably
/// fixed-pitch, otherwise the best installed monospace face. Cached — font
/// enumeration + probing on every layout would be wasteful.
pub fn resolve_mono_family(ts: &TextSystem, requested: &str) -> SharedString {
    let key = requested.trim().to_string();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(hit) = cache.lock().unwrap().get(&key) {
        return SharedString::from(hit.clone());
    }
    let resolved = resolve_uncached(ts, &key);
    cache.lock().unwrap().insert(key, resolved.clone());
    SharedString::from(resolved)
}

/// Per-glyph fallback chain: installed monospace-looking faces only, so a
/// missing glyph lands in another terminal font, never a proportional UI face.
pub fn fallback_families(ts: &TextSystem) -> Vec<String> {
    let mut names = ts.all_font_names();
    names.sort();
    names.dedup();
    names.into_iter().filter(|n| looks_fixed_pitch(n)).collect()
}

#[cfg(test)]
mod tests {
    use super::{advances_uniform, candidate_families, looks_fixed_pitch, platform_defaults};

    #[test]
    fn requested_family_comes_first_and_list_is_deduped() {
        let c = candidate_families("JetBrains Mono");
        assert_eq!(c[0], "JetBrains Mono");
        assert!(c.windows(2).all(|w| w[0] != w[1]));
    }

    #[test]
    fn blank_request_is_exactly_the_platform_defaults() {
        let c = candidate_families("");
        let expected: Vec<String> = platform_defaults().iter().map(|s| s.to_string()).collect();
        assert_eq!(c, expected);
    }

    #[test]
    fn name_heuristic_matches_terminal_fonts_only() {
        assert!(looks_fixed_pitch("JetBrainsMono Nerd Font"));
        assert!(looks_fixed_pitch("DejaVu Sans Mono"));
        assert!(looks_fixed_pitch("Fira Code"));
        assert!(!looks_fixed_pitch("Adwaita Sans"));
        assert!(!looks_fixed_pitch("Inter"));
    }

    #[test]
    fn uniform_advances_are_fixed_pitch() {
        assert!(advances_uniform(&[8.0, 8.0, 8.0, 8.0]));
        assert!(advances_uniform(&[8.0, 8.1, 7.95, 8.05]));
        assert!(!advances_uniform(&[3.4, 12.3, 8.0, 9.1]));
        assert!(!advances_uniform(&[]));
        assert!(!advances_uniform(&[0.0, 8.0]));
    }
}
