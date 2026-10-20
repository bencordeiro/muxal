//! State for the in-app settings page.
//!
//! The page edits one preset / project at a time, reusing a fixed set of
//! [`InputState`] widgets that are repopulated on selection and read back on
//! Save. The rendering + handlers live as `MuxalApp` methods in `app.rs`.

use crate::i18n::t;
use gpui::*;
use gpui_component::input::InputState;
use gpui_component::select::SelectState;
use muxal_core::{EnvVar, InjectionMode};
use uuid::Uuid;

/// Configurable actions: `(action, default keystroke, key context)`. The names
/// are matched in `app::keybinding_for` to the corresponding gpui actions; the
/// context (e.g. `Some("Terminal")`) scopes a binding to a focus region.
///
/// **Terminal vs app chords.** Plain `ctrl-<letter>` is claimed by agents as C0
/// (Claude Ctrl+S stash, shells Ctrl+R, …). `install_keybindings` auto-scopes
/// those to `!Terminal` unless the action is in [`KEEP_GLOBAL_WHILE_TERMINAL`].
/// Prefer `ctrl-shift-*` for muxal chrome.
pub use muxal_core::keybindings::DEFAULT_KEYBINDINGS;

/// Actions whose plain `ctrl-<letter>` binding stays live while a terminal is
/// focused. Everything else with that shape is scoped to `!Terminal`.
pub const KEEP_GLOBAL_WHILE_TERMINAL: &[&str] = &[
    "NewTab", // ctrl-t — open a tab without leaving the agent
];

/// Which settings section is shown.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Appearance,
    Editor,
    Behavior,
    Agents,
    Runners,
    Snippets,
    Loops,
    Projects,
    Keybindings,
    Music,
}

/// Editable widgets + selection state for the settings page.
pub struct SettingsUi {
    pub section: SettingsSection,

    // Agent preset editor.
    pub selected_preset: Option<usize>,
    pub p_injection: InjectionMode,
    pub p_name: Entity<InputState>,
    pub p_program: Entity<InputState>,
    pub p_model: Entity<InputState>,
    pub p_model_flag: Entity<InputState>,
    pub p_effort: Entity<InputState>,
    pub p_effort_flag: Entity<InputState>,
    pub p_args: Entity<InputState>,
    pub p_prompt: Entity<InputState>,
    pub p_inj_flag: Entity<InputState>,
    pub p_env: Entity<InputState>,
    pub p_working_markers: Entity<InputState>,
    pub p_blocked_markers: Entity<InputState>,
    pub p_startup_delay: Entity<InputState>,

    // Runner editor.
    pub selected_runner: Option<usize>,
    pub r_preset_id: Option<Uuid>,
    pub r_presses: u8,
    pub r_name: Entity<InputState>,
    pub r_prompt: Entity<InputState>,

    // Snippet editor.
    pub selected_snippet: Option<usize>,
    pub sn_submit: bool,
    pub sn_name: Entity<InputState>,
    pub sn_text: Entity<InputState>,

    // Loop editor.
    pub selected_loop: Option<usize>,
    pub l_preset_id: Option<Uuid>,
    pub l_project_id: Option<Uuid>,
    pub l_presses: u8,
    /// Schedule kind: 0 = every N minutes, 1 = every N hours, 2 = daily at.
    pub l_sched_kind: u8,
    pub l_exit: bool,
    pub l_enabled: bool,
    pub l_name: Entity<InputState>,
    pub l_prompt: Entity<InputState>,
    pub l_interval: Entity<InputState>,
    pub l_hour: Entity<InputState>,
    pub l_minute: Entity<InputState>,

    // Project editor.
    pub selected_project: Option<Uuid>,
    pub proj_name: Entity<InputState>,

    // Appearance.
    pub theme_picker: Entity<SelectState<Vec<SharedString>>>,
    pub language_picker: Entity<SelectState<Vec<SharedString>>>,
    pub font_family: Entity<InputState>,
    /// Ambience (cool/warm tint) slider.
    pub ambience_slider: Entity<gpui_component::slider::SliderState>,
    pub music_volume_slider: Entity<gpui_component::slider::SliderState>,

    // Editor.
    pub editor_font_family: Entity<InputState>,

    // Keybindings (action name -> keystroke input).
    pub keybinds: Vec<(String, Entity<InputState>)>,
    /// Comma/space-separated chords passed through to a focused terminal.
    pub passthrough_keys: Entity<InputState>,
    pub keys_modified_only: bool,
    pub keys_numbered_open: bool,
    pub keys_passthrough_open: bool,
    pub key_error: Option<String>,
}

impl SettingsUi {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            section: SettingsSection::Appearance,
            selected_preset: None,
            p_injection: InjectionMode::None,
            p_name: cx.new(|cx| InputState::new(window, cx).placeholder(t("Name"))),
            p_program: cx
                .new(|cx| InputState::new(window, cx).placeholder(t("Program (blank = shell)"))),
            p_model: cx.new(|cx| InputState::new(window, cx).placeholder(t("Model (optional)"))),
            p_model_flag: cx.new(|cx| InputState::new(window, cx).placeholder(t("--model"))),
            p_effort: cx.new(|cx| InputState::new(window, cx).placeholder(t("Effort (optional)"))),
            p_effort_flag: cx.new(|cx| InputState::new(window, cx).placeholder(t("Effort flag"))),
            p_args: cx.new(|cx| {
                InputState::new(window, cx).placeholder(t("Extra args (quotes group words)"))
            }),
            p_prompt: cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .placeholder(t("System prompt"))
            }),
            p_inj_flag: cx
                .new(|cx| InputState::new(window, cx).placeholder(t("--append-system-prompt"))),
            p_env: cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .placeholder(t("KEY=VALUE per line"))
            }),
            p_working_markers: cx.new(|cx| {
                InputState::new(window, cx).placeholder(t("comma-separated; blank = default"))
            }),
            p_blocked_markers: cx.new(|cx| {
                InputState::new(window, cx).placeholder(t("comma-separated; blank = default"))
            }),
            p_startup_delay: cx
                .new(|cx| InputState::new(window, cx).placeholder(t("0 = auto (wait for quiet)"))),
            selected_runner: None,
            r_preset_id: None,
            r_presses: 3,
            r_name: cx.new(|cx| InputState::new(window, cx).placeholder(t("Name"))),
            r_prompt: cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .placeholder(t("Task prompt — use {{input}} for the run-time details"))
            }),
            selected_snippet: None,
            sn_submit: false,
            sn_name: cx.new(|cx| InputState::new(window, cx).placeholder(t("Name"))),
            sn_text: cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .placeholder(t("Text typed into the pane"))
            }),
            selected_loop: None,
            l_preset_id: None,
            l_project_id: None,
            l_presses: 0,
            l_sched_kind: 1,
            l_exit: false,
            l_enabled: true,
            l_name: cx.new(|cx| InputState::new(window, cx).placeholder(t("Name"))),
            l_prompt: cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .placeholder(t("Prompt to run each time"))
            }),
            l_interval: cx.new(|cx| InputState::new(window, cx).placeholder("1")),
            l_hour: cx.new(|cx| InputState::new(window, cx).placeholder("9")),
            l_minute: cx.new(|cx| InputState::new(window, cx).placeholder("00")),
            selected_project: None,
            proj_name: cx.new(|cx| InputState::new(window, cx).placeholder(t("Project name"))),
            theme_picker: cx
                .new(|cx| SelectState::new(crate::theme::theme_names(cx), None, window, cx)),
            language_picker: cx.new(|cx| {
                let languages = crate::i18n::available_languages()
                    .iter()
                    .map(|entry| SharedString::from(entry.1))
                    .collect::<Vec<_>>();
                SelectState::new(languages, None, window, cx)
            }),
            font_family: {
                // Show the *detected* installed default, not a hardcoded family
                // that may not exist on this machine.
                let detected = muxal_terminal::resolve_mono_family(cx.text_system(), "");
                cx.new(|cx| InputState::new(window, cx).placeholder(detected))
            },
            ambience_slider: cx.new(|_| {
                gpui_component::slider::SliderState::new()
                    .min(-f32::from(muxal_core::ambience::AMBIENCE_MAX))
                    .max(f32::from(muxal_core::ambience::AMBIENCE_MAX))
                    .step(1.0)
            }),
            music_volume_slider: cx.new(|_| {
                gpui_component::slider::SliderState::new()
                    .min(0.0)
                    .max(100.0)
                    .step(1.0)
            }),
            editor_font_family: cx
                .new(|cx| InputState::new(window, cx).placeholder(t("theme monospace"))),
            keys_modified_only: false,
            keys_numbered_open: false,
            keys_passthrough_open: false,
            key_error: None,
            keybinds: DEFAULT_KEYBINDINGS
                .iter()
                .map(|(name, default, _ctx)| {
                    (
                        name.to_string(),
                        cx.new(|cx| InputState::new(window, cx).default_value(*default)),
                    )
                })
                .collect(),
            passthrough_keys: cx
                .new(|cx| InputState::new(window, cx).placeholder(t("ctrl-p, ctrl-t"))),
        }
    }
}

/// Parse a `KEY=VALUE`-per-line block into env vars (blank lines ignored).
pub fn parse_env(text: &str) -> Vec<EnvVar> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let (k, v) = line.split_once('=')?;
            Some(EnvVar {
                key: k.trim().to_string(),
                value: v.trim().to_string(),
            })
        })
        .collect()
}

/// Render env vars back to a `KEY=VALUE`-per-line block for editing.
pub fn format_env(env: &[EnvVar]) -> String {
    env.iter()
        .map(|e| format!("{}={}", e.key, e.value))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Split an extra-args string into shell words — quote-aware
/// ([`muxal_core::split_words`], shared with the iOS companion), so
/// `--append-system-prompt "be terse"` stays one argument. Unbalanced quotes
/// degrade to a plain whitespace split (a preset must never fail to save);
/// `save_preset` warns via the feed in that case.
pub fn parse_args(text: &str) -> Vec<String> {
    muxal_core::split_words(text)
        .unwrap_or_else(|| text.split_whitespace().map(|s| s.to_string()).collect())
}

/// Split a status-marker string on commas (markers contain spaces), trimming
/// each and dropping blanks.
pub fn parse_markers(text: &str) -> Vec<String> {
    text.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}
