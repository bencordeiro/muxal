//! Short-lived handoff transfer files and migration of earlier saved reviews.
use anyhow::{Context, Result};
use muxal_core::Uuid;
use muxal_core::handoff::{HandoffRequest, MAX_HANDOFF_BYTES};
use std::collections::HashSet;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

pub fn directory(workspace: Uuid) -> Option<PathBuf> {
    Some(
        crate::workspace_doc_path(workspace)?
            .parent()?
            .join("handoffs"),
    )
}

pub fn source_path(dir: &Path, request: &HandoffRequest) -> PathBuf {
    dir.join(format!("{}.md", request.id))
}

pub fn reviewed_path(dir: &Path, request: &HandoffRequest) -> PathBuf {
    dir.join(format!("{}.reviewed.md", request.id))
}

fn read_document(path: &Path, request: &HandoffRequest) -> Result<String> {
    let file = std::fs::File::open(path).context("opening handoff")?;
    let mut raw = String::new();
    file.take(MAX_HANDOFF_BYTES as u64 + 1)
        .read_to_string(&mut raw)?;
    request
        .document(&raw)
        .context("handoff is incomplete or too large")
}

pub fn read(dir: &Path, request: &HandoffRequest) -> Result<String> {
    if let Some(text) = &request.summary {
        return Ok(text.clone());
    }
    let reviewed = reviewed_path(dir, request);
    // A damaged saved review must not silently fall back to the agent's draft.
    let path = if reviewed.try_exists()? {
        reviewed
    } else {
        source_path(dir, request)
    };
    read_document(&path, request)
}

/// A snapshot of a file we own. Cleanup only touches this snapshot, so a new
/// request created while a worker runs cannot have its transfer file removed.
pub struct TransferFile {
    pub request: Uuid,
    path: PathBuf,
}

pub fn scan(dir: &Path) -> Result<Vec<TransferFile>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some((id, suffix)) = name.to_str().and_then(|name| name.split_at_checked(36)) else {
            continue;
        };
        let Ok(request) = Uuid::parse_str(id) else {
            continue;
        };
        if !matches!(
            suffix,
            ".md" | ".md.tmp" | ".reviewed.md" | ".reviewed.md.tmp"
        ) {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_file() || kind.is_symlink() {
            files.push(TransferFile {
                request,
                path: entry.path(),
            });
        }
    }
    Ok(files)
}

/// Unlink only recognized transfer files. Unknown files and directories stay put.
/// Keep the empty directory: a pending agent may be about to create its file.
pub fn remove_transfers(files: Vec<TransferFile>, retained: &HashSet<Uuid>) -> Result<()> {
    for file in files {
        if retained.contains(&file.request) {
            continue;
        }
        match std::fs::remove_file(&file.path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("removing {}", file.path.display()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{read, remove_transfers, scan, source_path};
    use muxal_core::{Instance, Uuid, Workspace, handoff::HandoffRequest};
    use std::collections::HashSet;

    fn scratch() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("muxal-handoff-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn captured_review_survives_restart_without_any_handoff_file() {
        let dir = scratch();
        let source = Instance::shell(Uuid::new_v4());
        let mut request = HandoffRequest::new(&source, "/repo".into(), String::new());
        let path = source_path(&dir, &request);
        std::fs::write(&path, "half a summary").unwrap();
        assert!(read(&dir, &request).is_err());
        std::fs::write(&path, format!("original\n{}", request.completion_marker())).unwrap();
        request.summary = Some(read(&dir, &request).unwrap());
        // Review edits live only in the normal workspace document.
        request.summary = Some("# Edited\n\n```\n    code\n```".into());
        let ws = Workspace {
            handoffs: vec![request],
            ..Workspace::default()
        };
        let doc = dir.join("workspace.json");
        crate::save_workspace_to(&doc, &ws).unwrap();
        remove_transfers(scan(&dir).unwrap(), &HashSet::new()).unwrap();
        assert!(!path.exists());
        let restored = crate::load_workspace_from(&doc).unwrap();
        assert_eq!(
            read(&dir, &restored.handoffs[0]).unwrap(),
            "# Edited\n\n```\n    code\n```"
        );
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "only workspace.json remains"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cleanup_handles_late_files_and_preserves_pending_and_legacy_receivers() {
        let dir = scratch();
        let (pending, legacy, cancelled, completed) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        for id in [pending, legacy, cancelled, completed] {
            for suffix in ["md", "md.tmp", "reviewed.md", "reviewed.md.tmp"] {
                std::fs::write(dir.join(format!("{id}.{suffix}")), "summary").unwrap();
            }
        }
        std::fs::write(dir.join("notes.md"), "unrelated").unwrap();
        let retained = HashSet::from([pending, legacy]);
        let snapshot = scan(&dir).unwrap();
        // Another request arrives after this snapshot: never delete its file.
        let newer = dir.join(format!("{}.md", Uuid::new_v4()));
        std::fs::write(&newer, "new pending request").unwrap();
        remove_transfers(snapshot, &retained).unwrap();
        for id in [pending, legacy] {
            assert!(dir.join(format!("{id}.md")).exists());
        }
        for id in [cancelled, completed] {
            assert!(!dir.join(format!("{id}.md")).exists());
        }
        assert!(newer.exists());
        assert!(dir.join("notes.md").exists());
        // A cancelled source finishes late; the next sweep still removes it.
        let late = dir.join(format!("{cancelled}.md"));
        std::fs::write(&late, "late summary").unwrap();
        remove_transfers(scan(&dir).unwrap(), &retained).unwrap();
        assert!(!late.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn legacy_saved_review_takes_precedence_when_migrating() {
        let dir = scratch();
        let source = Instance::shell(Uuid::new_v4());
        let request = HandoffRequest::new(&source, "/repo".into(), String::new());
        std::fs::write(
            source_path(&dir, &request),
            format!("original\n{}", request.completion_marker()),
        )
        .unwrap();
        let reviewed = super::reviewed_path(&dir, &request);
        std::fs::write(
            &reviewed,
            format!("user edits\n{}", request.completion_marker()),
        )
        .unwrap();
        assert_eq!(read(&dir, &request).unwrap(), "user edits");
        std::fs::write(reviewed, "damaged review").unwrap();
        assert!(
            read(&dir, &request).is_err(),
            "never silently discard a saved review"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
