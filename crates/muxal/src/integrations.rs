//! Side-effecting wrappers around the `git` and `tmux` CLIs — the I/O half of
//! muxal-core's pure tmux/worktree helpers.

use crate::i18n::{t, tf};
use anyhow::{Context, Result, bail};
use muxal_core::memory::{self, MemoryEntry};
use muxal_core::{MEMORY_DIR, MEMORY_FILE, memory_header};
use std::path::{Path, PathBuf};
use std::process::Command;

/// `std::process::Command` for `program`, with the console window suppressed on
/// Windows. muxal is a GUI app, so spawning a console child (git, ssh, gh, …)
/// would otherwise flash a cmd window on every call — extremely visible because
/// muxal polls git in the background. No-op off Windows.
fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW — don't allocate/attach a console for the child.
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

/// Reap stale muxal AppImage squashfuse mounts left in `$TMPDIR` by prior
/// instances that were SIGKILLed or crashed before the AppImage runtime could
/// unmount them. Once such a mount goes dead (`statfs` → `ENOTCONN`), any
/// filesystem scan — `df`, which some desktop system monitors run every ~60s —
/// stalls in the kernel FUSE layer on it, which on Wayland surfaces as a
/// periodic cursor stutter that worsens as more leftovers pile up. muxal can't
/// catch SIGKILL, so it cleans up on the next launch.
///
/// Best-effort and fully detached: runs on a background thread so a hung probe
/// can never block startup; unmounts only mounts that fail a liveness probe (a
/// live one belongs to another running muxal instance) and never our own.
#[cfg(target_os = "linux")]
pub fn reap_stale_appimage_mounts() {
    let _ = std::thread::Builder::new()
        .name("muxal-reap-mounts".into())
        .spawn(|| {
            let Ok(mounts) = std::fs::read_to_string("/proc/self/mounts") else {
                return;
            };
            let self_appdir = std::env::var("APPDIR").ok();
            let candidates =
                muxal_core::foreign_muxal_appimage_mounts(&mounts, self_appdir.as_deref());

            let mut reaped = 0usize;
            for mp in candidates {
                // A live mount lists instantly; only reap the dead ones.
                if fuse_mount_is_live(&mp) {
                    continue;
                }
                if lazy_unmount_fuse(&mp) {
                    reaped += 1;
                }
            }
            if reaped > 0 {
                muxal_store::append_event_log(&format!("reaped {reaped} stale AppImage mount(s)"));
            }
        });
}

/// Probe a FUSE mountpoint for liveness. A dead squashfuse mount fails to open
/// or list (`ENOTCONN`); a live one lists instantly. Any error counts as dead —
/// the mount is unusable either way, and the caller only lazy-unmounts, which is
/// safe even if the probe is wrong.
#[cfg(target_os = "linux")]
fn fuse_mount_is_live(mountpoint: &str) -> bool {
    match std::fs::read_dir(mountpoint) {
        // `opendir` succeeded but the first `readdir` may still surface ENOTCONN.
        Ok(mut entries) => !matches!(entries.next(), Some(Err(_))),
        Err(_) => false,
    }
}

/// Lazily unmount a FUSE mountpoint via the user-space `fusermount` helper: it
/// works without root for the mount's owner, and `-z` detaches even a busy or
/// unresponsive mount (a plain unmount fails `EBUSY` on a mount something still
/// references). Returns whether a helper reported success.
#[cfg(target_os = "linux")]
fn lazy_unmount_fuse(mountpoint: &str) -> bool {
    ["fusermount3", "fusermount"].into_iter().any(|bin| {
        command(bin)
            .args(["-u", "-z", mountpoint])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

/// Where a git command runs: a local working tree.
pub struct RepoLoc(PathBuf);

impl RepoLoc {
    pub fn new(p: impl Into<PathBuf>) -> Self {
        Self(p.into())
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

/// Run `git <args>` at a [`RepoLoc`]: `git -C <path> …`.
fn git_output(loc: &RepoLoc, args: &[&str]) -> std::io::Result<std::process::Output> {
    command("git").arg("-C").arg(loc.path()).args(args).output()
}

/// Every tmux session on this machine. An empty list when no tmux server is
/// running; `None` when tmux couldn't be run at all.
fn ensure_gitignored(root: &Path, ignore_line: &str) -> Result<()> {
    let gitignore = root.join(".gitignore");
    let current = std::fs::read_to_string(&gitignore).unwrap_or_default();
    let ignored = current
        .lines()
        .any(|l| l.trim() == ignore_line || l.trim() == MEMORY_DIR);
    if ignored {
        return Ok(());
    }
    let mut next = current;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(ignore_line);
    next.push('\n');
    std::fs::write(&gitignore, next).with_context(|| format!("updating {}", gitignore.display()))
}

/// Ensure a project's shared memory file exists and is git-ignored, idempotently:
/// create `<root>/.muxal/`, seed `MEMORY.md` if absent, and add `.muxal/` to the
/// repo's `.gitignore` if not already there.
pub fn ensure_memory_file(loc: &RepoLoc) -> Result<()> {
    let root = loc.path();
    let ignore_line = format!("{MEMORY_DIR}/");
    let dir = root.join(MEMORY_DIR);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let file = dir.join(MEMORY_FILE);
    if !file.exists() {
        std::fs::write(&file, memory_header())
            .with_context(|| format!("writing {}", file.display()))?;
    }
    ensure_gitignored(root, &ignore_line)
}

/// Absolute path of a project's `.muxal/MEMORY.md`.
fn memory_abs(loc: &RepoLoc) -> PathBuf {
    loc.path().join(MEMORY_DIR).join(MEMORY_FILE)
}

/// Read and parse a project's memory file into entries. Missing/empty/unreadable →
/// an empty list (the file is optional and created on first save).
pub fn load_memory(loc: &RepoLoc) -> Vec<MemoryEntry> {
    muxal_store::migrate_legacy_memory_dir(loc.path());
    let text = std::fs::read_to_string(memory_abs(loc)).unwrap_or_default();
    memory::parse_document(&text)
}

/// Whether the project already has a `.muxal/MEMORY.md` — i.e. shared memory is
/// plainly in use here, whoever switched it on.
///
/// The evidence of last resort for the shared-memory flag: a layout doc written
/// before that flag existed carries no opinion, and defaulting such a project to
/// "off" would show the toggle off for a project whose agents are demonstrably
/// sharing a memory file on the host.
pub fn save_memory(loc: &RepoLoc, entries: &[MemoryEntry]) -> Result<()> {
    muxal_store::migrate_legacy_memory_dir(loc.path());
    let text = memory::render_document(entries);
    ensure_memory_file(loc)?; // dir + .gitignore (+ seed if absent)
    let file = memory_abs(loc);
    std::fs::write(&file, text).with_context(|| format!("writing {}", file.display()))
}

/// `git <args>` at `loc`; trimmed single-line stdout on success, else `None`.
fn git_line_loc(loc: &RepoLoc, args: &[&str]) -> Option<String> {
    let out = git_output(loc, args).ok().filter(|o| o.status.success())?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// `git <args>` at `loc`; `bail!`s with stderr (a useful toast) on failure.
fn git_run_loc(loc: &RepoLoc, args: &[&str]) -> Result<String> {
    let out = git_output(loc, args).with_context(|| format!("running `git {}`", args.join(" ")))?;
    if !out.status.success() {
        bail!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    // git writes its human summary to stdout (pull / commit / stash) or stderr
    // (push / fetch) — return whichever is non-empty so callers can surface it.
    let stdout = String::from_utf8_lossy(&out.stdout);
    let summary = stdout.trim();
    Ok(if summary.is_empty() {
        String::from_utf8_lossy(&out.stderr).trim().to_string()
    } else {
        summary.to_string()
    })
}

/// Whether `path` is inside a git working tree.
pub fn is_git_repo(path: &Path) -> bool {
    command("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Create a worktree at `worktree_path` on a new `branch`, based on `repo`.
pub fn create_worktree(repo: &Path, worktree_path: &Path, branch: &str) -> Result<()> {
    let output = command("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "add", "-b", branch])
        .arg(worktree_path)
        .output()
        .context("running `git worktree add`")?;
    if !output.status.success() {
        bail!(
            "git worktree add: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// Count uncommitted changes in `worktree_path` (staged, unstaged, untracked).
/// 0 if the path is gone or git fails — callers treat that as "clean".
pub fn worktree_change_count(worktree_path: &Path) -> usize {
    if !worktree_path.exists() {
        return 0;
    }
    command("git")
        .arg("-C")
        .arg(worktree_path)
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .count()
        })
        .unwrap_or(0)
}

/// The repo's current HEAD commit SHA — the base a worktree branch is measured
/// against. `None` if git fails.
pub fn repo_head(repo: &Path) -> Option<String> {
    git_line(repo, &["rev-parse", "HEAD"])
}

/// The repo's current branch name (e.g. `main`); `None` when detached (`"HEAD"`)
/// or git fails. Used only for display.
pub fn repo_current_branch(loc: &RepoLoc) -> Option<String> {
    git_line_loc(loc, &["rev-parse", "--abbrev-ref", "HEAD"]).filter(|b| b != "HEAD")
}

/// Run a git command in `dir` and return its trimmed single-line stdout on
/// success, else `None`.
fn git_line(dir: &Path, args: &[&str]) -> Option<String> {
    let out = command("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Count commits on the worktree's HEAD that are not reachable from `base` (the
/// main repo's HEAD) — i.e. unmerged work. 0 if the path is gone or git fails;
/// naturally 0 once the branch has been merged into `base`.
pub fn worktree_unmerged_count(worktree_path: &Path, base: &str) -> usize {
    if !worktree_path.exists() {
        return 0;
    }
    command("git")
        .arg("-C")
        .arg(worktree_path)
        .args(["rev-list", "--count", &format!("{base}..HEAD")])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(0)
}

/// Merge `branch` into whatever is checked out in `repo` (the base). On any
/// failure (e.g. conflicts) abort the merge so the repo isn't left mid-merge,
/// and return the error.
pub fn merge_worktree_branch(repo: &Path, branch: &str) -> Result<()> {
    let out = command("git")
        .arg("-C")
        .arg(repo)
        .args(["merge", "--no-edit", branch])
        .output()
        .context("running `git merge`")?;
    if !out.status.success() {
        // Leave no half-finished merge behind.
        let _ = command("git")
            .arg("-C")
            .arg(repo)
            .args(["merge", "--abort"])
            .output();
        bail!("git merge: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// Delete `branch` from `repo` (force). Best-effort — only valid once the branch
/// is no longer checked out in any worktree.
pub fn delete_branch(repo: &Path, branch: &str) {
    let _ = command("git")
        .arg("-C")
        .arg(repo)
        .args(["branch", "-D", branch])
        .output();
}

/// Stage **all** changes (tracked, untracked, and deletions) and commit them at
/// `loc`. Used where committing an entire worktree's work is the intent (e.g.
/// disposing a worktree). For a reviewed, file-by-file commit use
/// [`git_status_files`] + [`git_commit_paths`]. Errors on git failure.
pub fn git_commit(loc: &RepoLoc, msg: &str) -> Result<String> {
    git_run_loc(loc, &["add", "-A"])?;
    git_run_loc(loc, &["commit", "-m", msg])
}

/// Stage `paths` (`git add -- <paths>`), relative to the repo root. A directory
/// stages everything under it.
pub fn git_add_paths(loc: &RepoLoc, paths: &[String]) -> Result<()> {
    let mut args: Vec<&str> = vec!["add", "--"];
    args.extend(paths.iter().map(String::as_str));
    git_run_loc(loc, &args).map(|_| ())
}

/// One entry from `git status --porcelain` — i.e. a file a blanket `git add -A`
/// would stage. `status` is the two-char XY code (e.g. " M", "??", "A ", "D ",
/// "R "); `path` is the path to stage; `orig` is the source path for a
/// rename/copy (display only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitChange {
    pub status: String,
    pub path: String,
    pub orig: Option<String>,
}

/// List every changed/untracked file at `loc` — exactly what `git add -A` would
/// stage — from `git status --porcelain=v1 -z`. Empty on git failure or a clean
/// tree. The `-z` (NUL-separated) form sidesteps the path quoting `git status`
/// otherwise applies to names with spaces or non-ASCII characters.
pub fn git_status_files(loc: &RepoLoc) -> Vec<GitChange> {
    git_output(loc, &["status", "--porcelain=v1", "-z"])
        .ok()
        .filter(|o| o.status.success())
        .map(|o| parse_status_z(&o.stdout))
        .unwrap_or_default()
}

/// Parse the NUL-separated records of `git status --porcelain -z`: each record is
/// `XY <path>`, and for a rename/copy (X or Y is 'R'/'C') the *next* NUL field is
/// the original path (the `-z` form lists the new path first, then the old).
fn parse_status_z(bytes: &[u8]) -> Vec<GitChange> {
    let text = String::from_utf8_lossy(bytes);
    let mut fields = text.split('\0').filter(|s| !s.is_empty());
    let mut out = Vec::new();
    while let Some(rec) = fields.next() {
        // A record needs the two status chars, the separator space, and at least
        // one path character.
        if rec.len() < 4 {
            continue;
        }
        let status = rec[..2].to_string();
        let path = rec[3..].to_string();
        let orig = if status.starts_with('R') || status.starts_with('C') {
            fields.next().map(str::to_string)
        } else {
            None
        };
        out.push(GitChange { status, path, orig });
    }
    out
}

/// Unified diff for a single file at `loc`, vs HEAD (staged + unstaged combined).
/// `path` is the repo-relative path from [`git_status_files`]. For a brand-new
/// staged file (empty `diff HEAD`) it falls back to the staged (`--cached`) diff.
/// Returns display-ready diff text, truncated at [`MAX_DIFF_BYTES`], or a short
/// message when there's nothing to show.
pub fn git_diff_for(loc: &RepoLoc, path: &str) -> String {
    let run = |args: &[&str]| {
        git_output(loc, args)
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    };
    let mut out = run(&["diff", "HEAD", "--no-color", "--", path]).unwrap_or_default();
    if out.trim().is_empty() {
        // New/staged file: HEAD has nothing for it, so show the staged diff.
        out = run(&["diff", "--cached", "--no-color", "--", path]).unwrap_or_default();
    }
    if out.len() > MAX_DIFF_BYTES {
        out.truncate(MAX_DIFF_BYTES);
        out.push_str(&t("\n… diff truncated …\n"));
    }
    if out.trim().is_empty() {
        tf("# {path}\n\nNo diff available.\n", &[("path", path)])
    } else {
        out
    }
}

/// Stage a single file at `loc` (`git add -- <path>`).
pub fn git_stage_path(loc: &RepoLoc, path: &str) -> Result<String> {
    git_run_loc(loc, &["add", "--", path])
}

/// Unstage a single file at `loc` (`git restore --staged -- <path>`).
pub fn git_unstage_path(loc: &RepoLoc, path: &str) -> Result<String> {
    git_run_loc(loc, &["restore", "--staged", "--", path])
}

/// Discard all changes to a single file at `loc`: revert a tracked file's index
/// and worktree to HEAD, or delete it if untracked. DESTRUCTIVE — the caller must
/// confirm first.
pub fn git_discard_path(loc: &RepoLoc, path: &str) -> Result<String> {
    git_run_loc(
        loc,
        &[
            "restore",
            "--source=HEAD",
            "--staged",
            "--worktree",
            "--",
            path,
        ],
    )
    .or_else(|_| git_run_loc(loc, &["clean", "-fd", "--", path]))
}

/// Merge `branch` into whatever is checked out at `loc` (the base). `RepoLoc`
/// variant of [`merge_worktree_branch`]; aborts a failed merge
/// so the repo isn't left mid-merge.
pub fn merge_branch(loc: &RepoLoc, branch: &str) -> Result<String> {
    let out = git_output(loc, &["merge", "--no-edit", branch]).context("running `git merge`")?;
    if !out.status.success() {
        let _ = git_output(loc, &["merge", "--abort"]);
        bail!("git merge: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Remove the worktree at `worktree_path` (force) and prune stale entries, at
/// `loc`. `RepoLoc` variant of [`remove_worktree`].
pub fn remove_worktree_loc(loc: &RepoLoc, worktree_path: &str) -> Result<String> {
    let out = git_output(loc, &["worktree", "remove", "--force", worktree_path])
        .context("running `git worktree remove`")?;
    if !out.status.success() {
        bail!(
            "git worktree remove: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let _ = git_output(loc, &["worktree", "prune"]);
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Delete `branch` (force, `git branch -D`) at `loc`. `RepoLoc`
/// variant of [`delete_branch`].
pub fn delete_branch_loc(loc: &RepoLoc, branch: &str) -> Result<String> {
    git_run_loc(loc, &["branch", "-D", branch])
}

/// Stage exactly `paths` (their additions, modifications, and deletions, via
/// `git add -A -- <paths>`) and commit only those paths at `loc` (`git commit
/// --only`). Files outside `paths` are left untouched — even if already staged.
/// Errors on an empty selection or git failure.
pub fn git_commit_paths(loc: &RepoLoc, msg: &str, paths: &[String]) -> Result<String> {
    if paths.is_empty() {
        bail!("Select at least one file to commit");
    }
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();

    // Stage the selected paths first so untracked ones become known to git; then
    // commit only those paths, taking their working-tree state.
    let mut add = vec!["add", "-A", "--"];
    add.extend_from_slice(&refs);
    git_run_loc(loc, &add)?;

    let mut commit = vec!["commit", "--only", "-m", msg, "--"];
    commit.extend_from_slice(&refs);
    git_run_loc(loc, &commit)
}

/// Push the worktree's `branch` to `origin` (setting upstream). Errors on failure.
pub fn push_branch(worktree_path: &Path, branch: &str) -> Result<()> {
    let out = command("git")
        .arg("-C")
        .arg(worktree_path)
        .args(["push", "-u", "origin", branch])
        .output()
        .context("running `git push`")?;
    if !out.status.success() {
        bail!("git push: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// Push the branch, then open the PR-create page in a browser via `gh`.
pub fn create_pr(worktree_path: &Path, branch: &str) -> Result<()> {
    push_branch(worktree_path, branch)?;
    gh(worktree_path, &["pr", "create", "--web"])
}

/// Open the worktree branch's existing PR in a browser via `gh`.
pub fn open_pr(worktree_path: &Path) -> Result<()> {
    gh(worktree_path, &["pr", "view", "--web"])
}

/// Run `gh` in `dir`; bail with stderr on failure (e.g. not installed/authed).
fn gh(dir: &Path, args: &[&str]) -> Result<()> {
    let out = command("gh")
        .current_dir(dir)
        .args(args)
        .output()
        .context("running `gh` (is the GitHub CLI installed + authenticated?)")?;
    if !out.status.success() {
        bail!(
            "gh {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// Discard ALL changes in a worktree — uncommitted edits and its commits — by
/// hard-resetting to `base_ref` and removing untracked files. The worktree dir
/// stays (clean, at the base). Errors on git failure.
pub fn discard_worktree_changes(worktree_path: &Path, base_ref: &str) -> Result<()> {
    let run = |args: &[&str]| {
        command("git")
            .arg("-C")
            .arg(worktree_path)
            .args(args)
            .output()
            .context("running git")
    };
    let reset = run(&["reset", "--hard", base_ref])?;
    if !reset.status.success() {
        bail!(
            "git reset --hard: {}",
            String::from_utf8_lossy(&reset.stderr).trim()
        );
    }
    // Drop untracked files/dirs the agent created (best-effort).
    let _ = run(&["clean", "-fd"]);
    Ok(())
}

/// Remove a worktree (force) and prune stale entries. Best-effort.
pub fn remove_worktree(repo: &Path, worktree_path: &Path) {
    let _ = command("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "remove", "--force"])
        .arg(worktree_path)
        .output();
    let _ = command("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "prune"])
        .output();
}

/// Maximum diff text we load into the viewer (bytes), so a giant diff can't
/// bloat the editor buffer.
const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;

/// Working-tree changes for `dir`: tracked changes vs HEAD, scoped to the folder.
/// Returns display-ready unified-diff text, or a short human message when there's
/// nothing to show or `dir` isn't a git repo. Untracked/new files are not shown
/// (we can't tell agent-created files from pre-existing ones without a baseline).
pub fn git_diff(dir: &Path) -> String {
    let git = |args: &[&str]| {
        command("git")
            .arg("-C")
            .arg(dir)
            .arg("--no-pager")
            .args(args)
            .output()
    };

    // Resolve the repo root (also our "is this a git repo?" check, in one call).
    let toplevel = match git(&["rev-parse", "--show-toplevel"]) {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => {
            return tf(
                "# {dir}\n\nNot a git repository.\n",
                &[("dir", &dir.display().to_string())],
            );
        }
    };

    // A header so it's always obvious which folder the diff is reading from, and
    // a heads-up when that folder is only a subdirectory of a larger repo.
    let mut header = tf(
        "# Changes in {dir}\n",
        &[("dir", &dir.display().to_string())],
    );
    let is_subdir = matches!(
        (dir.canonicalize(), Path::new(&toplevel).canonicalize()),
        (Ok(d), Ok(t)) if d != t
    );
    if is_subdir {
        header.push_str(&tf(
            "# (subfolder of git repo {toplevel} — showing changes under this folder only)\n",
            &[("toplevel", &toplevel)],
        ));
    }
    header.push('\n');

    // Tracked changes (staged + unstaged) vs HEAD, scoped to this folder (`-- .`)
    // so a parent repo's changes elsewhere never bleed in.
    let mut out = String::new();
    match git(&["diff", "HEAD", "--no-color", "--", "."]) {
        Ok(o) if o.status.success() => out.push_str(&String::from_utf8_lossy(&o.stdout)),
        // No commits yet (HEAD invalid): fall back to the worktree/index diff.
        _ => {
            if let Ok(o) = git(&["diff", "--no-color", "--", "."]) {
                out.push_str(&String::from_utf8_lossy(&o.stdout));
            }
        }
    }

    if out.len() > MAX_DIFF_BYTES {
        out.truncate(MAX_DIFF_BYTES);
        out.push_str(&t("\n… diff truncated …\n"));
    }
    if out.trim().is_empty() {
        format!("{header}{}", t("No changes."))
    } else {
        format!("{header}{out}")
    }
}

/// Start the tmux server *before* any pane creates a session, from this benign
/// command line. Blocking, best-effort, idempotent.
///
/// tmux forks its server from whichever client first needs one, and the server
/// keeps that client's command line (its `comm` becomes `tmux: server`, but its
/// argv does not change). If that first client is a pane's
/// `tmux new-session -A -s muxal_<project>_… `, the argv of the *shared* server
/// contains the project's name — and one server hosts every session. An agent
/// then running `pkill -f <project>` to clear its dev server matches the server,
/// SIGKILLs it, and takes down every muxal session and every agent inside them.
///
/// Starting the server from here keeps project names out of its argv, so such a
/// `pkill` can only reach a pane's own tmux *client*: the session survives, the
/// agent keeps running, and the pane reattaches.
///
/// `exit-empty off` is not optional — by default a server holding no sessions
/// exits at once, so `start-server` alone would evaporate and the next
/// `new-session` would re-fork the server with the project name back in its argv.
/// [`restore_tmux_exit_empty`] puts it back when muxal quits.
pub fn ensure_tmux_server() {
    let _ = command("tmux")
        .args(muxal_core::tmux::start_server_args())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Undo [`ensure_tmux_server`]'s `exit-empty off` so the server goes away with
/// its last session once muxal is gone. Best-effort, fire-and-forget.
pub fn restore_tmux_exit_empty() {
    let _ = command("tmux")
        .args(muxal_core::tmux::restore_exit_empty_args())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Whether a tmux session is still alive (exact-match target, so `muxal_p_1` never
/// matches `muxal_p_12`). Fast and blocking; only called for a pane that just died.
pub fn tmux_session_exists(session: &str) -> bool {
    command("tmux")
        .args(["has-session", "-t", &format!("={session}")])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The last `lines` lines of a local tmux session's pane, scrollback included
/// (see [`muxal_core::tmux::capture_pane_args`]). `None` when tmux can't say.
pub fn kill_tmux_session(session: &str) {
    let _ = command("tmux")
        .args(muxal_core::tmux::kill_session_args(session))
        .output();
}

/// Fire-and-forget kill of a local tmux session (quit-time cleanup).
pub fn kill_local_tmux_detached(session: &str) {
    let _ = command("tmux")
        .args(muxal_core::tmux::kill_session_args(session))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Open the OS file manager at (or selecting) `path`. Best-effort, cross-platform.
pub fn reveal_in_file_manager(path: &Path) {
    #[cfg(target_os = "macos")]
    let _ = command("open").arg("-R").arg(path).output();
    #[cfg(target_os = "windows")]
    let _ = command("explorer")
        .arg(format!("/select,{}", path.display()))
        .output();
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        // No portable "select" on Linux — open the containing directory.
        let dir = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        let _ = command("xdg-open").arg(dir).output();
    }
}

/// Local branch names (e.g. `["main", "feature/x"]`) at `loc`.
pub fn list_branches(loc: &RepoLoc) -> Vec<String> {
    git_output(loc, &["branch", "--format=%(refname:short)"])
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Check out an existing branch.
pub fn checkout_branch(loc: &RepoLoc, branch: &str) -> Result<String> {
    git_run_loc(loc, &["checkout", branch])
}

/// Create + switch to a new branch.
pub fn create_branch(loc: &RepoLoc, name: &str) -> Result<String> {
    git_run_loc(loc, &["checkout", "-b", name])
}

/// `git pull` at `loc`.
pub fn git_pull(loc: &RepoLoc) -> Result<String> {
    git_run_loc(loc, &["pull"])
}

/// `git push` at `loc`.
pub fn git_push(loc: &RepoLoc) -> Result<String> {
    git_run_loc(loc, &["push"])
}

/// `git fetch` at `loc`.
pub fn git_fetch(loc: &RepoLoc) -> Result<String> {
    git_run_loc(loc, &["fetch"])
}

/// Stash the working tree (incl. untracked) at `loc`.
pub fn git_stash(loc: &RepoLoc) -> Result<String> {
    git_run_loc(loc, &["stash", "push", "--include-untracked"])
}

/// Pop (apply + remove) the most recent stash at `loc`.
pub fn git_stash_pop(loc: &RepoLoc) -> Result<String> {
    git_run_loc(loc, &["stash", "pop"])
}

/// Drop (discard) the most recent stash at `loc` — destructive.
pub fn git_stash_drop(loc: &RepoLoc) -> Result<String> {
    git_run_loc(loc, &["stash", "drop"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_create_and_remove() {
        let repo = std::env::temp_dir().join("muxal-it-repo");
        let worktree = std::env::temp_dir().join("muxal-it-worktree");
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&worktree);
        std::fs::create_dir_all(&repo).unwrap();

        let git = |args: &[&str]| {
            command("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap()
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "test@muxal"]);
        git(&["config", "user.name", "muxal test"]);
        std::fs::write(repo.join("file.txt"), "hello").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "init"]);

        assert!(is_git_repo(&repo));
        assert!(!is_git_repo(&std::env::temp_dir()));

        create_worktree(&repo, &worktree, "muxal/test").expect("create worktree");
        assert!(
            worktree.join("file.txt").exists(),
            "worktree should be checked out"
        );

        remove_worktree(&repo, &worktree);
        assert!(!worktree.exists(), "worktree should be removed");

        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn unmerged_count_and_merge() {
        let repo = std::env::temp_dir().join("muxal-it-unmerged");
        let worktree = std::env::temp_dir().join("muxal-it-unmerged-wt");
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&worktree);
        std::fs::create_dir_all(&repo).unwrap();
        let git = |dir: &Path, args: &[&str]| {
            command("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .unwrap()
        };
        git(&repo, &["init", "-q"]);
        git(&repo, &["config", "user.email", "test@muxal"]);
        git(&repo, &["config", "user.name", "muxal test"]);
        std::fs::write(repo.join("file.txt"), "hello").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "init"]);

        create_worktree(&repo, &worktree, "muxal/test").expect("create worktree");
        let base = repo_head(&repo).expect("repo head");
        // A fresh worktree has nothing ahead of base.
        assert_eq!(worktree_unmerged_count(&worktree, &base), 0);

        // Commit inside the worktree → one unmerged commit, but a clean tree.
        std::fs::write(worktree.join("feature.txt"), "work").unwrap();
        git(&worktree, &["add", "."]);
        git(&worktree, &["commit", "-q", "-m", "feature"]);
        assert_eq!(worktree_change_count(&worktree), 0, "tree should be clean");
        assert_eq!(worktree_unmerged_count(&worktree, &base), 1);

        // Merge it into the repo's base branch → the work lands there.
        merge_worktree_branch(&repo, "muxal/test").expect("merge");
        assert!(
            repo.join("feature.txt").exists(),
            "merged file should appear in the base repo"
        );
        // After merging, nothing is unmerged anymore.
        let base2 = repo_head(&repo).expect("repo head");
        assert_eq!(worktree_unmerged_count(&worktree, &base2), 0);

        // Cleanup: remove the worktree, then the (now merged) branch.
        remove_worktree(&repo, &worktree);
        delete_branch(&repo, "muxal/test");
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn git_diff_shows_tracked_changes_only() {
        let repo = std::env::temp_dir().join("muxal-it-diff");
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            command("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap()
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "test@muxal"]);
        git(&["config", "user.name", "muxal test"]);
        std::fs::write(repo.join("tracked.txt"), "one\ntwo\n").unwrap();
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        std::fs::write(repo.join("sub/insub.txt"), "a\nb\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "init"]);

        // Modify tracked files (root + subfolder) and create an untracked file.
        std::fs::write(repo.join("tracked.txt"), "one\nCHANGED\n").unwrap();
        std::fs::write(repo.join("sub/insub.txt"), "a\nSUBCHANGED\n").unwrap();
        std::fs::write(repo.join("untracked_new.txt"), "nope\n").unwrap();

        let diff = git_diff(&repo);
        // The header names the exact folder being diffed.
        assert!(
            diff.contains(&repo.display().to_string()),
            "header shows the folder path:\n{diff}"
        );
        assert!(
            diff.contains("tracked.txt"),
            "tracked change shown:\n{diff}"
        );
        assert!(diff.contains("CHANGED"), "modified line shown:\n{diff}");
        // Untracked files are NOT listed.
        assert!(
            !diff.contains("untracked_new.txt") && !diff.contains("nope"),
            "untracked file must be excluded:\n{diff}"
        );

        // Diffing the subfolder is scoped to it: flags the parent repo, shows the
        // subfolder's change, and does NOT include the parent's tracked.txt change.
        let sub_diff = git_diff(&repo.join("sub"));
        assert!(
            sub_diff.contains("subfolder of git repo"),
            "subfolder note shown:\n{sub_diff}"
        );
        assert!(
            sub_diff.contains("SUBCHANGED"),
            "subfolder change shown:\n{sub_diff}"
        );
        assert!(
            !sub_diff.contains("tracked.txt"),
            "parent's change must be scoped out:\n{sub_diff}"
        );

        // A non-repo directory reports as such (and still names the folder).
        let plain = std::env::temp_dir().join("muxal-it-not-a-repo");
        let _ = std::fs::remove_dir_all(&plain);
        std::fs::create_dir_all(&plain).unwrap();
        assert!(
            git_diff(&plain).contains("Not a git repository."),
            "non-repo message"
        );

        let _ = std::fs::remove_dir_all(&plain);
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn ensure_memory_file_local_creates_and_gitignores() {
        let root = std::env::temp_dir().join("muxal-it-memory");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // Pre-existing .gitignore without our entry.
        std::fs::write(root.join(".gitignore"), "target\n").unwrap();

        ensure_memory_file(&RepoLoc::new(root.clone())).expect("ensure memory");

        let mem = root.join(MEMORY_DIR).join(MEMORY_FILE);
        assert!(mem.exists(), "MEMORY.md should be created");
        let gi = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        assert!(gi.lines().any(|l| l.trim() == ".muxal/"), "gitignored");
        assert!(gi.contains("target"), "kept existing entries");

        // Idempotent: a second call doesn't duplicate the gitignore line or clobber.
        std::fs::write(&mem, "kept user notes").unwrap();
        ensure_memory_file(&RepoLoc::new(root.clone())).expect("ensure memory again");
        let gi2 = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        assert_eq!(gi2.matches(".muxal/").count(), 1, "no duplicate ignore");
        assert_eq!(std::fs::read_to_string(&mem).unwrap(), "kept user notes");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_status_z_handles_untracked_modified_and_rename() {
        // -z records: " M a.txt", "?? b c.txt" (space in name, unquoted),
        // and a staged rename "R  new.txt\0old.txt" (new path first, then old).
        let raw = b" M a.txt\0?? b c.txt\0R  new.txt\0old.txt\0";
        let got = parse_status_z(raw);
        assert_eq!(
            got,
            vec![
                GitChange {
                    status: " M".into(),
                    path: "a.txt".into(),
                    orig: None,
                },
                GitChange {
                    status: "??".into(),
                    path: "b c.txt".into(),
                    orig: None,
                },
                GitChange {
                    status: "R ".into(),
                    path: "new.txt".into(),
                    orig: Some("old.txt".into()),
                },
            ]
        );
    }

    #[test]
    fn status_files_lists_all_changes_and_commit_paths_is_selective() {
        let repo = std::env::temp_dir().join("muxal-it-commit-paths");
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            command("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap()
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "test@muxal"]);
        git(&["config", "user.name", "muxal test"]);
        std::fs::write(repo.join("keep.txt"), "v1\n").unwrap();
        std::fs::write(repo.join("gone.txt"), "bye\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "init"]);

        // Modify a tracked file, delete a tracked file, add two untracked files.
        std::fs::write(repo.join("keep.txt"), "v2\n").unwrap();
        std::fs::remove_file(repo.join("gone.txt")).unwrap();
        std::fs::write(repo.join("wanted.txt"), "new\n").unwrap();
        std::fs::write(repo.join("extra.txt"), "junk\n").unwrap();

        let loc = RepoLoc::new(repo.clone());

        // status lists every changed + untracked file.
        let listed: std::collections::BTreeSet<String> =
            git_status_files(&loc).into_iter().map(|c| c.path).collect();
        assert_eq!(
            listed,
            ["extra.txt", "gone.txt", "keep.txt", "wanted.txt"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        );

        // Commit only a subset (modify + deletion + one new file), NOT extra.txt.
        git_commit_paths(
            &loc,
            "selective",
            &["keep.txt".into(), "gone.txt".into(), "wanted.txt".into()],
        )
        .expect("selective commit");

        // The unselected untracked file is all that remains uncommitted.
        let remaining: Vec<String> = git_status_files(&loc).into_iter().map(|c| c.path).collect();
        assert_eq!(remaining, vec!["extra.txt".to_string()]);

        // HEAD recorded exactly the three selected changes.
        let show = command("git")
            .arg("-C")
            .arg(&repo)
            .args(["show", "--name-status", "--format=", "HEAD"])
            .output()
            .unwrap();
        let names = String::from_utf8_lossy(&show.stdout);
        assert!(names.contains("keep.txt"), "modify committed:\n{names}");
        assert!(names.contains("gone.txt"), "deletion committed:\n{names}");
        assert!(names.contains("wanted.txt"), "new file committed:\n{names}");
        assert!(
            !names.contains("extra.txt"),
            "unselected file must not be committed:\n{names}"
        );

        // An empty selection is rejected rather than producing an empty commit.
        assert!(git_commit_paths(&loc, "noop", &[]).is_err());

        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn stage_unstage_discard_and_diff_single_file() {
        let repo = std::env::temp_dir().join("muxal-it-per-file-ops");
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            command("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap()
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "test@muxal"]);
        git(&["config", "user.name", "muxal test"]);
        // Keep line endings deterministic: Windows git defaults to
        // core.autocrlf=true, which would restore the file as "one\r\n".
        git(&["config", "core.autocrlf", "false"]);
        std::fs::write(repo.join("a.txt"), "one\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "init"]);
        let loc = RepoLoc::new(repo.clone());

        // Modify the file: its single-file diff shows the change.
        std::fs::write(repo.join("a.txt"), "two\n").unwrap();
        let diff = git_diff_for(&loc, "a.txt");
        assert!(diff.contains("-one"), "diff shows removed line:\n{diff}");
        assert!(diff.contains("+two"), "diff shows added line:\n{diff}");

        // Stage → X column M (staged-modified, worktree clean).
        git_stage_path(&loc, "a.txt").expect("stage");
        assert_eq!(git_status_files(&loc)[0].status, "M ");

        // Unstage → back to worktree-modified.
        git_unstage_path(&loc, "a.txt").expect("unstage");
        assert_eq!(git_status_files(&loc)[0].status, " M");

        // Discard reverts a tracked file to HEAD: clean tree, original content.
        git_discard_path(&loc, "a.txt").expect("discard tracked");
        assert!(
            git_status_files(&loc).is_empty(),
            "tree clean after discard"
        );
        assert_eq!(
            std::fs::read_to_string(repo.join("a.txt")).unwrap(),
            "one\n"
        );

        // Discard also removes an untracked file.
        std::fs::write(repo.join("junk.txt"), "x\n").unwrap();
        git_discard_path(&loc, "junk.txt").expect("discard untracked");
        assert!(!repo.join("junk.txt").exists(), "untracked file removed");

        let _ = std::fs::remove_dir_all(&repo);
    }
}
