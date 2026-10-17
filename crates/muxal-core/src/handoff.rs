//! User-requested handoffs. File access and UI belong to the store and app.
use crate::{AgentPreset, InjectionMode, Instance, Uuid};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MAX_HANDOFF_BYTES: usize = 256 * 1024;

// Startup input is normalized to one line by the launch resolver. JSON-escape
// spaces too, so paths containing repeated spaces survive that normalization.
fn path_reference(path: &Path) -> String {
    serde_json::to_string(&path.to_string_lossy())
        .expect("string serialization")
        .replace(' ', "\\u0020")
}

/// A single outstanding request, tied to a source conversation and directory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HandoffRequest {
    pub id: Uuid,
    pub source: Uuid,
    pub project: Uuid,
    pub cwd: PathBuf,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub direction: String,
    /// Captured/reviewed text lives in the workspace document, not loose files.
    #[serde(default)]
    pub summary: Option<String>,
}

impl HandoffRequest {
    pub fn new(source: &Instance, cwd: PathBuf, direction: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            source: source.id,
            project: source.project_id,
            cwd,
            session_id: source.session_id.clone(),
            direction,
            summary: None,
        }
    }

    pub fn matches_source(&self, source: &Instance, cwd: &Path) -> bool {
        self.source == source.id
            && self.project == source.project_id
            && self.cwd == cwd
            && self
                .session_id
                .as_ref()
                .is_none_or(|id| source.session_id.as_ref() == Some(id))
    }

    pub fn completion_marker(&self) -> String {
        format!("<!-- muxal-handoff:{} -->", self.id)
    }

    /// The final marker rejects partial writes and documents from other requests.
    pub fn document(&self, raw: &str) -> Option<String> {
        if raw.len() > MAX_HANDOFF_BYTES {
            return None;
        }
        let text = raw
            .trim_end()
            .strip_suffix(&self.completion_marker())?
            .trim();
        reviewed_text(text)
    }

    pub fn preparation_prompt(&self, path: &Path) -> String {
        let temporary = path_reference(&path.with_extension("md.tmp"));
        let path = path_reference(path);
        format!(
            "The user requested a handoff to a fresh agent. Summarize this conversation; do not continue implementation. Write a concise Markdown handoff to the JSON-quoted path {path}. Include the goal, completed work, decisions and constraints, relevant files, tests actually run and their results, outstanding problems, and concrete next steps. Distinguish facts from assumptions. Include any uncommitted changes and do not claim work you have not verified. Direction for the next agent: {}. Write the entire document to the JSON-quoted temporary path {temporary} and rename it to the requested path only when complete. Muxal will capture the text and remove these transfer files automatically. The LAST line must be exactly {}. Keep the document under 256 KiB. Do not start another agent or change the project just to prepare this summary.",
            if self.direction.trim().is_empty() {
                "continue the current task"
            } else {
                self.direction.trim()
            },
            self.completion_marker(),
        )
    }

    /// Starts from the recipient's preset, never a clone of the source session.
    pub fn recipient(&self, preset: &AgentPreset, summary: &str) -> Instance {
        let mut instance = Instance::from_preset(self.project, preset);
        let opening = format!(
            "The user intentionally handed this task to you in a fresh conversation. The complete reviewed handoff is included below. Use it as context, verify the current files and working tree, and follow the user's constraints. {}\n\n--- Reviewed handoff ---\n{summary}",
            if self.direction.trim().is_empty() {
                "Continue from its next steps."
            } else {
                self.direction.trim()
            },
        );
        instance.system_prompt = Some(match instance.system_prompt.take() {
            Some(prompt) if !prompt.trim().is_empty() => format!("{prompt}\n\n{opening}"),
            _ => opening,
        });
        instance.injection = InjectionMode::TypeIn;
        instance.handoff_id = Some(self.id);
        instance.handoff_inline = true;
        instance
    }
}

/// Transfer files are needed only until capture, except for pre-inline panes
/// whose opening prompt may still refer to a file. Those are retained until close.
pub fn retained_transfer_ids(workspace: &crate::Workspace) -> std::collections::HashSet<Uuid> {
    workspace
        .handoffs
        .iter()
        .filter(|r| r.summary.is_none())
        .map(|r| r.id)
        .chain(
            workspace
                .instances
                .iter()
                .filter(|i| !i.handoff_inline)
                .filter_map(|i| i.handoff_id),
        )
        .collect()
}

/// Validate plain Markdown before persisting or sending it through a terminal.
/// Keep internal whitespace (including code indentation and blank lines) intact.
pub fn reviewed_text(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()
        && text.len() <= MAX_HANDOFF_BYTES
        && !text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')))
    .then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{HandoffRequest, path_reference};
    use crate::{AgentPreset, Instance, Uuid, resolve_launch_for_session};
    use std::path::Path;

    #[test]
    fn path_survives_startup_whitespace_normalization() {
        let path = Path::new("/data/with  spaces/quoted\"dir/handoff.md");
        let reference = path_reference(path);
        let normalized = reference.split_whitespace().collect::<Vec<_>>().join(" ");
        let decoded: String = serde_json::from_str(&normalized).unwrap();
        assert_eq!(decoded, path.to_str().unwrap());
    }

    #[test]
    fn readiness_requires_the_complete_matching_document() {
        let source = Instance::shell(Uuid::new_v4());
        let request = HandoffRequest::new(&source, "/repo".into(), String::new());
        assert!(request.document("unfinished").is_none());
        assert!(request.document(&request.completion_marker()).is_none());
        let other = HandoffRequest::new(&source, "/repo".into(), String::new());
        assert!(
            request
                .document(&format!("summary\n{}", other.completion_marker()))
                .is_none()
        );
        let raw = format!(
            "# Task\n\nNext: run tests.\n{}\n",
            request.completion_marker()
        );
        assert_eq!(
            request.document(&raw).as_deref(),
            Some("# Task\n\nNext: run tests.")
        );
    }

    #[test]
    fn rejects_a_switched_conversation_or_directory() {
        let mut source = Instance::shell(Uuid::new_v4());
        source.session_id = Some("original".into());
        let request = HandoffRequest::new(&source, "/repo/worktree".into(), String::new());
        assert!(request.matches_source(&source, Path::new("/repo/worktree")));
        assert!(!request.matches_source(&source, Path::new("/repo")));
        source.session_id = Some("different".into());
        assert!(!request.matches_source(&source, Path::new("/repo/worktree")));
    }

    #[test]
    fn closing_source_forgets_only_its_pending_request() {
        let source = Instance::shell(Uuid::new_v4());
        let other = Instance::shell(source.project_id);
        let request = HandoffRequest::new(&source, "/repo".into(), String::new());
        let other_request = HandoffRequest::new(&other, "/repo".into(), String::new());
        let other_id = other_request.id;
        let mut ws = crate::Workspace {
            instances: vec![source.clone(), other],
            handoffs: vec![request, other_request],
            ..crate::Workspace::default()
        };
        ws.remove_instance_meta(source.id);
        assert_eq!(ws.handoffs.len(), 1);
        assert_eq!(ws.handoffs[0].id, other_id);
    }

    #[test]
    fn cleanup_retains_only_pending_transfers_and_legacy_file_recipients() {
        let mut legacy = Instance::shell(Uuid::new_v4());
        legacy.handoff_id = Some(Uuid::new_v4());
        let mut inline = legacy.clone();
        inline.id = Uuid::new_v4();
        inline.handoff_id = Some(Uuid::new_v4());
        inline.handoff_inline = true;
        let pending = HandoffRequest::new(&legacy, "/repo".into(), String::new());
        let mut ready = HandoffRequest::new(&inline, "/repo".into(), String::new());
        ready.summary = Some("Captured".into());
        let expected = std::collections::HashSet::from([pending.id, legacy.handoff_id.unwrap()]);
        let ws = crate::Workspace {
            instances: vec![legacy, inline],
            handoffs: vec![pending, ready],
            ..crate::Workspace::default()
        };
        assert_eq!(super::retained_transfer_ids(&ws), expected);
    }

    #[test]
    fn old_workspaces_and_settings_default_to_no_handoffs() {
        let ws: crate::Workspace =
            serde_json::from_str(r#"{"projects":[],"active_project":null,"instances":[]}"#)
                .unwrap();
        assert!(ws.handoffs.is_empty());
        let settings: crate::Settings = serde_json::from_str("{}").unwrap();
        assert!(!settings.handoffs_enabled);
    }

    #[test]
    fn restore_and_duplicate_preserve_one_shot_semantics() {
        let preset = AgentPreset::claude();
        let source = Instance::from_preset(Uuid::new_v4(), &preset);
        let request = HandoffRequest::new(&source, "/repo".into(), String::new());
        let recipient = request.recipient(&preset, "# Task\nContinue with the tests.");
        let mut ws = crate::Workspace {
            instances: vec![recipient],
            ..crate::Workspace::default()
        };
        crate::sync_agent_injection_modes(&mut ws, &[preset]);
        assert!(
            resolve_launch_for_session(&ws.instances[0], false)
                .startup_input
                .is_some()
        );
        let mut duplicate = ws.instances[0].clone();
        duplicate.reset_conversation_for_duplicate();
        assert!(duplicate.handoff_id.is_none());
        assert!(
            resolve_launch_for_session(&duplicate, false)
                .startup_input
                .is_none()
        );
    }

    #[test]
    fn full_reviewed_text_is_in_the_prompt_with_formatting_and_no_file_reference() {
        let preset = AgentPreset::pi();
        let source = Instance::from_preset(Uuid::new_v4(), &preset);
        let request = HandoffRequest::new(&source, "/repo".into(), String::new());
        let summary = "# Context\n\n```csharp\nif (ready) {\n    Run(\"two  spaces\");\n}\n```\n\nNext: verify the result.";
        let recipient = request.recipient(&preset, summary);
        let launch = resolve_launch_for_session(&recipient, false);
        let input = launch.startup_input.unwrap();
        assert!(input.ends_with(summary));
        assert!(!input.contains("reviewed.md"));
        assert!(!input.contains("JSON-quoted path"));
        assert!(recipient.handoff_inline);
    }

    #[test]
    fn review_rejects_control_characters_and_oversized_content() {
        use super::{MAX_HANDOFF_BYTES, reviewed_text};
        assert!(reviewed_text(" ").is_none());
        assert!(reviewed_text("do\x03not submit").is_none());
        assert!(reviewed_text("escape\x1b[201~").is_none());
        assert!(reviewed_text(&"x".repeat(MAX_HANDOFF_BYTES + 1)).is_none());
        assert_eq!(
            reviewed_text("# Code\n\tRun();").as_deref(),
            Some("# Code\n\tRun();")
        );
    }

    #[test]
    fn recipient_is_fresh_and_startup_is_one_shot_even_without_resume() {
        let preset = AgentPreset::claude();
        let mut source = Instance::from_preset(Uuid::new_v4(), &preset);
        source.session_id = Some(Uuid::new_v4().to_string());
        source.session_started = true;
        let request = HandoffRequest::new(&source, "/repo".into(), "Review only.".into());
        let mut recipient = request.recipient(&preset, "# Task\nContinue with the tests.");
        assert_ne!(recipient.id, source.id);
        assert!(recipient.session_id.is_none());
        assert!(!recipient.session_started);
        let launch = resolve_launch_for_session(&recipient, false);
        assert!(launch.submit);
        assert!(launch.startup_input.unwrap().contains("Review only."));
        recipient.consume_handoff_prompt();
        assert!(
            resolve_launch_for_session(&recipient, false)
                .startup_input
                .is_none()
        );
        assert!(!recipient.auto_submit);
        assert_eq!(recipient.handoff_id, Some(request.id));
    }
}
