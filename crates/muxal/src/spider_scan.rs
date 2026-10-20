//! Bounded, cancellable, read-only inventory. Called only on a worker thread.
use crate::i18n::{t, tf};
use muxal_core::spider::{Entry, Kind, Tree, valid_relative};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub type Cancel = Arc<AtomicBool>;
const MAX_FILES: usize = 100_000;
const MAX_NODES: usize = 250_000;
const MAX_PATH_BYTES: usize = 32 * 1024 * 1024;
const FILE_BYTES: u64 = 2 * 1024 * 1024;
const TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const GIT_BYTES: u64 = 32 * 1024 * 1024;
const HISTORY_COMMITS: usize = 2000;
const SECONDS_30_DAYS: i64 = 30 * 86400;

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub root: PathBuf,
    pub tree: Tree,
    pub scanned_at: i64,
    pub elapsed: Duration,
    pub git: bool,
    pub history_empty: bool,
    pub history_limited: bool,
    pub issues: Vec<String>,
}

fn canceled(cancel: &AtomicBool, deadline: Instant) -> bool {
    cancel.load(Ordering::Relaxed) || Instant::now() >= deadline
}

/// Each child has bounded output and lifetime. Never block the UI or leave a
/// canceled `git log` running. Optional index locks/fsmonitor are disabled.
fn git(
    root: &Path,
    args: &[&str],
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<u8>, String> {
    if canceled(cancel, deadline) {
        return Err(t("Scan canceled or time budget reached").into());
    }
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("color.ui=false")
        .arg("-c")
        .arg("log.showSignature=false")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let overflow = Arc::new(AtomicBool::new(false));
    let flag = overflow.clone();
    let output = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(GIT_BYTES + 1).read_to_end(&mut bytes);
        if bytes.len() as u64 > GIT_BYTES {
            flag.store(true, Ordering::Relaxed);
        }
        (bytes, result)
    });
    let errors = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.take(64 * 1024).read_to_end(&mut bytes);
        bytes
    });
    let end = deadline.min(Instant::now() + Duration::from_secs(10));
    let status = loop {
        if canceled(cancel, end) || overflow.load(Ordering::Relaxed) {
            #[cfg(unix)]
            // This is the isolated process group created for this Git child.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let (bytes, read) = output.join().map_err(|_| "Git output reader failed")?;
    let errors = errors.join().unwrap_or_default();
    if status.is_none() || overflow.load(Ordering::Relaxed) {
        return Err(t("Git scan canceled, timed out, or exceeded its output budget").into());
    }
    read.map_err(|e| e.to_string())?;
    if !status.is_some_and(|s| s.success()) {
        return Err(String::from_utf8_lossy(&errors)
            .trim()
            .chars()
            .take(500)
            .collect());
    }
    Ok(bytes)
}

fn native_path(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(OsString::from_vec(bytes.to_vec()))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
    }
}

fn parse_status(bytes: &[u8]) -> HashMap<PathBuf, String> {
    let mut fields = bytes.split(|b| *b == 0);
    let mut out = HashMap::new();
    while let Some(field) = fields.next() {
        if field.len() < 4 {
            continue;
        }
        let status = String::from_utf8_lossy(&field[..2]).into_owned();
        let path = native_path(&field[3..]);
        if valid_relative(&path) {
            out.insert(path, status.clone());
        }
        if status.contains('R') || status.contains('C') {
            let _ = fields.next();
        }
    }
    out
}

/// `--raw -z --format=%x00%ct`: raw metadata consumes exactly one path field
/// (`--no-renames`), so a numeric/newline filename cannot be a timestamp/header.
fn parse_history(bytes: &[u8]) -> (HashMap<PathBuf, i64>, usize) {
    let mut out = HashMap::<PathBuf, i64>::new();
    let mut at = None;
    let mut path_next = false;
    let mut commits = 0;
    for field in bytes.split(|b| *b == 0) {
        if path_next {
            let path = native_path(field);
            if valid_relative(&path)
                && let Some(time) = at
            {
                out.entry(path)
                    .and_modify(|old| *old = (*old).max(time))
                    .or_insert(time);
            }
            path_next = false;
        } else {
            let trimmed = field.strip_prefix(b"\n").unwrap_or(field);
            if trimmed.starts_with(b":") {
                path_next = true;
            } else if let Ok(time) = String::from_utf8_lossy(trimmed).trim().parse::<i64>() {
                at = Some(time);
                commits += 1;
            }
        }
    }
    (out, commits)
}

/// Never read through a symlink in an ancestor, even if tracked paths survived
/// replacement of a whole directory by a link since the index was written.
fn entry_kind(root: &Path, relative: &Path) -> std::io::Result<(Kind, u64)> {
    let mut current = root.to_path_buf();
    for part in relative.components() {
        current.push(part);
        let meta = std::fs::symlink_metadata(&current)?;
        if meta.file_type().is_symlink() {
            return Ok((Kind::Symlink, meta.len()));
        }
        if meta.is_dir() && current.join(".git").exists() {
            return Ok((Kind::Boundary, 0));
        }
    }
    let meta = std::fs::symlink_metadata(&current)?;
    Ok((
        if meta.is_dir() {
            Kind::Boundary
        } else if meta.is_file() {
            Kind::Unknown
        } else {
            Kind::Boundary
        },
        meta.len(),
    ))
}

/// Resolve each component through directory descriptors on Unix, so a concurrent
/// directory-to-symlink replacement cannot redirect a read outside the checkout.
pub(super) fn open_project_file(root: &Path, relative: &Path) -> std::io::Result<std::fs::File> {
    #[cfg(unix)]
    {
        use std::os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        };
        let mut directory = std::fs::File::open(root)?;
        let mut parts = relative.components().peekable();
        while let Some(part) = parts.next() {
            let part = std::ffi::CString::new(part.as_os_str().as_bytes())?;
            let flags = libc::O_RDONLY
                | libc::O_CLOEXEC
                | libc::O_NOFOLLOW
                | libc::O_NONBLOCK
                | if parts.peek().is_some() {
                    libc::O_DIRECTORY
                } else {
                    0
                };
            let fd = unsafe { libc::openat(directory.as_raw_fd(), part.as_ptr(), flags) };
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            directory = unsafe { std::fs::File::from_raw_fd(fd) };
        }
        if !directory.metadata()?.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "not a regular file",
            ));
        }
        Ok(directory)
    }
    #[cfg(not(unix))]
    {
        std::fs::File::open(root.join(relative))
    }
}

fn line_count(
    root: &Path,
    relative: &Path,
    bytes: u64,
    total: &mut u64,
    cancel: &AtomicBool,
    deadline: Instant,
) -> (Kind, Option<u64>) {
    if bytes > FILE_BYTES || total.saturating_add(bytes) > TOTAL_BYTES {
        return (Kind::Unknown, None);
    }
    let Ok(file) = open_project_file(root, relative) else {
        return (Kind::Unknown, None);
    };
    let mut file = file.take(FILE_BYTES + 1);
    let mut buf = [0u8; 32 * 1024];
    let mut count = 0u64;
    let mut read = 0;
    let mut last = None;
    loop {
        if canceled(cancel, deadline) {
            return (Kind::Unknown, None);
        }
        let Ok(n) = file.read(&mut buf) else {
            return (Kind::Unknown, None);
        };
        if n == 0 {
            break;
        }
        *total += n as u64;
        if *total > TOTAL_BYTES {
            return (Kind::Unknown, None);
        }
        if read == 0
            && (buf[..n].contains(&0)
                || std::str::from_utf8(&buf[..n]).is_err_and(|e| e.error_len().is_some()))
        {
            return (Kind::Binary, None);
        }
        read += n as u64;
        if read > FILE_BYTES {
            return (Kind::Unknown, None);
        }
        count += buf[..n].iter().filter(|b| **b == b'\n').count() as u64;
        last = Some(buf[n - 1]);
    }
    (
        Kind::Text,
        Some(count + u64::from(last.is_some_and(|b| b != b'\n'))),
    )
}

pub fn scan(root: PathBuf, cancel: Cancel) -> Result<Snapshot, String> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(30);
    if !root.is_dir() {
        return Err(t("The selected checkout is unavailable").into());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let mut issues = Vec::new();
    let git_probe = git(
        &root,
        &["rev-parse", "--is-inside-work-tree"],
        &cancel,
        deadline,
    );
    let has_git = git_probe.as_ref().is_ok_and(|b| b.starts_with(b"true"));
    if let Err(error) = &git_probe
        && !error.contains("not a git repository")
    {
        issues.push(tf(
            "Git information unavailable: {error}",
            &[("error", error)],
        ));
    }
    let mut paths = BTreeSet::new();
    let mut changes = HashMap::new();
    let mut history = HashMap::new();
    let mut history_limited = true;
    let mut history_empty = false;
    let mut inventory_from_git = false;
    let head_args = ["rev-parse", "--verify", "--quiet", "HEAD"];
    let initial_head = if has_git {
        match git(&root, &head_args, &cancel, deadline) {
            Ok(head) => Some(head),
            Err(error) if error.is_empty() => {
                history_empty = true;
                None
            }
            Err(error) => {
                issues.push(tf("Git history unavailable: {error}", &[("error", &error)]));
                None
            }
        }
    } else {
        None
    };
    let index_path = if has_git {
        git(
            &root,
            &["rev-parse", "--git-path", "index"],
            &cancel,
            deadline,
        )
        .ok()
        .map(|path| root.join(native_path(path.strip_suffix(b"\n").unwrap_or(&path))))
    } else {
        None
    };
    let index_stamp = |path: &Path| {
        std::fs::metadata(path)
            .ok()
            .map(|m| (m.len(), m.modified().ok()))
    };
    let initial_index = index_path.as_deref().and_then(index_stamp);
    if has_git {
        match git(
            &root,
            &[
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ],
            &cancel,
            deadline,
        ) {
            Ok(output) => {
                inventory_from_git = true;
                for bytes in output.split(|b| *b == 0).filter(|b| !b.is_empty()) {
                    let path = native_path(bytes);
                    if valid_relative(&path) {
                        paths.insert(path);
                    }
                    if paths.len() >= MAX_FILES {
                        issues.push(t("File inventory reached its 100,000-file limit").into());
                        break;
                    }
                }
            }
            Err(error) => issues.push(tf(
                "Git inventory unavailable: {error}",
                &[("error", &error)],
            )),
        }
        // Capture HEAD around status/history so a branch switch cannot silently
        // combine paths from different checkouts.
        let head = initial_head.clone();
        match git(
            &root,
            &[
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--",
                ".",
            ],
            &cancel,
            deadline,
        ) {
            Ok(output) => {
                let prefix = git(&root, &["rev-parse", "--show-prefix"], &cancel, deadline)
                    .unwrap_or_default();
                let prefix = native_path(prefix.strip_suffix(b"\n").unwrap_or(&prefix));
                changes = parse_status(&output)
                    .into_iter()
                    .filter_map(|(path, status)| {
                        path.strip_prefix(&prefix)
                            .ok()
                            .map(|relative| (relative.to_path_buf(), status))
                    })
                    .collect();
            }
            Err(error) => issues.push(tf(
                "Git change status unavailable: {error}",
                &[("error", &error)],
            )),
        }
        if head.is_some() {
            let since = format!("--since-as-filter=@{}", now - SECONDS_30_DAYS);
            let count = format!("--max-count={}", HISTORY_COMMITS + 1);
            match git(
                &root,
                &[
                    "log",
                    "--raw",
                    "-z",
                    "--no-renames",
                    "--no-ext-diff",
                    "--format=%x00%ct",
                    "--relative",
                    &since,
                    &count,
                    "--",
                    ".",
                ],
                &cancel,
                deadline,
            ) {
                Ok(output) => {
                    let (times, count) = parse_history(&output);
                    history = times;
                    let shallow = git(
                        &root,
                        &["rev-parse", "--is-shallow-repository"],
                        &cancel,
                        deadline,
                    )
                    .ok();
                    history_limited =
                        count > HISTORY_COMMITS || shallow.is_none_or(|b| !b.starts_with(b"false"));
                }
                Err(error) => {
                    issues.push(tf("Git history unavailable: {error}", &[("error", &error)]))
                }
            }
        }
        if git(&root, &head_args, &cancel, deadline).ok() != head {
            return Err(
                t("Checkout changed during the scan; refresh to scan the new revision").into(),
            );
        }
    }
    if !inventory_from_git {
        let boundaries = Arc::new(std::sync::Mutex::new(Vec::new()));
        let found = boundaries.clone();
        let base = root.clone();
        let walker = ignore::WalkBuilder::new(&root)
            .hidden(false)
            .require_git(false)
            .follow_links(false)
            .max_depth(Some(128))
            .filter_entry(move |e| {
                if e.file_name() == ".git" {
                    return false;
                }
                if e.path() != base
                    && e.file_type().is_some_and(|t| t.is_dir())
                    && e.path().join(".git").exists()
                {
                    if let Ok(path) = e.path().strip_prefix(&base) {
                        found.lock().unwrap().push(path.to_path_buf());
                    }
                    return false;
                }
                true
            })
            .build();
        for entry in walker {
            if canceled(&cancel, deadline) {
                break;
            }
            match entry {
                Ok(entry) => {
                    if entry
                        .file_type()
                        .is_some_and(|t| t.is_file() || t.is_symlink())
                        && let Ok(path) = entry.path().strip_prefix(&root)
                    {
                        paths.insert(path.to_path_buf());
                    }
                }
                Err(error) => {
                    if issues.len() < 8 {
                        issues.push(error.to_string());
                    }
                }
            }
            if paths.len() >= MAX_FILES {
                issues.push(t("File inventory reached its 100,000-file limit").into());
                break;
            }
        }
        paths.extend(boundaries.lock().unwrap().iter().cloned());
    }
    // Deleted paths are kept as explicit change records, not omitted by the walk.
    for path in changes.keys() {
        if paths.len() < MAX_FILES {
            paths.insert(path.clone());
        }
    }
    let mut entries = Vec::with_capacity(paths.len());
    let mut total = 0;
    let mut skipped = 0;
    let mut model_paths = HashSet::new();
    let mut path_bytes = 0usize;
    let mut omitted = 0usize;
    for path in paths {
        if canceled(&cancel, deadline) {
            break;
        }
        if path.components().count() > 128 {
            omitted += 1;
            continue;
        }
        let new_paths: Vec<_> = path
            .ancestors()
            .filter(|p| !model_paths.contains(*p))
            .map(Path::to_path_buf)
            .collect();
        let added_bytes: usize = new_paths
            .iter()
            .map(|p| p.as_os_str().as_encoded_bytes().len())
            .sum();
        if model_paths.len() + new_paths.len() > MAX_NODES
            || path_bytes.saturating_add(added_bytes) > MAX_PATH_BYTES
        {
            omitted += 1;
            continue;
        }
        path_bytes += added_bytes;
        model_paths.extend(new_paths);
        let (mut kind, bytes) = match entry_kind(&root, &path) {
            Ok(value) => value,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && changes.get(&path).is_some_and(|s| s.contains('D')) =>
            {
                (Kind::Deleted, 0)
            }
            Err(_) => (Kind::Unknown, 0),
        };
        let lines = if kind == Kind::Unknown {
            let result = line_count(&root, &path, bytes, &mut total, &cancel, deadline);
            kind = result.0;
            result.1
        } else {
            None
        };
        if kind == Kind::Unknown {
            skipped += 1;
        }
        entries.push(Entry {
            committed_at: history.get(&path).copied(),
            change: changes.remove(&path),
            path,
            kind,
            bytes,
            lines,
        });
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(t("Scan canceled").into());
    }
    if has_git
        && Instant::now() < deadline
        && git(&root, &head_args, &cancel, deadline).ok() != initial_head
    {
        return Err(t("Checkout changed during the scan; refresh to scan the new revision").into());
    }
    if index_path.as_deref().and_then(index_stamp) != initial_index {
        return Err(
            t("Git index changed during the scan; refresh to scan its current state").into(),
        );
    }
    if Instant::now() >= deadline {
        issues.push(t("Scan time budget reached; inventory is partial").into());
    }
    if skipped > 0 {
        issues.push(tf(
            "{skipped} unreadable, changed, or oversized files have unknown text-line counts",
            &[("skipped", &skipped.to_string())],
        ));
    }
    if omitted > 0 {
        issues.push(tf(
            "{count} paths omitted at the map depth or memory limit",
            &[("count", &omitted.to_string())],
        ));
    }
    let mut tree = Tree::build(entries);
    if !issues.is_empty() {
        tree.nodes[0].incomplete = true;
    }
    Ok(Snapshot {
        root,
        tree,
        scanned_at: now,
        elapsed: started.elapsed(),
        git: has_git,
        history_empty,
        history_limited,
        issues,
    })
}

#[cfg(test)]
mod tests {
    use super::{Cancel, Kind, git, parse_history, parse_status, scan};
    use std::path::Path;
    use std::sync::{Arc, atomic::AtomicBool};
    use std::time::{Duration, Instant};
    fn cancel() -> Cancel {
        Arc::new(AtomicBool::new(false))
    }
    fn fixture() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("muxal-spider-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }
    fn run(root: &Path, args: &[&str]) {
        git(
            root,
            args,
            &cancel(),
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
    }
    #[test]
    fn inventory_counts_text_handles_binary_ignores_and_cancellation() {
        let root = fixture();
        std::fs::write(root.join(".ignore"), "skip/\n").unwrap();
        std::fs::create_dir(root.join("skip")).unwrap();
        std::fs::write(root.join("skip/no"), "no").unwrap();
        std::fs::write(root.join("text"), "a\nb").unwrap();
        std::fs::write(root.join("binary"), b"a\0b").unwrap();
        let snapshot = scan(root.clone(), cancel()).unwrap();
        assert!(!snapshot.git);
        assert!(snapshot.issues.is_empty());
        assert_eq!(
            snapshot.tree.nodes[snapshot.tree.find(Path::new("text")).unwrap()].lines,
            2
        );
        assert_eq!(
            snapshot.tree.nodes[snapshot.tree.find(Path::new("binary")).unwrap()].kind,
            Kind::Binary
        );
        assert!(snapshot.tree.find(Path::new("skip/no")).is_none());
        assert!(scan(root.clone(), Arc::new(AtomicBool::new(true))).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn empty_history_and_git_failure_are_distinct_from_a_non_git_folder() {
        let root = fixture();
        run(&root, &["init", "-q"]);
        std::fs::write(root.join("new.txt"), "new\n").unwrap();
        let snapshot = scan(root.clone(), cancel()).unwrap();
        assert!(snapshot.git);
        assert!(snapshot.history_empty);
        assert!(snapshot.issues.is_empty(), "{:?}", snapshot.issues);
        assert!(snapshot.tree.nodes[1].change.is_some());
        std::fs::remove_dir_all(root).unwrap();

        let broken = fixture();
        std::fs::write(broken.join(".git"), "invalid gitfile\n").unwrap();
        std::fs::write(broken.join("text"), "one\n").unwrap();
        let snapshot = scan(broken.clone(), cancel()).unwrap();
        assert!(!snapshot.git);
        assert!(!snapshot.history_empty);
        assert!(!snapshot.issues.is_empty());
        assert!(snapshot.tree.nodes[0].incomplete);
        std::fs::remove_dir_all(broken).unwrap();
    }
    #[test]
    fn real_git_tracks_ignored_files_status_history_and_project_subdirectory() {
        let root = fixture();
        run(&root, &["init", "-q"]);
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/123\nname"), "one\n").unwrap();
        std::fs::write(root.join("outside"), "outside\n").unwrap();
        run(&root, &["add", "."]);
        run(
            &root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-qm",
                "initial",
            ],
        );
        std::fs::write(root.join(".gitignore"), "sub/123*\nsub/ignored\n").unwrap();
        std::fs::write(root.join("sub/ignored"), "no").unwrap();
        std::fs::write(root.join("sub/123\nname"), "two\nthree\n").unwrap();
        let snapshot = scan(root.join("sub"), cancel()).unwrap();
        let id = snapshot.tree.find(Path::new("123\nname")).unwrap();
        assert_eq!(snapshot.tree.nodes[id].lines, 2);
        assert!(
            snapshot.tree.nodes[id].committed_at.is_some(),
            "{:?}",
            snapshot.issues
        );
        assert!(snapshot.tree.nodes[id].change.is_some());
        assert!(snapshot.tree.find(Path::new("outside")).is_none());
        assert!(snapshot.tree.find(Path::new("ignored")).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn worktree_snapshots_keep_their_own_files_and_changes() {
        let root = fixture();
        let worktree = fixture();
        run(&root, &["init", "-q"]);
        std::fs::write(root.join("shared.txt"), "main\n").unwrap();
        run(&root, &["add", "."]);
        run(
            &root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-qm",
                "initial",
            ],
        );
        run(
            &root,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "spider-fixture",
                worktree.to_str().unwrap(),
            ],
        );
        std::fs::write(worktree.join("shared.txt"), "worktree\nsecond line\n").unwrap();
        let main = scan(root.clone(), cancel()).unwrap();
        let other = scan(worktree.clone(), cancel()).unwrap();
        let main_node = &main.tree.nodes[main.tree.find(Path::new("shared.txt")).unwrap()];
        let other_node = &other.tree.nodes[other.tree.find(Path::new("shared.txt")).unwrap()];
        assert_eq!(main_node.lines, 1);
        assert!(main_node.change.is_none());
        assert_eq!(other_node.lines, 2);
        assert!(other_node.change.is_some());
        assert_ne!(main.root, other.root);
        assert!(other.git);
        std::fs::remove_dir_all(worktree).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn raw_parsers_keep_rename_and_numeric_path_boundaries() {
        let status = parse_status(b"R  new name\0old name\0?? next\0");
        assert_eq!(status.len(), 2);
        assert!(status.contains_key(Path::new("new name")));
        let (history, commits) = parse_history(
            b"\x00100\n\0:100644 100644 a b M\x00123\n\0\x00200\n\0:100644 100644 a b M\x00123\n\0",
        );
        assert_eq!(commits, 2);
        assert_eq!(history.get(Path::new("123\n")), Some(&200));
    }
    #[cfg(unix)]
    #[test]
    fn cancel_stops_git_and_its_pipe_holding_children() {
        let root = fixture();
        let token = cancel();
        let flag = token.clone();
        let timer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        let start = Instant::now();
        let result = git(
            &root,
            &["-c", "alias.muxal_scan_wait=!sleep 20", "muxal_scan_wait"],
            &token,
            start + Duration::from_secs(5),
        );
        timer.join().unwrap();
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_secs(3));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn deleted_and_oversized_files_are_explicit_and_nested_repos_are_boundaries() {
        let root = fixture();
        run(&root, &["init", "-q"]);
        std::fs::write(root.join("gone"), "old\n").unwrap();
        run(&root, &["add", "."]);
        run(
            &root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-qm",
                "initial",
            ],
        );
        std::fs::remove_file(root.join("gone")).unwrap();
        std::fs::write(
            root.join("huge"),
            vec![b'x'; (super::FILE_BYTES + 1) as usize],
        )
        .unwrap();
        std::fs::create_dir(root.join("nested")).unwrap();
        run(&root.join("nested"), &["init", "-q"]);
        std::fs::write(root.join("nested/private"), "not traversed").unwrap();
        let snapshot = scan(root.clone(), cancel()).unwrap();
        let tree = snapshot.tree;
        assert_eq!(
            tree.nodes[tree.find(Path::new("gone")).unwrap()].kind,
            Kind::Deleted
        );
        assert_eq!(
            tree.nodes[tree.find(Path::new("huge")).unwrap()].kind,
            Kind::Unknown
        );
        assert!(tree.find(Path::new("nested/private")).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn symlinks_keep_identity_without_reading_targets() {
        use std::os::unix::fs::symlink;
        let root = fixture();
        symlink("/etc", root.join("outside")).unwrap();
        symlink(".", root.join("loop")).unwrap();
        assert!(super::open_project_file(&root, Path::new("outside/passwd")).is_err());
        assert!(super::open_project_file(&root, Path::new("loop/outside/passwd")).is_err());
        let snapshot = scan(root.clone(), cancel()).unwrap();
        assert_eq!(
            snapshot.tree.nodes[snapshot.tree.find(Path::new("outside")).unwrap()].kind,
            Kind::Symlink
        );
        assert!(snapshot.tree.find(Path::new("outside/passwd")).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
    // APFS rejects invalid UTF-8 names; exercise this filesystem case on Linux
    // without preventing the portable symlink checks from running on macOS.
    #[cfg(target_os = "linux")]
    #[test]
    fn non_utf8_names_keep_their_path_identity() {
        use std::os::unix::ffi::OsStringExt;
        let root = fixture();
        let name = std::ffi::OsString::from_vec(b"file-\xff".to_vec());
        std::fs::write(root.join(&name), "x").unwrap();
        let snapshot = scan(root.clone(), cancel()).unwrap();
        let id = snapshot.tree.find(Path::new(&name)).unwrap();
        assert_eq!(snapshot.tree.nodes[id].lines, 1);
        assert!(snapshot.tree.find(Path::new("file-\u{fffd}")).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
