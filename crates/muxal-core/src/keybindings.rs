//! Stable shortcut defaults and pure override decisions. No UI or I/O.

use crate::KeyBindingCfg;

pub const DEFAULT_KEYBINDINGS: &[(&str, &str, Option<&str>)] = &[
    ("NewPane", "ctrl-shift-t", None),
    // Keep global while a terminal is focused (see KEEP_GLOBAL_WHILE_TERMINAL).
    ("NewTab", "ctrl-t", None),
    ("TabNext", "ctrl-tab", None),
    ("TabPrev", "ctrl-shift-tab", None),
    ("SplitRight", "ctrl-shift-l", None),
    ("SplitDown", "ctrl-shift-j", None),
    ("ClosePane", "ctrl-shift-w", None),
    ("FocusNext", "ctrl-shift-]", None),
    ("FocusPrev", "ctrl-shift-[", None),
    ("FocusLeft", "ctrl-alt-left", None),
    ("FocusRight", "ctrl-alt-right", None),
    ("FocusUp", "ctrl-alt-up", None),
    ("FocusDown", "ctrl-alt-down", None),
    ("ZoomIn", "ctrl-shift-up", None),
    ("ZoomOut", "ctrl-shift-down", None),
    ("ToggleSidebar", "ctrl-shift-b", None),
    ("ToggleDashboard", "ctrl-shift-e", None),
    ("ToggleSettings", "ctrl-shift-,", None),
    // Ctrl+P also opens it when no terminal is focused (see install_keybindings),
    // leaving Ctrl+P free for the terminal agent (e.g. opencode's commands).
    ("GlobalSearch", "ctrl-shift-p", None),
    ("FindInProject", "ctrl-shift-f", None),
    // Same key as FindInProject but scoped to a focused terminal (searches it).
    ("SearchTerminal", "ctrl-shift-f", Some("Terminal")),
    ("ClearTerminal", "ctrl-shift-k", None),
    ("FocusAttention", "ctrl-shift-a", None),
    ("ShowKeys", "ctrl-shift-/", None),
    ("ToggleBroadcast", "ctrl-shift-i", None),
    // Toggle the toolbar's "new agents get a git worktree" switch.
    ("ToggleWorktree", "ctrl-shift-g", None),
    // OS fullscreen; the sidebar hides until revealed or fullscreen exits.
    ("ToggleFullScreen", "f11", None),
    ("ToggleDevConsole", "f12", None),
    // Editor save — auto !Terminal so Claude stash (Ctrl+S) reaches the PTY.
    ("SaveFile", "ctrl-s", None),
    ("SaveFileAs", "ctrl-shift-s", None),
    ("JumpToTab1", "alt-1", None),
    ("JumpToTab2", "alt-2", None),
    ("JumpToTab3", "alt-3", None),
    ("JumpToTab4", "alt-4", None),
    ("JumpToTab5", "alt-5", None),
    ("JumpToTab6", "alt-6", None),
    ("JumpToProject1", "ctrl-1", None),
    ("JumpToProject2", "ctrl-2", None),
    ("JumpToProject3", "ctrl-3", None),
    ("JumpToProject4", "ctrl-4", None),
    ("JumpToProject5", "ctrl-5", None),
    ("JumpToProject6", "ctrl-6", None),
    ("JumpToProject7", "ctrl-7", None),
    ("JumpToProject8", "ctrl-8", None),
    ("JumpToProject9", "ctrl-9", None),
    // New pane running the Nth agent preset (in the preset-list order; drag
    // the list in Settings → Agents to change it).
    ("NewAgent1", "ctrl-alt-1", None),
    ("NewAgent2", "ctrl-alt-2", None),
    ("NewAgent3", "ctrl-alt-3", None),
    ("NewAgent4", "ctrl-alt-4", None),
    ("NewAgent5", "ctrl-alt-5", None),
    ("NewAgent6", "ctrl-alt-6", None),
    // Cycle the pinned projects (top-bar slots), wrapping.
    ("CycleProjects", "alt-p", None),
    ("CycleProjectsPrev", "alt-shift-p", None),
];

/// Compare spelling without treating harmless whitespace/case as customization.
pub fn modified(value: &str, default: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && !value.eq_ignore_ascii_case(default.trim())
}

/// Preserve unknown actions, and save only actual overrides for known actions.
pub fn apply_draft(existing: &[KeyBindingCfg], draft: &[(String, String)]) -> Vec<KeyBindingCfg> {
    let mut result: Vec<_> = existing
        .iter()
        .filter(|entry| {
            !DEFAULT_KEYBINDINGS
                .iter()
                .any(|(id, _, _)| *id == entry.action)
        })
        .cloned()
        .collect();
    for (action, value) in draft {
        if let Some((_, default, _)) = DEFAULT_KEYBINDINGS.iter().find(|(id, _, _)| *id == action)
            && modified(value, default)
        {
            result.push(KeyBindingCfg {
                action: action.clone(),
                keystroke: value.trim().into(),
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_KEYBINDINGS, apply_draft, modified};
    use crate::KeyBindingCfg;
    #[test]
    fn defaults_remain_unique_and_scoped_duplicates_remain_valid() {
        let mut ids = std::collections::HashSet::new();
        for (id, _, _) in DEFAULT_KEYBINDINGS {
            assert!(ids.insert(id));
        }
        let project = DEFAULT_KEYBINDINGS
            .iter()
            .find(|(id, _, _)| *id == "FindInProject")
            .unwrap();
        let terminal = DEFAULT_KEYBINDINGS
            .iter()
            .find(|(id, _, _)| *id == "SearchTerminal")
            .unwrap();
        assert_eq!(project.1, terminal.1);
        assert_ne!(project.2, terminal.2);
    }
    #[test]
    fn reset_and_explicit_defaults_preserve_unknown_overrides() {
        let existing = vec![KeyBindingCfg {
            action: "FutureAction".into(),
            keystroke: "alt-k".into(),
        }];
        let result = apply_draft(
            &existing,
            &[
                ("NewPane".into(), " CTRL-SHIFT-T ".into()),
                ("NewTab".into(), "alt-t".into()),
                ("SplitDown".into(), String::new()),
            ],
        );
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].action, "FutureAction");
        assert_eq!(result[1].action, "NewTab");
        assert!(!modified("", "ctrl-t"));
    }
}
