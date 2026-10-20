//! Native observation shared by visible optional Spider overlays. No polling of
//! the checkout, no Git processes, and no reads of source for activity alone.
use crate::i18n::t;
use crate::spider_scan::{Snapshot, open_project_file};
use muxal_core::spider::connections::{Graph, rust::RustIndex};
use muxal_core::spider::{Kind, valid_relative};
use notify::{
    Event, EventKind, RecursiveMode, Watcher,
    event::{MetadataKind, ModifyKind},
};
use std::collections::{BTreeSet, HashSet, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc::{self, Receiver, SyncSender},
};
use std::time::{Duration, Instant};

const MAX_DIRECTORIES: usize = 2048;
const MAX_EVENT_PATHS: usize = 2048;
const MAX_SOURCE_FILES: usize = 5000;
const MAX_SOURCE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TOTAL_SOURCE_BYTES: u64 = 32 * 1024 * 1024;

pub struct Update {
    pub paths: Vec<PathBuf>,
    pub observed: Instant,
    pub structural: bool,
    pub graph: Option<Arc<Graph>>,
    pub ready: bool,
    pub issue: Option<String>,
}
enum Command {
    Event(notify::Result<Event>),
    Stop,
}

#[derive(Default)]
struct Stats {
    watches: AtomicUsize,
    source_reads: AtomicUsize,
    alive: AtomicBool,
}
struct Running<'a>(&'a AtomicBool);
impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
#[derive(Default)]
struct Pending {
    updates: VecDeque<Update>,
    graph: Option<Arc<Graph>>,
}

pub struct Monitor {
    tx: SyncSender<Command>,
    pending: Arc<Mutex<Pending>>,
    cancel: Arc<AtomicBool>,
    overflow: Arc<AtomicBool>,
    stats: Arc<Stats>,
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        let _ = self.tx.try_send(Command::Stop);
    }
}
impl Monitor {
    pub fn start(snapshot: Arc<Snapshot>, connections: bool) -> std::io::Result<Self> {
        let (tx, input) = mpsc::sync_channel(128);
        let pending = Arc::new(Mutex::new(Pending::default()));
        let output = pending.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let overflow = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(Stats::default());
        let (worker_cancel, worker_overflow, worker_stats) =
            (cancel.clone(), overflow.clone(), stats.clone());
        let events = tx.clone();
        std::thread::Builder::new()
            .name("spider-observer".into())
            .spawn(move || {
                worker_stats.alive.store(true, Ordering::Release);
                let _running = Running(&worker_stats.alive);
                run(
                    snapshot,
                    connections,
                    events,
                    input,
                    output,
                    &worker_cancel,
                    &worker_overflow,
                    &worker_stats,
                );
            })?;
        Ok(Self {
            tx,
            pending,
            cancel,
            overflow,
            stats,
        })
    }
    pub fn drain(&self) -> Vec<Update> {
        let mut updates = Vec::new();
        if let Ok(mut pending) = self.pending.try_lock() {
            updates.extend(pending.updates.drain(..));
            if let Some(last) = updates.last_mut() {
                last.graph = pending.graph.take();
            }
        }
        if self.overflow.swap(false, Ordering::Relaxed) {
            updates.push(Update {
                paths: Vec::new(),
                observed: Instant::now(),
                structural: true,
                graph: None,
                ready: false,
                issue: Some(
                    t("Some live changes could not be observed; refresh to reconcile").into(),
                ),
            });
        }
        updates
    }
    pub fn is_alive(&self) -> bool {
        self.stats.alive.load(Ordering::Acquire)
    }
}

fn send(output: &Mutex<Pending>, overflow: &AtomicBool, mut update: Update) {
    let Ok(mut pending) = output.lock() else {
        overflow.store(true, Ordering::Relaxed);
        return;
    };
    // Retain only the latest potentially large graph, while preserving event
    // timestamps in a bounded queue. A blocked UI cannot accumulate graph copies.
    if update.graph.is_some() {
        pending.graph = update.graph.take();
    }
    if pending.updates.len() >= 32 {
        pending.updates.pop_front();
        overflow.store(true, Ordering::Relaxed);
    }
    pending.updates.push_back(update);
}
fn issue(output: &Mutex<Pending>, overflow: &AtomicBool, message: String) {
    send(
        output,
        overflow,
        Update {
            paths: Vec::new(),
            observed: Instant::now(),
            structural: false,
            graph: None,
            ready: true,
            issue: Some(message),
        },
    );
}
fn safe_directory(root: &Path, relative: &Path) -> bool {
    let mut path = root.to_path_buf();
    for part in relative.components() {
        path.push(part);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            return false;
        };
        if !meta.is_dir() || meta.file_type().is_symlink() || path.join(".git").exists() {
            return false;
        }
    }
    true
}
fn read_source(root: &Path, relative: &Path, total: &mut u64, stats: &Stats) -> Option<String> {
    let file = open_project_file(root, relative).ok()?;
    let size = file.metadata().ok()?.len();
    if size > MAX_SOURCE_BYTES || total.saturating_add(size) > MAX_TOTAL_SOURCE_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    *total = total.saturating_add(bytes.len() as u64);
    if bytes.len() as u64 > MAX_SOURCE_BYTES
        || *total > MAX_TOTAL_SOURCE_BYTES
        || bytes.contains(&0)
    {
        return None;
    }
    stats.source_reads.fetch_add(1, Ordering::Relaxed);
    String::from_utf8(bytes).ok()
}

#[allow(clippy::too_many_arguments)]
fn run(
    snapshot: Arc<Snapshot>,
    connections: bool,
    events: SyncSender<Command>,
    input: Receiver<Command>,
    output: Arc<Mutex<Pending>>,
    cancel: &AtomicBool,
    overflow: &Arc<AtomicBool>,
    stats: &Stats,
) {
    let Ok(root) = snapshot.root.canonicalize() else {
        issue(
            &output,
            overflow,
            t("The selected checkout is unavailable").into(),
        );
        return;
    };
    let flag = overflow.clone();
    let mut watcher = match notify::recommended_watcher(move |result: notify::Result<Event>| {
        if let Ok(event) = &result
            && matches!(event.kind, EventKind::Access(_))
        {
            return;
        }
        let result = result.map(|mut e| {
            if e.paths.len() > 64 {
                flag.store(true, Ordering::Relaxed);
            }
            e.paths.truncate(64);
            e
        });
        if events.try_send(Command::Event(result)).is_err() {
            flag.store(true, Ordering::Relaxed);
        }
    }) {
        Ok(watcher) => watcher,
        Err(_) => {
            issue(
                &output,
                overflow,
                t("Live file observation is unavailable; manual Refresh still works").into(),
            );
            return;
        }
    };
    let tree = &snapshot.tree;
    let mut directories = HashSet::new();
    let mut warnings = Vec::new();
    let mut ignore_builder = ignore::gitignore::GitignoreBuilder::new(&root);
    for node in tree.nodes.iter().filter(|n| n.kind == Kind::Directory) {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        if !node.path.as_os_str().is_empty() && !safe_directory(&root, &node.path) {
            continue;
        }
        if directories.len() >= MAX_DIRECTORIES {
            warnings.push(
                t("Live observation reached its folder limit; some areas need manual Refresh")
                    .to_string(),
            );
            break;
        }
        let directory = root.join(&node.path);
        for name in [".gitignore", ".ignore"] {
            let path = directory.join(name);
            if path
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_file())
            {
                let _ = ignore_builder.add(path);
            }
        }
        if watcher
            .watch(&directory, RecursiveMode::NonRecursive)
            .is_ok()
        {
            directories.insert(node.path.clone());
            stats.watches.fetch_add(1, Ordering::Relaxed);
        } else if warnings.is_empty() {
            warnings.push(
                t("Some folders could not be observed; manual Refresh is available").to_string(),
            );
        }
    }
    let ignored = ignore_builder.build().ok();
    let (global_ignored, _) = ignore::gitignore::GitignoreBuilder::new(&root).build_global();
    let source_paths: BTreeSet<_> = if connections {
        tree.nodes
            .iter()
            .filter(|n| {
                matches!(n.kind, Kind::Text | Kind::Unknown | Kind::Deleted)
                    && RustIndex::supported(&n.path)
            })
            .take(MAX_SOURCE_FILES + 1)
            .map(|n| n.path.clone())
            .collect()
    } else {
        BTreeSet::new()
    };
    let source_allowed: HashSet<_> = source_paths
        .iter()
        .take(MAX_SOURCE_FILES)
        .cloned()
        .collect();
    let mut index = RustIndex::default();
    let mut source_limited = source_paths.len() > MAX_SOURCE_FILES;
    if connections {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut total = 0;
        let mut count = 0;
        for path in source_paths.iter().take(MAX_SOURCE_FILES) {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            if count >= MAX_SOURCE_FILES || Instant::now() >= deadline {
                source_limited = true;
                break;
            }
            if let Some(source) = read_source(&root, path, &mut total, stats) {
                index.update(path.clone(), &source);
                count += 1;
            } else {
                source_limited = true;
            }
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    let graph = connections.then(|| {
        let mut graph = index.graph();
        graph.partial |= source_limited;
        Arc::new(graph)
    });
    send(
        &output,
        overflow,
        Update {
            paths: Vec::new(),
            observed: Instant::now(),
            structural: false,
            graph,
            ready: true,
            issue: (!warnings.is_empty()).then(|| warnings.join(" · ")),
        },
    );
    log::debug!(
        "Spider observer ready: {} folders, {} source reads",
        stats.watches.load(Ordering::Relaxed),
        stats.source_reads.load(Ordering::Relaxed)
    );
    while !cancel.load(Ordering::Relaxed) {
        let Ok(first) = input.recv() else {
            return;
        };
        let mut pending = vec![first];
        let until = Instant::now() + Duration::from_millis(100);
        while pending.len() < 128 {
            let Some(left) = until.checked_duration_since(Instant::now()) else {
                break;
            };
            match input.recv_timeout(left) {
                Ok(event) => pending.push(event),
                Err(_) => break,
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let mut paths = BTreeSet::new();
        let mut structural = false;
        let mut rescan = false;
        for command in pending {
            let result = match command {
                Command::Stop => return,
                Command::Event(result) => result,
            };
            let event = match result {
                Ok(event) => event,
                Err(_) => {
                    rescan = true;
                    continue;
                }
            };
            rescan |= event.need_rescan();
            if matches!(
                event.kind,
                EventKind::Access(_)
                    | EventKind::Modify(ModifyKind::Metadata(
                        MetadataKind::AccessTime
                            | MetadataKind::Permissions
                            | MetadataKind::Ownership
                    ))
            ) {
                continue;
            }
            for absolute in event.paths {
                let Ok(relative) = absolute.strip_prefix(&root) else {
                    continue;
                };
                if relative.as_os_str().is_empty() && matches!(event.kind, EventKind::Remove(_)) {
                    issue(
                        &output,
                        overflow,
                        t("The observed checkout was removed; select another checkout or refresh")
                            .into(),
                    );
                    return;
                }
                if !valid_relative(relative)
                    || relative.components().any(|c| c.as_os_str() == ".git")
                {
                    continue;
                }
                let is_known = tree.find(relative).is_some_and(|i| {
                    matches!(
                        tree.nodes[i].kind,
                        Kind::Text | Kind::Binary | Kind::Unknown | Kind::Deleted
                    )
                });
                let is_directory = tree
                    .find(relative)
                    .is_some_and(|i| tree.nodes[i].kind == Kind::Directory);
                let metadata =
                    if !is_known || !matches!(event.kind, EventKind::Modify(ModifyKind::Data(_))) {
                        absolute.symlink_metadata().ok()
                    } else {
                        None
                    };
                // Known tracked files stay visible even if an ignore rule matches.
                if !is_known && !is_directory {
                    // A temporary save sibling already renamed away isn't a
                    // separate node or a persistent inventory change.
                    let Some(meta) = &metadata else {
                        continue;
                    };
                    if global_ignored
                        .matched_path_or_any_parents(&absolute, meta.is_dir())
                        .is_ignore()
                        || ignored.as_ref().is_some_and(|i| {
                            i.matched_path_or_any_parents(&absolute, meta.is_dir())
                                .is_ignore()
                        })
                    {
                        continue;
                    }
                    // Don't report events from a symlink or repository boundary.
                    if tree.find(relative).is_some() {
                        continue;
                    }
                    let parent = relative.parent().unwrap_or(Path::new(""));
                    if !directories.contains(parent) {
                        continue;
                    }
                }
                let membership_event = matches!(
                    event.kind,
                    EventKind::Create(_)
                        | EventKind::Remove(_)
                        | EventKind::Modify(ModifyKind::Name(_))
                );
                structural |= (!is_known && !is_directory)
                    || (membership_event && metadata.is_none())
                    || (metadata.is_some()
                        && tree
                            .find(relative)
                            .is_some_and(|i| tree.nodes[i].kind == Kind::Deleted))
                    || relative
                        .file_name()
                        .is_some_and(|n| n == ".gitignore" || n == ".ignore");
                if paths.len() < MAX_EVENT_PATHS {
                    paths.insert(relative.to_path_buf());
                } else {
                    rescan = true;
                }
            }
        }
        if paths.is_empty() && !rescan {
            continue;
        }
        let observed = Instant::now();
        let graph = if connections {
            let mut total = 0;
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut updates = 0;
            let mut changed_sources = false;
            for path in &paths {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                if !source_allowed.contains(path) {
                    continue;
                }
                changed_sources = true;
                index.remove(path);
                if updates >= 256 || Instant::now() >= deadline {
                    source_limited = true;
                    continue;
                }
                if let Some(source) = read_source(&root, path, &mut total, stats) {
                    index.update(path.clone(), &source);
                } else if root.join(path).exists() {
                    source_limited = true;
                }
                updates += 1;
            }
            // A removed directory invalidates every cached source below it.
            for directory in paths.iter().filter(|p| {
                tree.find(p)
                    .is_some_and(|i| tree.nodes[i].kind == Kind::Directory)
            }) {
                if !root.join(directory).is_dir() {
                    for path in source_allowed.iter().filter(|p| p.starts_with(directory)) {
                        changed_sources = true;
                        index.remove(path);
                    }
                }
            }
            if changed_sources || rescan {
                let mut graph = index.graph();
                graph.partial |= source_limited || rescan;
                Some(Arc::new(graph))
            } else {
                None
            }
        } else {
            None
        };
        send(
            &output,
            overflow,
            Update {
                paths: paths.into_iter().collect(),
                observed,
                structural,
                graph,
                ready: true,
                issue: rescan
                    .then(|| t("Live change coverage is limited; refresh to reconcile").into()),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::Monitor;
    use crate::spider_scan;
    use muxal_core::spider::connections::Direction;
    use std::path::Path;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::{Duration, Instant};
    fn fixture() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("muxal-spider-live-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join(".ignore"), "ignored/\n").unwrap();
        std::fs::write(root.join("src/lib.rs"), "mod a; mod b;\n").unwrap();
        std::fs::write(root.join("src/a.rs"), "use crate::b::Thing;\n").unwrap();
        std::fs::write(root.join("src/b.rs"), "pub struct Thing;\n").unwrap();
        root
    }
    fn wait(monitor: &Monitor, predicate: impl Fn(&super::Update) -> bool) -> super::Update {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            for update in monitor.drain() {
                if predicate(&update) {
                    return update;
                }
            }
            assert!(Instant::now() < deadline, "observer timed out");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    #[test]
    fn activity_is_event_driven_baselined_and_stops_without_reading_sources() {
        let root = fixture();
        let snapshot =
            Arc::new(spider_scan::scan(root.clone(), Arc::new(AtomicBool::new(false))).unwrap());
        let monitor = Monitor::start(snapshot, false).unwrap();
        let initial = wait(&monitor, |u| u.ready);
        assert!(initial.paths.is_empty());
        assert_eq!(monitor.stats.source_reads.load(Ordering::Relaxed), 0);
        // Atomic editor save: write sibling, rename over the existing file.
        std::fs::write(root.join("src/a.rs.tmp"), "// changed\n").unwrap();
        std::fs::rename(root.join("src/a.rs.tmp"), root.join("src/a.rs")).unwrap();
        let update = wait(&monitor, |u| {
            u.paths.iter().any(|p| p == Path::new("src/a.rs"))
        });
        assert!(
            !update.structural,
            "an atomic replacement keeps the same inventory entry"
        );
        assert_eq!(monitor.stats.source_reads.load(Ordering::Relaxed), 0);
        let stats = monitor.stats.clone();
        drop(monitor);
        let deadline = Instant::now() + Duration::from_secs(2);
        while stats.alive.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!stats.alive.load(Ordering::Acquire));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn connections_reparse_saved_sources_and_drop_deleted_edges() {
        let root = fixture();
        let snapshot =
            Arc::new(spider_scan::scan(root.clone(), Arc::new(AtomicBool::new(false))).unwrap());
        let monitor = Monitor::start(snapshot, true).unwrap();
        let initial = wait(&monitor, |u| u.graph.is_some());
        assert!(
            initial
                .graph
                .unwrap()
                .relations(Path::new("src/a.rs"), false, 20)
                .iter()
                .any(|r| r.direction == Direction::Uses)
        );
        std::fs::write(root.join("src/a.rs"), "// import removed\n").unwrap();
        let update = wait(&monitor, |u| {
            u.paths.iter().any(|p| p == Path::new("src/a.rs")) && u.graph.is_some()
        });
        assert!(
            !update
                .graph
                .unwrap()
                .relations(Path::new("src/a.rs"), false, 20)
                .iter()
                .any(|r| r.direction == Direction::Uses)
        );
        std::fs::remove_file(root.join("src/b.rs")).unwrap();
        let update = wait(&monitor, |u| {
            u.paths.iter().any(|p| p == Path::new("src/b.rs")) && u.graph.is_some()
        });
        assert!(
            !update
                .graph
                .unwrap()
                .relations(Path::new("src/lib.rs"), false, 20)
                .iter()
                .any(|r| r.path == Path::new("src/b.rs"))
        );
        drop(monitor);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn new_files_request_inventory_refresh_and_ignored_paths_do_not_glow() {
        let root = fixture();
        let snapshot =
            Arc::new(spider_scan::scan(root.clone(), Arc::new(AtomicBool::new(false))).unwrap());
        let monitor = Monitor::start(snapshot, false).unwrap();
        wait(&monitor, |u| u.ready);
        std::fs::create_dir(root.join("ignored")).unwrap();
        std::fs::write(root.join("ignored/output"), "build output\n").unwrap();
        std::fs::write(root.join("src/new.rs"), "pub struct New;\n").unwrap();
        let update = wait(&monitor, |u| {
            u.paths.iter().any(|p| p == Path::new("src/new.rs"))
        });
        assert!(update.structural);
        assert!(!update.paths.iter().any(|p| p.starts_with("ignored")));
        assert_eq!(monitor.stats.source_reads.load(Ordering::Relaxed), 0);
        drop(monitor);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn refreshed_inventory_observes_new_directories_renames_and_removals() {
        use muxal_core::spider::refresh::InventoryRefresh;
        let root = fixture();
        let scan =
            || Arc::new(spider_scan::scan(root.clone(), Arc::new(AtomicBool::new(false))).unwrap());
        let mut monitor = Monitor::start(scan(), false).unwrap();
        wait(&monitor, |u| u.ready);
        std::fs::create_dir(root.join("src/new_folder")).unwrap();
        std::fs::write(root.join("src/new_folder/new.rs"), "// new\n").unwrap();
        let update = wait(&monitor, |u| u.structural);
        assert!(
            update
                .paths
                .iter()
                .any(|p| p == Path::new("src/new_folder"))
        );
        let mut refresh = InventoryRefresh::default();
        refresh.changed(Duration::ZERO);
        assert!(refresh.due(Duration::from_secs(1)));
        refresh.begin(Duration::from_secs(1));
        let snapshot = scan();
        refresh.finish(true, Duration::from_secs(2));
        assert!(!refresh.pending());
        assert!(
            snapshot
                .tree
                .find(Path::new("src/new_folder/new.rs"))
                .is_some()
        );
        // The replacement must watch newly discovered folders too.
        drop(monitor);
        monitor = Monitor::start(snapshot, false).unwrap();
        wait(&monitor, |u| u.ready);
        std::fs::rename(
            root.join("src/new_folder/new.rs"),
            root.join("src/new_folder/renamed.rs"),
        )
        .unwrap();
        wait(&monitor, |u| {
            u.structural
                && u.paths
                    .iter()
                    .any(|p| p == Path::new("src/new_folder/renamed.rs"))
        });
        let snapshot = scan();
        assert!(
            snapshot
                .tree
                .find(Path::new("src/new_folder/new.rs"))
                .is_none()
        );
        assert!(
            snapshot
                .tree
                .find(Path::new("src/new_folder/renamed.rs"))
                .is_some()
        );
        drop(monitor);
        monitor = Monitor::start(snapshot, false).unwrap();
        wait(&monitor, |u| u.ready);
        std::fs::remove_file(root.join("src/new_folder/renamed.rs")).unwrap();
        wait(&monitor, |u| u.structural);
        assert!(
            scan()
                .tree
                .find(Path::new("src/new_folder/renamed.rs"))
                .is_none()
        );
        assert_eq!(monitor.stats.source_reads.load(Ordering::Relaxed), 0);
        drop(monitor);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn tracked_files_keep_activity_even_after_an_ignore_rule_matches() {
        let root = fixture();
        for args in [vec!["init", "-q"], vec!["add", "src/a.rs"]] {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success());
        }
        std::fs::write(root.join(".gitignore"), "src/a.rs\n").unwrap();
        let snapshot =
            Arc::new(spider_scan::scan(root.clone(), Arc::new(AtomicBool::new(false))).unwrap());
        let monitor = Monitor::start(snapshot, false).unwrap();
        wait(&monitor, |u| u.ready);
        std::fs::write(root.join("src/a.rs"), "// tracked change\n").unwrap();
        wait(&monitor, |u| {
            u.paths.iter().any(|p| p == Path::new("src/a.rs"))
        });
        drop(monitor);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn a_blocked_ui_keeps_only_the_latest_graph_and_bounded_event_batches() {
        let pending = std::sync::Mutex::new(super::Pending::default());
        let overflow = AtomicBool::new(false);
        let first = Arc::new(muxal_core::spider::connections::Graph::default());
        let weak = Arc::downgrade(&first);
        super::send(
            &pending,
            &overflow,
            super::Update {
                paths: Vec::new(),
                observed: Instant::now(),
                structural: false,
                graph: Some(first),
                ready: true,
                issue: None,
            },
        );
        for n in 0..100 {
            let mut graph = muxal_core::spider::connections::Graph::default();
            graph.files = n;
            super::send(
                &pending,
                &overflow,
                super::Update {
                    paths: vec![format!("file{n}").into()],
                    observed: Instant::now(),
                    structural: false,
                    graph: Some(Arc::new(graph)),
                    ready: true,
                    issue: None,
                },
            );
        }
        assert!(weak.upgrade().is_none());
        let pending = pending.lock().unwrap();
        assert_eq!(pending.updates.len(), 32);
        assert_eq!(pending.graph.as_ref().unwrap().files, 99);
        assert!(pending.updates.iter().all(|u| u.graph.is_none()));
        assert!(overflow.load(Ordering::Relaxed));
    }
    #[test]
    #[ignore = "manual read-only observer benchmark against the current repository"]
    fn repository_observer_benchmark() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let snapshot = Arc::new(spider_scan::scan(root, Arc::new(AtomicBool::new(false))).unwrap());
        for connections in [false, true] {
            let started = Instant::now();
            let monitor = Monitor::start(snapshot.clone(), connections).unwrap();
            let update = wait(&monitor, |u| u.ready);
            assert!(update.issue.is_none(), "{:?}", update.issue);
            let graph = update.graph;
            eprintln!(
                "Spider observer: connections={connections}, startup={:?}, watches={}, source reads={}, files={}, links={}, unresolved={}, partial={}",
                started.elapsed(),
                monitor.stats.watches.load(Ordering::Relaxed),
                monitor.stats.source_reads.load(Ordering::Relaxed),
                graph.as_ref().map_or(0, |g| g.files),
                graph.as_ref().map_or(0, |g| g.edges),
                graph.as_ref().map_or(0, |g| g.unresolved),
                graph.as_ref().is_some_and(|g| g.partial)
            );
            let reads = monitor.stats.source_reads.load(Ordering::Relaxed);
            if !connections {
                assert_eq!(reads, 0);
            }
            let stats = monitor.stats.clone();
            drop(monitor);
            let deadline = Instant::now() + Duration::from_secs(2);
            while stats.alive.load(Ordering::Acquire) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(!stats.alive.load(Ordering::Acquire));
        }
    }
}
