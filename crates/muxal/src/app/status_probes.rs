//! Worker-owned caches: stat metadata cheaply, launch subprocesses only when
//! inputs change or a bounded fallback interval expires.
use super::{AgentPreset, HashMap, HashSet, Instant, PathBuf, installed_programs, program_on_path};
use crate::integrations;
use std::path::Path;
use std::time::{Duration, SystemTime};

type Branches = (Option<String>, Vec<String>);
type ProgramResult = (HashSet<String>, bool, bool, bool);
#[derive(Default)]
pub(super) struct Cache {
    branches: HashMap<PathBuf, (String, Instant, Branches)>,
    changes: HashMap<PathBuf, (Instant, usize)>,
    programs: Option<(String, Instant, ProgramResult)>,
}
fn metadata(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = path.metadata().ok()?;
    Some((meta.modified().ok()?, meta.len()))
}
fn ref_stamp(root: &Path) -> String {
    let git = root.ancestors().find_map(|parent| {
        let dot = parent.join(".git");
        if dot.is_dir() {
            Some(dot)
        } else {
            let file = std::fs::read_to_string(&dot).ok()?;
            Some(parent.join(file.trim().strip_prefix("gitdir:")?.trim()))
        }
    });
    let Some(git) = git else { return String::new() };
    let common = std::fs::read_to_string(git.join("commondir"))
        .map(|path| git.join(path.trim()))
        .unwrap_or_else(|_| git.clone());
    let mut refs = Vec::new();
    let mut dirs = vec![common.join("refs/heads")];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .take(4096)
        {
            if refs.len() >= 4096 {
                break;
            }
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                dirs.push(path.clone());
            }
            refs.push((path.clone(), metadata(&path)));
        }
        if refs.len() >= 4096 {
            break;
        }
    }
    refs.sort();
    format!(
        "{:?}{:?}{:?}{:?}",
        std::fs::read(git.join("HEAD")).ok(),
        refs,
        metadata(&common.join("packed-refs")),
        metadata(&common.join("reftable/tables.list"))
    )
}
impl Cache {
    pub(super) fn retain(&mut self, roots: &[PathBuf], worktrees: &[PathBuf]) {
        self.branches.retain(|path, _| roots.contains(path));
        self.changes.retain(|path, _| worktrees.contains(path));
    }
    pub(super) fn branches(&mut self, root: &Path) -> Branches {
        let stamp = ref_stamp(root);
        if let Some((old, at, result)) = self.branches.get(root)
            && *old == stamp
            && at.elapsed() < Duration::from_secs(60)
        {
            return result.clone();
        }
        let loc = integrations::RepoLoc::new(root);
        let result = (
            integrations::repo_current_branch(&loc),
            integrations::list_branches(&loc),
        );
        self.branches
            .insert(root.to_owned(), (stamp, Instant::now(), result.clone()));
        result
    }
    pub(super) fn changes(&mut self, path: &Path) -> usize {
        if let Some((at, count)) = self.changes.get(path)
            && at.elapsed() < Duration::from_secs(10)
        {
            return *count;
        }
        let count = integrations::worktree_change_count(path);
        self.changes
            .insert(path.to_owned(), (Instant::now(), count));
        count
    }
    pub(super) fn programs(&mut self, presets: &[AgentPreset]) -> ProgramResult {
        let paths = std::env::var_os("PATH").unwrap_or_default();
        let mut inputs: Vec<_> = std::env::split_paths(&paths)
            .map(|path| (path.clone(), metadata(&path)))
            .collect();
        for program in presets.iter().filter_map(|preset| preset.program.as_ref()) {
            let path = PathBuf::from(program);
            inputs.push((path.clone(), metadata(&path)));
        }
        let stamp = format!("{inputs:?}");
        if let Some((old, at, result)) = &self.programs
            && *old == stamp
            && at.elapsed() < Duration::from_secs(60)
        {
            return result.clone();
        }
        let result = (
            installed_programs(presets),
            program_on_path("gh"),
            program_on_path("sshpass"),
            cfg!(unix) && program_on_path("tmux"),
        );
        self.programs = Some((stamp, Instant::now(), result.clone()));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::{Cache, ref_stamp};
    use std::process::Command;
    #[test]
    fn branches_refresh_on_nested_refs_and_linked_worktree_head_changes() {
        let root = std::env::temp_dir().join(format!("muxal-probes-{}", uuid::Uuid::new_v4()));
        let repo = root.join("repo");
        let wt = root.join("worktree");
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        let plain = ref_stamp(&repo);
        git(&["init", "-qb", "main"]);
        git(&["config", "user.email", "test@example.invalid"]);
        git(&["config", "user.name", "Test"]);
        git(&["commit", "--allow-empty", "-qm", "base"]);
        assert_ne!(ref_stamp(&repo), plain);
        let mut cache = Cache::default();
        assert_eq!(cache.branches(&repo).0.as_deref(), Some("main"));
        let first = cache.branches[&repo].1;
        cache.branches(&repo);
        assert_eq!(
            cache.branches[&repo].1, first,
            "unchanged repo must reuse its probe"
        );
        git(&["branch", "feature/nested"]);
        assert!(
            cache
                .branches(&repo)
                .1
                .contains(&"feature/nested".to_string())
        );
        git(&["worktree", "add", "-qb", "wt", &wt.to_string_lossy()]);
        assert_eq!(cache.branches(&wt).0.as_deref(), Some("wt"));
        git(&["branch", "other"]);
        assert!(cache.branches(&wt).1.contains(&"other".to_string()));
        git(&["pack-refs", "--all"]);
        git(&["branch", "-D", "feature/nested"]);
        assert!(
            !cache
                .branches(&wt)
                .1
                .contains(&"feature/nested".to_string())
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
