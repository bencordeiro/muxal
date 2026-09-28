//! App ambience: a warm/cool color cast blended over the whole UI.
//!
//! The slider is a purely paint-level effect (no OS window transparency), so
//! it behaves identically on every platform. The cast is deliberately modest:
//! at maximum the UI shifts at most [`AMBIENCE_BLEND`] toward warm or cool,
//! which keeps text contrast and semantic colors (red = error, ANSI hues)
//! intact.

/// Maximum slider magnitude (cool -50 .. +50 warm). 0 = the theme as authored.
pub const AMBIENCE_MAX: i8 = 50;

/// Blend strength at full slider: 15% toward the warm or cool pole.
pub const AMBIENCE_BLEND: f32 = 0.15;

/// Signed blend strength for a slider value: -0.15 (cool) .. +0.15 (warm).
pub fn ambience_blend(cast: i8) -> f32 {
    f32::from(cast.clamp(-AMBIENCE_MAX, AMBIENCE_MAX)) / f32::from(AMBIENCE_MAX) * AMBIENCE_BLEND
}

#[cfg(test)]
mod tests {
    use super::{AMBIENCE_BLEND, AMBIENCE_MAX, ambience_blend};

    #[test]
    fn zero_is_the_theme_as_authored() {
        assert_eq!(ambience_blend(0), 0.0);
    }

    #[test]
    fn extremes_stay_within_the_blend_cap() {
        assert_eq!(ambience_blend(AMBIENCE_MAX), AMBIENCE_BLEND);
        assert_eq!(ambience_blend(-AMBIENCE_MAX), -AMBIENCE_BLEND);
        assert_eq!(ambience_blend(127), AMBIENCE_BLEND); // clamped
        assert_eq!(ambience_blend(-128), -AMBIENCE_BLEND); // clamped
    }

    #[test]
    fn old_settings_without_the_field_default_to_neutral() {
        let s: crate::Settings = serde_json::from_str("{}").expect("defaults fill in");
        assert_eq!(s.ambience, 0);
        assert_eq!(ambience_blend(s.ambience), 0.0);
    }
}
