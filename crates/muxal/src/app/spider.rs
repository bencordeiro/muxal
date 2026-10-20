use super::*;
use crate::spider::{
    Event as SpiderEvent, LiveState, ScopeChoice, SpiderView, State as SpiderState,
};
use crate::spider_live::Monitor;
use crate::spider_scan::{self, Cancel, Snapshot};
use muxal_core::spider::activity::Activity;
use muxal_core::spider::refresh::InventoryRefresh;
use muxal_core::spider::{Config, Scope};
use std::sync::atomic::{AtomicBool, Ordering};

struct ScanSlot {
    token: Uuid,
    cancel: Cancel,
    snapshot: Option<Arc<Snapshot>>,
    running: bool,
    pending: bool,
    automatic: bool,
}
impl Drop for ScanSlot {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

struct LiveSlot {
    monitor: Monitor,
    snapshot: Arc<Snapshot>,
    connections: bool,
    activity: bool,
    state: LiveState,
    refresh: InventoryRefresh,
}
impl LiveSlot {
    fn drain(&mut self, started: Instant) {
        for update in self.monitor.drain() {
            let at = update.observed.saturating_duration_since(started);
            if self.activity && !update.paths.is_empty() {
                Arc::make_mut(&mut self.state.activity).observe(update.paths, at);
            }
            if update.structural {
                self.refresh.changed(at);
            }
            self.state.ready |= update.ready;
            if update.graph.is_some() {
                self.state.graph = update.graph;
            }
            if update.issue.is_some() {
                self.state.issue = update.issue;
            }
        }
    }
}

pub(super) struct SpiderUi {
    pub views: HashMap<Uuid, Entity<SpiderView>>,
    scans: HashMap<PathBuf, ScanSlot>,
    pub redocks: Vec<(Uuid, SpiderState, Option<RedockAnchor>)>,
    live: HashMap<PathBuf, LiveSlot>,
    started: Instant,
    shown: HashSet<Uuid>,
}
impl Default for SpiderUi {
    fn default() -> Self {
        Self {
            views: HashMap::new(),
            scans: HashMap::new(),
            redocks: Vec::new(),
            live: HashMap::new(),
            started: Instant::now(),
            shown: HashSet::new(),
        }
    }
}

impl MuxalApp {
    pub(super) fn open_project_spider(
        &mut self,
        pid: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.settings.spider_enabled {
            return;
        }
        let owner = self
            .secondary_windows
            .iter()
            .find(|s| s.pid == pid)
            .map(|s| s.handle)
            .or(self.main_window);
        if let Some(owner) = owner
            && owner.window_id() != window.window_handle().window_id()
        {
            let app = cx.weak_entity();
            cx.defer(move |cx| {
                let _ = owner.update(cx, |_, window, cx| {
                    let _ = app.update(cx, |app, cx| app.open_project_spider(pid, window, cx));
                });
            });
            return;
        }
        self.select_project(pid, window, cx);
        let target = self
            .workspace
            .project(pid)
            .and_then(|p| p.preferred_instance());
        self.open_spider(
            target,
            PlacementMode::Split(SplitDirection::Horizontal),
            window,
            cx,
        );
    }
    pub(super) fn spider_choices(&self, pid: Uuid) -> Vec<ScopeChoice> {
        let mut choices = Vec::new();
        if let Some(project) = self.workspace.project(pid) {
            choices.push(ScopeChoice {
                scope: Scope::ProjectRoot,
                label: t("Project root").into(),
                root: project.root_path.clone(),
            });
        }
        choices.extend(
            self.workspace
                .worktrees
                .iter()
                .filter(|w| w.project_id == pid)
                .map(|w| ScopeChoice {
                    scope: Scope::Worktree(w.id),
                    label: w.name.clone(),
                    root: w.path.clone(),
                }),
        );
        choices
    }
    pub(super) fn spider_view(&self, iid: Uuid) -> Option<Entity<SpiderView>> {
        self.spider_ui.views.get(&iid).cloned().or_else(|| {
            self.popouts.get(&iid).and_then(|p| match &p.view {
                PaneView::Spider(v) => Some(v.clone()),
                _ => None,
            })
        })
    }
    pub(super) fn spider_views(&self) -> Vec<(Uuid, Entity<SpiderView>)> {
        self.spider_ui
            .views
            .iter()
            .map(|(id, v)| (*id, v.clone()))
            .chain(self.popouts.iter().filter_map(|(id, p)| match &p.view {
                PaneView::Spider(v) => Some((*id, v.clone())),
                _ => None,
            }))
            .collect()
    }
    pub(super) fn attach_spider(
        &mut self,
        iid: Uuid,
        view: &Entity<SpiderView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe_in(view, window, move |this, source, event, window, cx| {
            if this
                .spider_view(iid)
                .is_none_or(|v| v.entity_id() != source.entity_id())
            {
                return;
            }
            match event {
                SpiderEvent::Scan(refresh) => this.scan_spider(iid, *refresh, cx),
                SpiderEvent::Config => {
                    if !this.settings.spider_enabled {
                        return;
                    }
                    let config = source.read(cx).state.config.clone();
                    if let Some(instance) = this.workspace.instance_mut(iid) {
                        instance.spider = Some(config);
                    }
                    this.sync_spiders(cx);
                    this.persist();
                    cx.notify();
                }
                SpiderEvent::Open(path) | SpiderEvent::Reveal(path) => {
                    if !this.settings.spider_enabled {
                        return;
                    }
                    let Some(root) = source.read(cx).state.root() else {
                        return;
                    };
                    let reveal = matches!(event, SpiderEvent::Reveal(_));
                    this.navigate_spider(iid, root, path.clone(), reveal, window, cx);
                }
                SpiderEvent::Settings => {
                    let Some(handle) = this.main_window else {
                        return;
                    };
                    let app = cx.weak_entity();
                    cx.defer(move |cx| {
                        let _ = handle.update(cx, |_, window, cx| {
                            let _ = app.update(cx, |app, cx| {
                                if !app.show_settings {
                                    app.toggle_settings(window, cx);
                                }
                                app.settings_ui.section = SettingsSection::Behavior;
                                cx.notify();
                                window.activate_window();
                            });
                        });
                    });
                }
                SpiderEvent::Overlay => cx.notify(),
            }
        })
        .detach();
        cx.observe(view, |_, _, cx| cx.notify()).detach();
    }
    pub(super) fn spawn_spider(&mut self, iid: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        if self.spider_view(iid).is_some() {
            return;
        }
        let Some(instance) = self.workspace.instance(iid) else {
            return;
        };
        let state = SpiderState::new(
            instance.spider.clone().unwrap_or_default(),
            self.spider_choices(instance.project_id),
            self.settings.spider_enabled,
        );
        let view = cx.new(|cx| SpiderView::new(state, window, cx));
        self.attach_spider(iid, &view, window, cx);
        self.spider_ui.views.insert(iid, view);
    }
    pub(super) fn open_spider(
        &mut self,
        anchor: Option<Uuid>,
        placement: PlacementMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.settings.spider_enabled {
            return;
        }
        let pid = anchor
            .and_then(|id| self.workspace.instance(id).map(|i| i.project_id))
            .or_else(|| self.secondary_pid_for(window))
            .or(self.workspace.active_project);
        let Some(pid) = pid else {
            return;
        };
        let scope = anchor
            .and_then(|id| self.workspace.instance(id))
            .and_then(|i| {
                i.spider
                    .as_ref()
                    .map(|c| c.scope.clone())
                    .or_else(|| i.worktree_id.map(Scope::Worktree))
            })
            .unwrap_or_default();
        let instance = Instance::spider(
            pid,
            Config {
                scope,
                ..Config::default()
            },
        );
        let iid = instance.id;
        if !self.place_resource_instance(pid, instance, anchor, Some(placement)) {
            return;
        }
        self.spawn_spider(iid, window, cx);
        self.focus_instance(iid, window, cx);
        self.persist();
        cx.notify();
    }
    pub(super) fn scan_spider(&mut self, iid: Uuid, refresh: bool, cx: &mut Context<Self>) {
        if !self.settings.spider_enabled {
            return;
        }
        let Some(view) = self.spider_view(iid) else {
            return;
        };
        let Some(root) = view.read(cx).state.root() else {
            return;
        };
        if !refresh && let Some(slot) = self.spider_ui.scans.get(&root) {
            if let Some(snapshot) = &slot.snapshot {
                let snapshot = snapshot.clone();
                view.update(cx, |v, cx| v.receive(Ok(snapshot), cx));
            } else if !slot.running && !slot.pending {
                view.update(cx, |v, cx| {
                    v.receive(Err(t("Refresh to retry the scan").into()), cx)
                });
            }
            return;
        }
        self.queue_spider_scan(root, false, cx);
    }
    fn queue_spider_scan(&mut self, root: PathBuf, automatic: bool, cx: &mut Context<Self>) {
        if automatic
            && self
                .spider_ui
                .scans
                .get(&root)
                .is_some_and(|s| s.running || s.pending)
        {
            return;
        }
        if let Some(slot) = self.spider_ui.scans.get_mut(&root)
            && slot.running
        {
            slot.automatic = false;
            slot.pending = true;
            slot.cancel.store(true, Ordering::Relaxed);
            return;
        }
        let previous = self
            .spider_ui
            .scans
            .remove(&root)
            .and_then(|s| s.snapshot.clone());
        self.spider_ui.scans.insert(
            root,
            ScanSlot {
                token: Uuid::new_v4(),
                cancel: Arc::new(AtomicBool::new(false)),
                snapshot: previous,
                running: false,
                pending: true,
                automatic,
            },
        );
        self.pump_spider_scans(cx);
    }
    fn pump_spider_scans(&mut self, cx: &mut Context<Self>) {
        if !self.settings.spider_enabled {
            return;
        }
        let active = self.spider_ui.scans.values().filter(|s| s.running).count();
        let ready: Vec<_> = self
            .spider_ui
            .scans
            .iter()
            .filter(|(_, s)| !s.running && s.pending)
            .take(2usize.saturating_sub(active))
            .map(|(root, _)| root.clone())
            .collect();
        for root in ready {
            let slot = self.spider_ui.scans.get_mut(&root).unwrap();
            slot.running = true;
            slot.pending = false;
            let token = slot.token;
            let cancel = slot.cancel.clone();
            if let Some(live) = self.spider_ui.live.get_mut(&root) {
                live.drain(self.spider_ui.started);
                live.refresh.begin(self.spider_ui.started.elapsed());
            }
            log::debug!("Spider scan started: {token}");
            cx.spawn(async move |app: WeakEntity<Self>, cx| {
                let scan_root = root.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { spider_scan::scan(scan_root, cancel).map(Arc::new) })
                    .await;
                let _ = app.update(cx, |app, cx| {
                    if !app.settings.spider_enabled {
                        return;
                    }
                    let Some(slot) = app
                        .spider_ui
                        .scans
                        .get_mut(&root)
                        .filter(|s| s.token == token)
                    else {
                        return;
                    };
                    slot.running = false;
                    log::debug!("Spider scan finished: {token}, success={}", result.is_ok());
                    // Manual Refresh may supersede this generation. Hiding or
                    // disabling the final activity consumer cancels auto work.
                    let discarded = slot.cancel.load(Ordering::Relaxed);
                    if let Some(live) = app.spider_ui.live.get_mut(&root) {
                        live.drain(app.spider_ui.started);
                        let now = app.spider_ui.started.elapsed();
                        live.refresh.finish(result.is_ok() || discarded, now);
                        if discarded {
                            live.refresh.changed(now);
                        }
                    }
                    if slot.pending {
                        slot.token = Uuid::new_v4();
                        slot.cancel = Arc::new(AtomicBool::new(false));
                    } else if !discarded {
                        if let Ok(snapshot) = &result {
                            slot.snapshot = Some(snapshot.clone());
                        }
                        for (_, view) in app.spider_views() {
                            if view.read(cx).state.root().as_ref() == Some(&root) {
                                view.update(cx, |v, cx| v.receive(result.clone(), cx));
                            }
                        }
                    }
                    app.pump_spider_scans(cx);
                    app.sync_spider_live(cx);
                    cx.notify();
                });
            })
            .detach();
        }
    }
    pub(super) fn prune_spider_scans(&mut self, cx: &App) {
        let roots: HashSet<_> = self
            .spider_views()
            .iter()
            .filter_map(|(_, v)| v.read(cx).state.root())
            .collect();
        self.spider_ui
            .scans
            .retain(|root, _| self.settings.spider_enabled && roots.contains(root));
    }
    pub(super) fn sync_spiders(&mut self, cx: &mut Context<Self>) {
        if !self.settings.spider_enabled {
            self.spider_ui.scans.clear();
            self.spider_ui.live.clear();
            for (_, state, _) in &mut self.spider_ui.redocks {
                state.enabled = false;
                state.snapshot = None;
            }
        }
        for (iid, view) in self.spider_views() {
            let choices = self
                .workspace
                .instance(iid)
                .map(|i| self.spider_choices(i.project_id))
                .unwrap_or_default();
            let enabled = self.settings.spider_enabled;
            view.update(cx, |v, cx| {
                if v.state.choices != choices {
                    v.set_choices(choices, cx);
                }
                v.set_enabled(enabled, cx);
            });
        }
        self.prune_spider_scans(cx);
        self.pump_spider_scans(cx);
        self.sync_spider_live(cx);
    }
    fn visible_spider_ids(&self) -> HashSet<Uuid> {
        let mut shown = HashSet::new();
        let mut projects: Vec<_> = self.secondary_windows.iter().map(|s| s.pid).collect();
        if !self.show_dashboard
            && !self.show_workspace_selector
            && let Some(pid) = self.workspace.active_project
            && !projects.contains(&pid)
        {
            projects.push(pid);
        }
        for pid in projects {
            if let Some(layout) = self.workspace.project(pid).and_then(|p| p.layout.as_ref()) {
                shown.extend(muxal_core::spider::visible_tabs(
                    layout,
                    self.active_instance,
                    self.maximized,
                ));
            }
        }
        shown.extend(
            self.popouts
                .iter()
                .filter_map(|(iid, p)| matches!(p.view, PaneView::Spider(_)).then_some(*iid)),
        );
        shown.retain(|id| {
            self.spider_ui.views.contains_key(id)
                || self
                    .popouts
                    .get(id)
                    .is_some_and(|p| matches!(&p.view, PaneView::Spider(_)))
        });
        shown
    }
    pub(super) fn sync_spider_visibility(&mut self, cx: &mut Context<Self>) {
        if !self.settings.spider_enabled {
            return;
        }
        let shown = self.visible_spider_ids();
        if shown != self.spider_ui.shown {
            self.spider_ui.shown = shown;
            let app = cx.weak_entity();
            cx.defer(move |cx| {
                let _ = app.update(cx, |app, cx| app.sync_spider_live(cx));
            });
        }
    }
    fn sync_spider_live(&mut self, cx: &mut Context<Self>) {
        let shown = self.visible_spider_ids();
        self.spider_ui.shown = shown.clone();
        let views = self.spider_views();
        let mut wanted = std::collections::BTreeMap::<PathBuf, (Arc<Snapshot>, bool, bool)>::new();
        if self.settings.spider_enabled {
            for (iid, view) in &views {
                let state = &view.read(cx).state;
                if !shown.contains(iid)
                    || !state.enabled
                    || (!state.config.activity && !state.config.connections)
                {
                    continue;
                }
                if let Some(snapshot) = &state.snapshot
                    && state.root().as_ref() == Some(&snapshot.root)
                {
                    let entry = wanted.entry(snapshot.root.clone()).or_insert((
                        snapshot.clone(),
                        false,
                        false,
                    ));
                    entry.1 |= state.config.activity;
                    entry.2 |= state.config.connections;
                }
            }
        }
        // Bound both native watch registrations and background parsing across
        // projects. Extra consumers get an explicit availability message.
        let allowed: HashSet<_> = wanted.keys().take(4).cloned().collect();
        for (root, scan) in &mut self.spider_ui.scans {
            if scan.automatic
                && (!allowed.contains(root)
                    || !wanted.get(root).is_some_and(|(_, activity, _)| *activity))
            {
                scan.pending = false;
                scan.cancel.store(true, Ordering::Relaxed);
            }
        }
        self.spider_ui.live.retain(|root, _| allowed.contains(root));
        let mut failures = HashMap::new();
        let mut refresh_roots = Vec::new();
        for (root, (snapshot, activity, connections)) in &wanted {
            if !allowed.contains(root) {
                continue;
            }
            let replace = self.spider_ui.live.get(root).is_none_or(|slot| {
                !Arc::ptr_eq(&slot.snapshot, snapshot) || slot.connections != *connections
            });
            if replace {
                let previous = self
                    .spider_ui
                    .live
                    .remove(root)
                    .map(|mut slot| {
                        // Drain before replacing a watch generation: events
                        // received during the inventory scan still need a turn.
                        slot.drain(self.spider_ui.started);
                        (slot.state.activity, slot.refresh)
                    })
                    .unwrap_or_default();
                match Monitor::start(snapshot.clone(), *connections) {
                    Ok(monitor) => {
                        self.spider_ui.live.insert(
                            root.clone(),
                            LiveSlot {
                                monitor,
                                snapshot: snapshot.clone(),
                                connections: *connections,
                                activity: *activity,
                                refresh: previous.1,
                                state: LiveState {
                                    activity: if *activity {
                                        previous.0
                                    } else {
                                        Arc::new(Activity::default())
                                    },
                                    ..LiveState::default()
                                },
                            },
                        );
                    }
                    Err(_) => {
                        failures.insert(
                            root.clone(),
                            t("Live observation could not start; manual Refresh still works")
                                .to_string(),
                        );
                    }
                }
            }
            let Some(slot) = self.spider_ui.live.get_mut(root) else {
                continue;
            };
            slot.activity = *activity;
            slot.drain(self.spider_ui.started);
            if slot.state.ready && !slot.monitor.is_alive() && slot.state.issue.is_none() {
                slot.state.issue = Some(t("Live observation stopped; refresh to retry").into());
            }
            let now = Duration::from_secs(self.spider_ui.started.elapsed().as_secs());
            if !*activity && !slot.state.activity.is_empty() {
                slot.state.activity = Arc::new(Activity::default());
            }
            if slot.state.now != now && !slot.state.activity.is_empty() {
                Arc::make_mut(&mut slot.state.activity).expire(now);
            }
            slot.state.now = now;
            slot.state.structural = slot.refresh.pending();
            slot.state.refresh_failed = slot.refresh.failed();
            if *activity && slot.refresh.due(self.spider_ui.started.elapsed()) {
                refresh_roots.push(root.clone());
            }
        }
        for root in refresh_roots {
            self.queue_spider_scan(root, true, cx);
        }
        for (iid, view) in views {
            let state = &view.read(cx).state;
            let enabled = shown.contains(&iid)
                && self.settings.spider_enabled
                && state.enabled
                && (state.config.activity || state.config.connections);
            let live = if enabled {
                state.root().and_then(|root| {
                    if let Some(slot) = self.spider_ui.live.get(&root) { Some(slot.state.clone()) }
                    else if wanted.contains_key(&root) { Some(LiveState { ready: true, issue: Some(failures.get(&root).cloned().unwrap_or_else(|| t("Live overlay limit reached; close another live map or use Refresh").into())), ..LiveState::default() }) }
                    else { None }
                }).unwrap_or_default()
            } else {
                LiveState::default()
            };
            view.update(cx, |v, cx| v.set_live(live, cx));
        }
    }
    pub(super) fn rebuild_spiders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pid = self
            .secondary_pid_for(window)
            .or(self.workspace.active_project);
        let mut waiting = Vec::new();
        for (iid, mut state, redock) in std::mem::take(&mut self.spider_ui.redocks) {
            let Some(instance) = self.workspace.instance(iid).cloned() else {
                continue;
            };
            if Some(instance.project_id) != pid {
                waiting.push((iid, state, redock));
                continue;
            }
            state.enabled = self.settings.spider_enabled;
            state.choices = self.spider_choices(instance.project_id);
            let view = cx.new(|cx| SpiderView::new(state, window, cx));
            self.attach_spider(iid, &view, window, cx);
            self.spider_ui.views.insert(iid, view);
            if let Some(redock) = redock {
                self.redock_into_layout(iid, instance.project_id, redock, cx);
            }
        }
        self.spider_ui.redocks.extend(waiting);
    }
    pub(super) fn navigate_spider(
        &mut self,
        iid: Uuid,
        root: PathBuf,
        path: PathBuf,
        reveal: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pid) = self.workspace.instance(iid).map(|i| i.project_id) else {
            return;
        };
        if !path.as_os_str().is_empty() && !muxal_core::spider::valid_relative(&path) {
            return;
        }
        let full = root.join(&path);
        if let (Ok(base), Ok(file)) = (root.canonicalize(), full.canonicalize()) {
            if !file.starts_with(base) {
                return;
            }
        } else {
            return;
        }
        // A pop-out map sends resources back to the project's owning window.
        if !self.project_is_shown_in_window(pid, window) {
            let handle = self
                .secondary_windows
                .iter()
                .find(|s| s.pid == pid)
                .map(|s| s.handle)
                .or(self.main_window);
            let Some(handle) = handle else {
                return;
            };
            let app = cx.weak_entity();
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = app.update(cx, |app, cx| {
                        app.select_project(pid, window, cx);
                        app.navigate_spider(iid, root, path, reveal, window, cx);
                        window.activate_window();
                    });
                });
            });
            return;
        }
        if reveal {
            self.show_file_browser = true;
            self.show_memory = false;
            self.file_browser_root_override = Some((pid, root));
            self.load_file_browser(pid, cx);
            let mut parent = full.parent();
            while let Some(dir) = parent {
                self.file_browser_expanded.insert(dir.to_path_buf());
                parent = dir.parent();
            }
            if full.is_dir() {
                self.file_browser_expanded.insert(full);
            }
            self.rebuild_file_browser_rows(cx);
        } else if full.is_file() {
            let target = self.workspace.project(pid).and_then(|p| {
                if p.instances().contains(&iid) {
                    Some(iid)
                } else {
                    p.preferred_instance()
                }
            });
            self.open_editor_at(pid, Some(full), target, window, cx);
        }
        cx.notify();
    }
}
