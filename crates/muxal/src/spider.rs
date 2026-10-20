//! Native Spider pane. The view owns interaction; the app owns scans/navigation.
use crate::i18n::{t, tf};
use crate::spider_scan::Snapshot;
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme, Disableable, Selectable, Sizable, StyledExt};
use muxal_core::spider::activity::Activity;
use muxal_core::spider::connections::{Direction, Graph, Relation};
use muxal_core::spider::navigation::{Effect, MapContext, Navigation, Viewport, fit_map};
use muxal_core::spider::{self, Camera, Config, Kind, Layout, Overlay, Scope};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Weak};
use std::time::Duration;

#[derive(Clone, Default)]
pub struct LiveState {
    pub activity: Arc<Activity>,
    pub graph: Option<Arc<Graph>>,
    pub now: Duration,
    pub ready: bool,
    pub issue: Option<String>,
    pub structural: bool,
    pub refresh_failed: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ScopeChoice {
    pub scope: Scope,
    pub label: String,
    pub root: PathBuf,
}

#[derive(Clone)]
pub struct State {
    pub config: Config,
    pub choices: Vec<ScopeChoice>,
    pub navigation: Navigation,
    pub camera: Camera,
    pub snapshot: Option<Arc<Snapshot>>,
    pub enabled: bool,
    pub query: String,
}
impl State {
    pub fn new(config: Config, choices: Vec<ScopeChoice>, enabled: bool) -> Self {
        Self {
            config,
            choices,
            navigation: Navigation::default(),
            camera: Camera::default(),
            snapshot: None,
            enabled,
            query: String::new(),
        }
    }
    pub fn root(&self) -> Option<PathBuf> {
        self.choices
            .iter()
            .find(|c| c.scope == self.config.scope)
            .map(|c| c.root.clone())
    }
}

#[derive(Clone)]
pub enum Event {
    Scan(bool),
    Config,
    Open(PathBuf),
    Reveal(PathBuf),
    Settings,
    Overlay,
}

#[derive(Default)]
struct ActivityProjection {
    snapshot: Weak<Snapshot>,
    activity: Weak<Activity>,
    shown: Vec<usize>,
    value: Arc<Activity>,
}
impl ActivityProjection {
    fn get(
        &mut self,
        snapshot: &Arc<Snapshot>,
        activity: &Arc<Activity>,
        shown: &[usize],
    ) -> Arc<Activity> {
        let snapshot_key = Arc::downgrade(snapshot);
        let activity_key = Arc::downgrade(activity);
        if !self.snapshot.ptr_eq(&snapshot_key)
            || !self.activity.ptr_eq(&activity_key)
            || self.shown != shown
        {
            self.value = Arc::new(activity.visible(&snapshot.tree, shown));
            self.snapshot = snapshot_key;
            self.activity = activity_key;
            self.shown = shown.to_vec();
        }
        self.value.clone()
    }
}

pub struct SpiderView {
    pub state: State,
    pub loading: bool,
    pub error: Option<String>,
    requested: bool,
    focus: FocusHandle,
    input: Entity<InputState>,
    options: bool,
    inspect: bool,
    search: bool,
    search_index: usize,
    list_page: usize,
    camera: Rc<Cell<Camera>>,
    fit: Rc<Cell<bool>>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    geometry: Rc<RefCell<Vec<(usize, spider::Point)>>>,
    pointer_down: Option<Point<Pixels>>,
    drag_origin: spider::Point,
    dragged: bool,
    hovered: Option<PathBuf>,
    live: LiveState,
    relations: Vec<Relation>,
    relation_key: Option<(usize, PathBuf, bool)>,
    activity_projection: Rc<RefCell<ActivityProjection>>,
}
impl EventEmitter<Event> for SpiderView {}
impl Focusable for SpiderView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl SpiderView {
    pub fn new(state: State, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t("Find a file or folder…"))
                .default_value(state.query.clone())
        });
        cx.subscribe_in(&input, window, |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.state.query = input.read(cx).value().to_string();
                this.search_index = 0;
                cx.notify();
            }
        })
        .detach();
        Self {
            camera: Rc::new(Cell::new(state.camera)),
            fit: Rc::new(Cell::new(state.snapshot.is_none())),
            state,
            input,
            focus: cx.focus_handle(),
            loading: false,
            error: None,
            requested: false,
            options: false,
            inspect: false,
            search: false,
            search_index: 0,
            list_page: 0,
            bounds: Rc::new(Cell::new(Bounds::default())),
            geometry: Rc::new(RefCell::new(Vec::new())),
            pointer_down: None,
            drag_origin: spider::Point::default(),
            dragged: false,
            hovered: None,
            live: LiveState::default(),
            relations: Vec::new(),
            relation_key: None,
            activity_projection: Rc::new(RefCell::new(ActivityProjection::default())),
        }
    }
    pub fn capture(&self) -> State {
        let mut state = self.state.clone();
        state.camera = self.camera.get();
        state
    }
    pub fn menu_open(&self) -> bool {
        self.options
    }
    pub fn set_live(&mut self, live: LiveState, cx: &mut Context<Self>) {
        let graph_id = |graph: &Option<Arc<Graph>>| graph.as_ref().map(|g| Arc::as_ptr(g) as usize);
        let changed = graph_id(&self.live.graph) != graph_id(&live.graph)
            || ((!self.live.activity.is_empty() || !live.activity.is_empty())
                && !Arc::ptr_eq(&self.live.activity, &live.activity))
            || self.live.ready != live.ready
            || self.live.issue != live.issue
            || self.live.structural != live.structural
            || self.live.refresh_failed != live.refresh_failed
            || (self.state.config.activity
                && !live.activity.is_empty()
                && self.live.now != live.now);
        if graph_id(&self.live.graph) != graph_id(&live.graph) {
            self.relation_key = None;
            self.relations.clear();
        }
        self.live = live;
        if changed {
            cx.notify();
        }
    }
    fn sync_relations(&mut self) {
        let key = self
            .live
            .graph
            .as_ref()
            .filter(|_| self.state.config.connections)
            .map(|g| {
                (
                    Arc::as_ptr(g) as usize,
                    self.state.navigation.selected.clone(),
                    self.state.config.indirect,
                )
            });
        if key != self.relation_key {
            self.relations = self
                .live
                .graph
                .as_ref()
                .filter(|_| key.is_some())
                .map(|g| {
                    g.relations(
                        &self.state.navigation.selected,
                        self.state.config.indirect,
                        100,
                    )
                })
                .unwrap_or_default();
            self.relation_key = key;
        }
    }
    pub fn set_choices(&mut self, choices: Vec<ScopeChoice>, cx: &mut Context<Self>) {
        let old = self.state.root();
        self.state.choices = choices;
        if self.state.root() != old {
            self.state.snapshot = None;
            self.state.navigation = Navigation::default();
            self.camera.set(Camera::default());
            self.fit.set(true);
            self.live = LiveState::default();
            self.geometry.borrow_mut().clear();
            self.requested = false;
            self.loading = false;
        }
        cx.notify();
    }
    pub fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.state.enabled == enabled {
            return;
        }
        self.state.enabled = enabled;
        self.requested = false;
        self.loading = false;
        if !enabled {
            self.state.snapshot = None;
            self.geometry.borrow_mut().clear();
            self.options = false;
            self.search = false;
            self.inspect = false;
            self.live = LiveState::default();
        }
        cx.notify();
    }
    pub fn receive(&mut self, snapshot: Result<Arc<Snapshot>, String>, cx: &mut Context<Self>) {
        self.loading = false;
        match snapshot {
            Ok(snapshot) => {
                if self.state.root().as_ref() != Some(&snapshot.root) || !self.state.enabled {
                    return;
                }
                if self.state.navigation.reconcile(&snapshot.tree) {
                    self.fit.set(true);
                }
                self.geometry.borrow_mut().clear();
                if self
                    .state
                    .snapshot
                    .as_ref()
                    .is_none_or(|old| !Arc::ptr_eq(old, &snapshot))
                {
                    self.live = LiveState::default();
                }
                self.state.snapshot = Some(snapshot);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }
    fn request(&mut self, refresh: bool, cx: &mut Context<Self>) {
        if !self.state.enabled || self.state.root().is_none() {
            return;
        }
        self.requested = true;
        self.loading = true;
        self.error = None;
        cx.emit(Event::Scan(refresh));
        cx.notify();
    }
    fn change_config(&mut self, cx: &mut Context<Self>) {
        cx.emit(Event::Config);
        cx.notify();
    }
    fn select(&mut self, id: usize, cx: &mut Context<Self>) {
        if let Some(snapshot) = &self.state.snapshot
            && let Some(node) = snapshot.tree.nodes.get(id)
        {
            self.state.navigation.selected = node.path.clone();
            self.list_page = 0;
        }
        cx.notify();
    }
    fn current_path_id(&self, root: &std::path::Path, path: &std::path::Path) -> Option<usize> {
        let snapshot = self.state.snapshot.as_ref()?;
        (snapshot.root == root && self.state.root().as_deref() == Some(root))
            .then(|| snapshot.tree.find(path))
            .flatten()
    }
    fn navigation_effect(&mut self, effect: Effect, cx: &mut Context<Self>) {
        match effect {
            Effect::None => return,
            Effect::Fit => self.fit.set(true),
            Effect::Restore(camera) => {
                self.camera.set(camera);
                self.fit.set(false);
            }
        }
        self.geometry.borrow_mut().clear();
        self.list_page = 0;
        cx.notify();
    }
    fn viewport(&self) -> Viewport {
        Viewport {
            width: f32::from(self.bounds.get().size.width),
            height: f32::from(self.bounds.get().size.height),
        }
    }
    fn go_back(&mut self, cx: &mut Context<Self>) {
        if let Some(snapshot) = &self.state.snapshot {
            let effect = self.state.navigation.back(&snapshot.tree);
            self.navigation_effect(effect, cx);
        }
    }
    fn toggle_branch(&mut self, cx: &mut Context<Self>) {
        if let Some(snapshot) = &self.state.snapshot {
            let context = MapContext {
                tree: &snapshot.tree,
                view: self.viewport(),
                layout: self.state.config.layout,
            };
            let effect = self.state.navigation.toggle(context, self.camera.get());
            self.navigation_effect(effect, cx);
        }
    }
    fn search_results(&self) -> Vec<usize> {
        self.state
            .snapshot
            .as_ref()
            .map(|s| s.tree.matching(&self.state.query, 500))
            .unwrap_or_default()
    }
    fn reveal_match(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = &self.state.snapshot {
            let path = snapshot.tree.nodes[id].path.clone();
            let related: Vec<_> = self
                .live
                .graph
                .as_ref()
                .filter(|_| self.state.config.connections)
                .map(|graph| graph.relations(&path, self.state.config.indirect, 48))
                .unwrap_or_default()
                .iter()
                .filter_map(|r| snapshot.tree.find(&r.path))
                .collect();
            let context = MapContext {
                tree: &snapshot.tree,
                view: self.viewport(),
                layout: self.state.config.layout,
            };
            let effect = self
                .state
                .navigation
                .reveal(context, path, &related, self.camera.get());
            self.navigation_effect(effect, cx);
        }
        self.search = false;
        window.focus(&self.focus, cx);
        cx.notify();
    }
    fn pointer(&self, position: Point<Pixels>) -> spider::Point {
        let b = self.bounds.get();
        spider::Point {
            x: f32::from(position.x - b.origin.x),
            y: f32::from(position.y - b.origin.y),
        }
    }
    fn hit(&self, position: Point<Pixels>) -> Option<usize> {
        let snapshot = self.state.snapshot.as_ref()?;
        let b = self.bounds.get();
        spider::hit_test(
            &self.geometry.borrow(),
            self.pointer(position),
            &snapshot.tree,
            self.camera.get(),
            f32::from(b.size.width),
            f32::from(b.size.height),
        )
    }
    fn open_selected(&mut self, reveal: bool, cx: &mut Context<Self>) {
        let Some(snapshot) = &self.state.snapshot else {
            return;
        };
        let Some(id) = snapshot.tree.find(&self.state.navigation.selected) else {
            return;
        };
        let node = &snapshot.tree.nodes[id];
        if reveal {
            cx.emit(Event::Reveal(node.path.clone()));
        } else if matches!(node.kind, Kind::Text | Kind::Binary | Kind::Unknown) {
            cx.emit(Event::Open(node.path.clone()));
        } else if node.kind == Kind::Directory {
            self.toggle_branch(cx);
        }
    }
    fn options(&self, cx: &mut Context<Self>) -> AnyElement {
        let config = self.state.config.clone();
        let mut box_ = div()
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .px_2()
            .py_1()
            .bg(cx.theme().secondary);
        box_ = box_.child(
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .child(
                    Button::new("radial")
                        .xsmall()
                        .ghost()
                        .label(t("Radial"))
                        .selected(config.layout == Layout::Radial)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.state.config.layout = Layout::Radial;
                            this.fit.set(true);
                            this.change_config(cx);
                        })),
                )
                .child(
                    Button::new("tree")
                        .xsmall()
                        .ghost()
                        .label(t("Tree"))
                        .selected(config.layout == Layout::Tree)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.state.config.layout = Layout::Tree;
                            this.fit.set(true);
                            this.change_config(cx);
                        })),
                )
                .child(
                    Button::new("structure")
                        .xsmall()
                        .ghost()
                        .label(t("Structure"))
                        .selected(config.overlay == Overlay::Structure)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.state.config.overlay = Overlay::Structure;
                            this.change_config(cx);
                        })),
                )
                .child(
                    Button::new("activity")
                        .xsmall()
                        .ghost()
                        .label(t("Git activity"))
                        .selected(config.overlay == Overlay::GitActivity)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.state.config.overlay = Overlay::GitActivity;
                            this.change_config(cx);
                        })),
                ),
        );
        box_ = box_.child(
            div().flex().flex_wrap().gap_1()
                .child(Button::new("recent-activity").xsmall().ghost().label(t("Recent activity"))
                    .tooltip(t("Glow for 25 seconds after observed file changes; does not identify an agent"))
                    .selected(config.activity)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.config.activity = !this.state.config.activity;
                        this.change_config(cx);
                    })))
                .child(Button::new("code-connections").xsmall().ghost().label(t("Code connections"))
                    .tooltip(t("Rust module and import links for the selected file"))
                    .selected(config.connections)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.config.connections = !this.state.config.connections;
                        if this.state.config.connections { this.inspect = true; }
                        this.change_config(cx);
                    })))
                .child(Button::new("indirect-connections").xsmall().ghost().label(t("Indirect"))
                    .tooltip(t("Include indirect connections up to three steps away"))
                    .disabled(!config.connections).selected(config.indirect)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.config.indirect = !this.state.config.indirect;
                        this.change_config(cx);
                    }))),
        );
        if self.state.choices.is_empty()
            || (self.state.choices.len() == 1 && self.state.choices[0].scope == config.scope)
        {
            return box_.into_any_element();
        }
        let mut scopes = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .child(div().text_xs().child(t("Checkout:")));
        for (i, choice) in self.state.choices.iter().enumerate() {
            let scope = choice.scope.clone();
            scopes = scopes.child(
                Button::new(SharedString::from(format!("scope-{i}")))
                    .xsmall()
                    .ghost()
                    .label(choice.label.clone())
                    .selected(config.scope == choice.scope)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.state.config.scope = scope.clone();
                        this.state.snapshot = None;
                        this.live = LiveState::default();
                        this.geometry.borrow_mut().clear();
                        this.state.navigation = Navigation::default();
                        this.camera.set(Camera::default());
                        this.fit.set(true);
                        this.requested = false;
                        this.change_config(cx);
                        this.request(true, cx);
                    })),
            );
        }
        box_.child(scopes).into_any_element()
    }
    fn inspector(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(snapshot) = &self.state.snapshot else {
            return div().into_any_element();
        };
        let tree = &snapshot.tree;
        let id = tree.find(&self.state.navigation.selected).unwrap_or(0);
        let node = &tree.nodes[id];
        let mut list = div()
            .id("spider-inspector")
            .max_h(relative(0.45))
            .min_h_0()
            .overflow_y_scroll()
            .p_2()
            .text_xs()
            .flex()
            .flex_col()
            .gap_1()
            .bg(cx.theme().secondary);
        let description = tf(
            "{files} files · {lines} text lines{partial} · {bytes} bytes",
            &[
                ("files", &node.files.to_string()),
                ("lines", &node.lines.to_string()),
                ("partial", if node.incomplete { "+" } else { "" }),
                ("bytes", &node.bytes.to_string()),
            ],
        );
        list = list.child(div().child(description));
        let kind = match node.kind {
            Kind::Directory => t("Folder"),
            Kind::Text => t("Text file"),
            Kind::Binary => t("Binary file"),
            Kind::Symlink => t("Symbolic link — not followed"),
            Kind::Boundary => t("Repository or filesystem boundary"),
            Kind::Deleted => t("Deleted file"),
            Kind::Unknown => t("Text size unavailable"),
        };
        list = list.child(div().child(kind));
        if self.state.config.activity && self.live.activity.changed_under(&node.path, self.live.now)
        {
            list = list.child(div().child(if node.kind == Kind::Directory {
                t("Recent file changes inside this folder")
            } else {
                t("Observed a file change within the last 25 seconds")
            }));
        }
        if let Some(time) = node.committed_at {
            let when = chrono::DateTime::from_timestamp(time, 0)
                .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
                .unwrap_or_default();
            list = list.child(div().child(tf("Latest commit: {time}", &[("time", &when)])));
        } else {
            list = list.child(div().child(t("No recent commit found in scanned history")));
        }
        list = list.child(div().child(tf(
            "{recent} recent files · {changed} changed files",
            &[
                ("recent", &node.recent_files.to_string()),
                ("changed", &node.changed_files.to_string()),
            ],
        )));
        if let Some(status) = &node.change {
            list = list.child(div().child(tf("Git status: {status}", &[("status", status)])));
        }
        list = list.child(
            div()
                .flex()
                .gap_1()
                .child(
                    Button::new("open-spider-file")
                        .xsmall()
                        .ghost()
                        .label(if node.kind == Kind::Directory {
                            if self.state.navigation.selected == self.state.navigation.focused
                                && self.state.navigation.can_go_back()
                            {
                                t("Back")
                            } else {
                                t("Expand / collapse")
                            }
                        } else {
                            t("Open file")
                        })
                        .disabled(!matches!(
                            node.kind,
                            Kind::Directory | Kind::Text | Kind::Binary | Kind::Unknown
                        ))
                        .on_click(cx.listener(|this, _, _, cx| this.open_selected(false, cx))),
                )
                .child(
                    Button::new("reveal-spider-file")
                        .xsmall()
                        .ghost()
                        .label(t("Reveal in files"))
                        .on_click(cx.listener(|this, _, _, cx| this.open_selected(true, cx))),
                ),
        );
        for child in node.children.iter().skip(self.list_page * 60).take(60) {
            let child = *child;
            let relative = tree.nodes[child].path.clone();
            let expected_root = snapshot.root.clone();
            let name = spider::display_path(std::path::Path::new(
                tree.nodes[child].path.file_name().unwrap_or_default(),
            ));
            list = list.child(
                div()
                    .id(SharedString::from(format!("child-{child}")))
                    .cursor_pointer()
                    .text_ellipsis()
                    .hover(|s| s.bg(cx.theme().accent))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(id) = this.current_path_id(&expected_root, &relative) {
                            this.select(id, cx);
                        }
                    }))
                    .child(name),
            );
        }
        if node.children.len() > 60 {
            list = list.child(
                div()
                    .flex()
                    .gap_1()
                    .child(
                        Button::new("children-prev")
                            .xsmall()
                            .ghost()
                            .label(t("Previous"))
                            .disabled(self.list_page == 0)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.list_page = this.list_page.saturating_sub(1);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("children-next")
                            .xsmall()
                            .ghost()
                            .label(t("Next"))
                            .disabled((self.list_page + 1) * 60 >= node.children.len())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.list_page += 1;
                                cx.notify();
                            })),
                    ),
            );
        }
        if self.state.config.connections {
            if let Some(graph) = &self.live.graph {
                list = list.child(
                    div()
                        .font_semibold()
                        .child(t("Rust module / import connections")),
                );
                list = list.child(div().child(tf(
                    "{files} parsed files · {edges} links · {unresolved} unresolved · {external} external imports excluded",
                    &[("files", &graph.files.to_string()), ("edges", &graph.edges.to_string()),
                      ("unresolved", &graph.unresolved.to_string()), ("external", &graph.external.to_string())],
                )));
                list = list.child(div().child(t(
                    "Static links; macros, cfg conditions, and runtime calls are not evaluated",
                )));
                if graph.partial {
                    list = list.child(div().text_color(cx.theme().warning).child(t(
                        "Connection coverage is partial; some sources could not be analyzed",
                    )));
                }
                if node.kind == Kind::Directory {
                    list = list.child(
                        div().child(t("Select a Rust file to see what it uses and what uses it")),
                    );
                } else if self.relations.is_empty() {
                    list =
                        list.child(div().child(t("No resolved local connections for this file")));
                }
                for (index, relation) in self.relations.iter().enumerate() {
                    let path = relation.path.clone();
                    let expected_root = snapshot.root.clone();
                    let direction = match relation.direction {
                        Direction::Uses => t("Uses →"),
                        Direction::UsedBy => t("Used by ←"),
                    };
                    let label = format!(
                        "{} {}{}",
                        direction,
                        spider::display_path(&path),
                        if relation.depth > 1 {
                            format!(
                                " ({})",
                                tf("{steps} steps", &[("steps", &relation.depth.to_string())])
                            )
                        } else {
                            String::new()
                        }
                    );
                    list = list.child(
                        div()
                            .id(SharedString::from(format!("connection-{index}")))
                            .cursor_pointer()
                            .text_ellipsis()
                            .hover(|s| s.bg(cx.theme().accent))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(id) = this.current_path_id(&expected_root, &path) {
                                    this.reveal_match(id, window, cx);
                                }
                            }))
                            .child(label),
                    );
                }
                if self.relations.len() >= 100 {
                    list = list.child(div().child(t("Showing the first 100 connections")));
                }
            } else {
                list = list.child(div().child(if self.live.ready {
                    t("No connection data available")
                } else {
                    t("Preparing Rust connections…")
                }));
            }
        }
        for issue in &snapshot.issues {
            list = list.child(div().text_color(cx.theme().warning).child(issue.clone()));
        }
        list.into_any_element()
    }
}

impl Render for SpiderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_relations();
        if self.state.enabled && !self.requested && self.state.root().is_some() {
            self.requested = true;
            let view = cx.weak_entity();
            cx.defer(move |cx| {
                let _ = view.update(cx, |this, cx| this.request(false, cx));
            });
        }
        let root = div()
            .id("spider-pane")
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .key_context("Spider")
            .track_focus(&self.focus)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if !this.state.enabled || !this.focus.is_focused(window) {
                    return;
                }
                match ev.keystroke.key.as_str() {
                    "enter" => this.open_selected(false, cx),
                    "space" => this.toggle_branch(cx),
                    "backspace" if this.state.navigation.can_go_back() => this.go_back(cx),
                    "escape" => {
                        this.options = false;
                        this.search = false;
                        this.inspect = false;
                        window.focus(&this.focus, cx);
                        cx.emit(Event::Overlay);
                    }
                    "f" if !ev.keystroke.modifiers.control && !ev.keystroke.modifiers.platform => {
                        this.fit.set(true)
                    }
                    "/" => {
                        this.search = true;
                        this.input.update(cx, |i, cx| i.focus(window, cx));
                    }
                    _ => return,
                }
                cx.stop_propagation();
                cx.notify();
            }));
        if !self.state.enabled {
            if self.input.read(cx).focus_handle(cx).is_focused(window) {
                window.focus(&self.focus, cx);
            }
            return root
                .items_center()
                .justify_center()
                .gap_2()
                .child(div().text_sm().child(t("Spider map is disabled")))
                .child(
                    Button::new("enable-spider-settings")
                        .small()
                        .label(t("Open Settings"))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Settings))),
                )
                .into_any_element();
        }
        let scope = self
            .state
            .choices
            .iter()
            .find(|c| c.scope == self.state.config.scope)
            .map(|c| c.label.clone())
            .unwrap_or_else(|| t("Checkout unavailable").into());
        let heading = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div().flex_1().min_w_0().child(
                    Button::new("spider-overview")
                        .xsmall()
                        .ghost()
                        .w_full()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_left()
                                .text_ellipsis()
                                .child(scope),
                        )
                        .tooltip(t("Return to checkout overview"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            let effect = this.state.navigation.overview(this.camera.get());
                            this.search = false;
                            window.focus(&this.focus, cx);
                            this.navigation_effect(effect, cx);
                        })),
                ),
            )
            .child(
                Button::new("spider-view")
                    .xsmall()
                    .ghost()
                    .label(t("View"))
                    .selected(self.options)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.options = !this.options;
                        cx.emit(Event::Overlay);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("spider-fit")
                    .xsmall()
                    .ghost()
                    .label(t("Fit"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.fit.set(true);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("spider-find")
                    .xsmall()
                    .ghost()
                    .label(t("Find"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.search = !this.search;
                        if this.search {
                            this.input.update(cx, |input, cx| input.focus(window, cx));
                        } else {
                            window.focus(&this.focus, cx);
                        }
                        cx.notify();
                    })),
            )
            .child(
                Button::new("spider-refresh")
                    .xsmall()
                    .ghost()
                    .label("↻")
                    .tooltip(t("Refresh map"))
                    .on_click(cx.listener(|this, _, _, cx| this.request(true, cx))),
            );
        let mut body = root
            .child(heading)
            .children(self.options.then(|| self.options(cx)));
        if self.search {
            let results = self.search_results();
            let mut list = div()
                .id("spider-search-results")
                .max_h(px(180.0))
                .overflow_y_scroll()
                .text_xs()
                .flex_none();
            for (index, id) in results.iter().take(60).enumerate() {
                let id = *id;
                let relative = self.state.snapshot.as_ref().unwrap().tree.nodes[id]
                    .path
                    .clone();
                let expected_root = self.state.snapshot.as_ref().unwrap().root.clone();
                let path = spider::display_path(
                    &self.state.snapshot.as_ref().unwrap().tree.nodes[id].path,
                );
                list = list.child(
                    div()
                        .id(SharedString::from(format!("result-{id}")))
                        .px_2()
                        .py_1()
                        .text_ellipsis()
                        .cursor_pointer()
                        .when(index == self.search_index, |d| d.bg(cx.theme().accent))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(id) = this.current_path_id(&expected_root, &relative) {
                                this.reveal_match(id, window, cx);
                            }
                        }))
                        .child(if path.is_empty() {
                            t("Project root").to_string()
                        } else {
                            path
                        }),
                );
            }
            if results.len() > 60 {
                list = list.child(
                    div()
                        .px_2()
                        .py_1()
                        .text_color(cx.theme().muted_foreground)
                        .child(t(
                            "Showing the first 60 matches; refine the path to find more",
                        )),
                );
            }
            body = body
                .child(
                    div()
                        .px_2()
                        .capture_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                            let results = this.search_results();
                            match ev.keystroke.key.as_str() {
                                "down" => {
                                    this.search_index = (this.search_index + 1)
                                        .min(results.len().min(60).saturating_sub(1))
                                }
                                "up" => this.search_index = this.search_index.saturating_sub(1),
                                "enter" => {
                                    if let Some(id) = results.get(this.search_index) {
                                        this.reveal_match(*id, window, cx);
                                    }
                                }
                                "escape" => {
                                    this.search = false;
                                    window.focus(&this.focus, cx);
                                }
                                _ => return,
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }))
                        .child(Input::new(&self.input).small()),
                )
                .child(list);
        }
        let map = self.map(cx);
        body = body.child(map);
        if let Some(error) = &self.error {
            body = body.child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }
        let path = if self.state.navigation.selected.as_os_str().is_empty() {
            t("Project root").to_string()
        } else {
            spider::display_path(&self.state.navigation.selected)
        };
        body = body.child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    Button::new("spider-back")
                        .xsmall()
                        .ghost()
                        .label(t("Back"))
                        .tooltip(t("Back to previous map view"))
                        .disabled(!self.state.navigation.can_go_back())
                        .on_click(cx.listener(|this, _, _, cx| this.go_back(cx))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_ellipsis()
                        .child(path),
                )
                .child(
                    Button::new("spider-inspect")
                        .xsmall()
                        .ghost()
                        .label(t("Inspect"))
                        .selected(self.inspect)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.inspect = !this.inspect;
                            cx.notify();
                        })),
                ),
        );
        if self.inspect {
            body = body.child(self.inspector(cx));
        }
        if !self.state.navigation.focused.as_os_str().is_empty() {
            body = body.child(div().flex_none().px_2().text_size(px(10.0)).text_color(cx.theme().muted_foreground)
                .child(t("Focused for readability · double-click the center folder or use Back to return")));
        }
        let status = if self.loading {
            t("Scanning…").to_string()
        } else if let Some(snapshot) = &self.state.snapshot {
            let time = chrono::DateTime::from_timestamp(snapshot.scanned_at, 0)
                .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string())
                .unwrap_or_default();
            tf(
                "{files} files · {time} · {seconds}s",
                &[
                    ("files", &snapshot.tree.nodes[0].files.to_string()),
                    ("time", &time),
                    ("seconds", &format!("{:.1}", snapshot.elapsed.as_secs_f32())),
                ],
            )
        } else {
            t("Choose a checkout in View").to_string()
        };
        let legend = if self.state.config.overlay == Overlay::GitActivity {
            t("Color: commit age ≤30d · outline: changes · size: text lines")
        } else {
            t("Edges: contains · size: text lines · +N: hidden children")
        };
        body = body.child(
            div()
                .flex_none()
                .px_2()
                .text_size(px(10.0))
                .text_color(cx.theme().muted_foreground)
                .child(status)
                .child(div().child(legend)),
        );
        if self
            .state
            .snapshot
            .as_ref()
            .is_some_and(|s| !s.issues.is_empty())
        {
            body = body.child(
                div()
                    .flex_none()
                    .px_2()
                    .text_size(px(10.0))
                    .text_color(cx.theme().warning)
                    .child(t(
                        "Partial scan or unavailable metrics — Inspect for details",
                    )),
            );
        }
        if self.state.config.overlay == Overlay::GitActivity
            && let Some(snapshot) = &self.state.snapshot
        {
            body = body.child(
                div()
                    .flex_none()
                    .px_2()
                    .text_size(px(10.0))
                    .text_color(cx.theme().muted_foreground)
                    .child(if !snapshot.git {
                        t("Git activity is unavailable for this folder")
                    } else if snapshot.history_empty {
                        t("This checkout has no commits yet")
                    } else if snapshot.history_limited {
                        t("Recent history coverage is limited")
                    } else {
                        t("Recent → older: green → blue → gray")
                    }),
            );
        }
        if self.state.config.activity || self.state.config.connections {
            body = body.child(
                div()
                    .flex_none()
                    .px_2()
                    .text_size(px(10.0))
                    .text_color(cx.theme().muted_foreground)
                    .child(if !self.live.ready {
                        t("Starting live overlays…")
                    } else if self.state.config.activity {
                        t("Cyan glow: edited file or nearest visible folder · 25s · root stays quiet")
                    } else {
                        t("Rust connections update after saved source changes")
                    }),
            );
            if self.state.config.connections {
                body = body.child(div().flex_none().px_2().text_size(px(10.0)).text_color(cx.theme().muted_foreground)
                    .child(t("Links: cyan = uses · purple = used by · dashed = indirect · hidden files end at folders")));
            }
            if self.live.structural {
                body = body.child(div().flex_none().px_2().text_size(px(10.0)).text_color(cx.theme().warning)
                    .child(if self.live.refresh_failed {
                        t("Inventory refresh failed; use Refresh to retry")
                    } else if self.state.config.activity {
                        t("Updating map for new, renamed, or removed paths…")
                    } else {
                        t("New, renamed, or removed paths detected; Refresh updates the inventory")
                    }));
            }
            if let Some(issue) = &self.live.issue {
                body = body.child(
                    div()
                        .flex_none()
                        .px_2()
                        .text_size(px(10.0))
                        .text_color(cx.theme().warning)
                        .child(issue.clone()),
                );
            }
        }
        let _ = window;
        body.into_any_element()
    }
}

impl SpiderView {
    fn map(&self, cx: &mut Context<Self>) -> AnyElement {
        let snapshot = self.state.snapshot.clone();
        let focused = self.state.navigation.focused.clone();
        let expanded = self.state.navigation.expanded.clone();
        let selected = self.state.navigation.selected.clone();
        let hovered = self.hovered.clone();
        let camera = self.camera.clone();
        let fit = self.fit.clone();
        let bounds_cell = self.bounds.clone();
        let geometry = self.geometry.clone();
        let overlay = self.state.config.overlay;
        let layout = self.state.config.layout;
        let relations = self.relations.clone();
        let activity_projection = self.activity_projection.clone();
        let related_paths: Vec<_> = relations.iter().take(48).map(|r| r.path.clone()).collect();
        let recent = self
            .state
            .config
            .activity
            .then(|| (self.live.activity.clone(), self.live.now));
        let canvas = canvas(
            move |bounds, _, _| {
                bounds_cell.set(bounds);
                let Some(snapshot) = &snapshot else {
                    geometry.borrow_mut().clear();
                    return None;
                };
                let tree = &snapshot.tree;
                let root = tree.find(&focused).unwrap_or(0);
                let width = f32::from(bounds.size.width);
                let height = f32::from(bounds.size.height);
                let viewport = Viewport { width, height };
                let budget = viewport.budget(layout);
                let related: Vec<_> = related_paths
                    .iter()
                    .filter_map(|path| tree.find(path))
                    .collect();
                let visible =
                    tree.visible_related(root, &expanded, budget, tree.find(&selected), &related);
                let points = spider::positions(tree, &visible, root, layout);
                if fit.replace(false) {
                    camera.set(fit_map(tree, &points, viewport));
                }
                let recent = recent.as_ref().map(|(activity, now)| {
                    (
                        activity_projection
                            .borrow_mut()
                            .get(snapshot, activity, &visible),
                        *now,
                    )
                });
                *geometry.borrow_mut() = points.clone();
                Some((
                    snapshot.clone(),
                    points,
                    camera.get(),
                    selected.clone(),
                    hovered.clone(),
                    overlay,
                    recent,
                ))
            },
            move |bounds, data, window, cx| {
                let Some((snapshot, points, camera, selected, hovered, overlay, recent)) = data
                else {
                    return;
                };
                let tree = &snapshot.tree;
                let width = f32::from(bounds.size.width);
                let height = f32::from(bounds.size.height);
                let screen: HashMap<_, _> = points
                    .iter()
                    .map(|(id, p)| {
                        let p = camera.screen(*p, width, height);
                        (
                            *id,
                            point(bounds.origin.x + px(p.x), bounds.origin.y + px(p.y)),
                        )
                    })
                    .collect();
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    for (id, _) in &points {
                        if let Some(parent) = tree.nodes[*id].parent.and_then(|p| screen.get(&p)) {
                            let mut path = PathBuilder::stroke(px(1.0));
                            path.move_to(*parent);
                            path.line_to(screen[id]);
                            if let Ok(path) = path.build() {
                                window.paint_path(path, cx.theme().muted_foreground.opacity(0.3));
                            }
                        }
                    }
                    // Draw a bounded set of selected-file links. A hidden target
                    // terminates at its visible ancestor; exact paths stay in Inspect.
                    if let Some(selected_id) = tree.find(&selected)
                        && let Some(selected_point) = screen.get(&selected_id)
                    {
                        let mut drawn = HashSet::new();
                        for relation in relations.iter().take(48) {
                            let mut target = tree.find(&relation.path);
                            while let Some(id) = target {
                                if screen.contains_key(&id) {
                                    break;
                                }
                                target = tree.nodes[id].parent;
                            }
                            let Some(target) = target.filter(|id| *id != selected_id) else {
                                continue;
                            };
                            let uses = relation.direction == Direction::Uses;
                            if !drawn.insert((target, uses)) {
                                continue;
                            }
                            let peer = screen[&target];
                            let (from, to) = if uses {
                                (*selected_point, peer)
                            } else {
                                (peer, *selected_point)
                            };
                            let color: Hsla = rgb(if uses {
                                if cx.theme().is_dark() {
                                    0x22d3ee
                                } else {
                                    0x0e7490
                                }
                            } else if cx.theme().is_dark() {
                                0xc4b5fd
                            } else {
                                0x7c3aed
                            })
                            .into();
                            let dx = f32::from(to.x - from.x);
                            let dy = f32::from(to.y - from.y);
                            let length = dx.hypot(dy).max(1.0);
                            let mut path = PathBuilder::stroke(px(1.5));
                            if relation.depth == 1 {
                                path.move_to(from);
                                path.line_to(to);
                            } else {
                                let mut at = 0.0;
                                let step = (length / 64.0).max(10.0);
                                while at < length {
                                    path.move_to(
                                        from + point(px(dx * at / length), px(dy * at / length)),
                                    );
                                    let end = (at + step * 0.5).min(length);
                                    path.line_to(
                                        from + point(px(dx * end / length), px(dy * end / length)),
                                    );
                                    at += step;
                                }
                            }
                            // Put arrowheads away from the node so the fill cannot hide them.
                            let tip = from + point(px(dx * 0.72), px(dy * 0.72));
                            let back = point(px(dx / length * 7.0), px(dy / length * 7.0));
                            let side = point(px(-dy / length * 3.5), px(dx / length * 3.5));
                            path.move_to(tip - back + side);
                            path.line_to(tip);
                            path.line_to(tip - back - side);
                            if let Ok(path) = path.build() {
                                window.paint_path(path, color.opacity(0.8));
                            }
                        }
                    }
                    let mut visible_children: HashMap<usize, usize> = HashMap::new();
                    for (id, _) in &points {
                        if let Some(parent) = tree.nodes[*id].parent {
                            *visible_children.entry(parent).or_default() += 1;
                        }
                    }
                    let mut labels: Vec<Bounds<Pixels>> = Vec::new();
                    let mut ordered: Vec<_> = points.iter().map(|(id, _)| *id).collect();
                    ordered.sort_by_key(|id| {
                        (
                            tree.nodes[*id].path != selected
                                && hovered.as_ref() != Some(&tree.nodes[*id].path),
                            tree.nodes[*id].kind != Kind::Directory,
                            *id,
                        )
                    });
                    for id in ordered {
                        let node = &tree.nodes[id];
                        let center = screen[&id];
                        let radius = px((node.radius() * camera.zoom).clamp(3.0, 30.0));
                        let color = if overlay == Overlay::GitActivity {
                            match node
                                .committed_at
                                .map(|t| snapshot.scanned_at.saturating_sub(t))
                            {
                                Some(age) if age <= 86400 => rgb(if cx.theme().is_dark() {
                                    0x34d399
                                } else {
                                    0x047857
                                })
                                .into(),
                                Some(age) if age <= 7 * 86400 => rgb(if cx.theme().is_dark() {
                                    0x60a5fa
                                } else {
                                    0x1d4ed8
                                })
                                .into(),
                                _ => cx.theme().muted_foreground,
                            }
                        } else if node.kind == Kind::Directory {
                            cx.theme().primary
                        } else {
                            cx.theme().muted_foreground
                        };
                        let chosen = node.path == selected;
                        let label_priority = chosen || hovered.as_ref() == Some(&node.path);
                        if let Some((activity, now)) = &recent {
                            let glow = activity.strength(&node.path, *now);
                            if glow > 0.0 {
                                let color: Hsla = rgb(if cx.theme().is_dark() {
                                    0x22d3ee
                                } else {
                                    0x0891b2
                                })
                                .into();
                                for (spread, opacity) in [(10.0, 0.14), (6.0, 0.30)] {
                                    let halo = radius + px(spread);
                                    window.paint_quad(
                                        fill(
                                            Bounds::new(
                                                center - point(halo, halo),
                                                size(halo * 2.0, halo * 2.0),
                                            ),
                                            color.opacity(glow * opacity),
                                        )
                                        .corner_radii(
                                            if node.kind == Kind::Directory {
                                                halo
                                            } else {
                                                px(5.0)
                                            },
                                        ),
                                    );
                                }
                            }
                        }
                        if chosen || node.changed_files > 0 {
                            let ring = radius + px(if chosen { 4.0 } else { 2.0 });
                            window.paint_quad(
                                fill(
                                    Bounds::new(
                                        center - point(ring, ring),
                                        size(ring * 2.0, ring * 2.0),
                                    ),
                                    if chosen {
                                        cx.theme().foreground
                                    } else {
                                        cx.theme().warning
                                    },
                                )
                                .corner_radii(
                                    if node.kind == Kind::Directory {
                                        ring
                                    } else {
                                        px(4.0)
                                    },
                                ),
                            );
                        }
                        window.paint_quad(
                            fill(
                                Bounds::new(
                                    center - point(radius, radius),
                                    size(radius * 2.0, radius * 2.0),
                                ),
                                color,
                            )
                            .corner_radii(
                                if node.kind == Kind::Directory {
                                    radius
                                } else {
                                    px(2.0)
                                },
                            ),
                        );
                        let hidden = node
                            .children
                            .len()
                            .saturating_sub(visible_children.get(&id).copied().unwrap_or(0));
                        let root_name = spider::display_path(std::path::Path::new(
                            snapshot.root.file_name().unwrap_or_default(),
                        ));
                        let text = spider::node_label(node, hidden, &root_name);
                        let label_width = px(text.chars().count() as f32 * 6.6);
                        let mut label = Bounds::new(
                            point(center.x + radius + px(5.0), center.y - px(8.0)),
                            size(label_width, px(17.0)),
                        );
                        if label_priority {
                            label.origin.x = label
                                .origin
                                .x
                                .min(bounds.right() - label.size.width - px(4.0))
                                .max(bounds.left() + px(4.0));
                        }
                        if !label_priority
                            && (labels.iter().any(|b| b.intersects(&label))
                                || label.right() > bounds.right())
                        {
                            continue;
                        }
                        labels.push(label);
                        let style = TextRun {
                            len: text.len(),
                            font: font("sans-serif"),
                            color: cx.theme().foreground,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let line =
                            window
                                .text_system()
                                .shape_line(text.into(), px(11.0), &[style], None);
                        let _ =
                            line.paint(label.origin, px(17.0), TextAlign::Left, None, window, cx);
                    }
                });
            },
        )
        .size_full();
        div()
            .id("spider-canvas")
            .flex_1()
            .min_h_0()
            .min_w_0()
            .relative()
            .overflow_hidden()
            .cursor(if self.hovered.is_some() {
                CursorStyle::PointingHand
            } else {
                CursorStyle::OpenHand
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    this.pointer_down = Some(ev.position);
                    this.drag_origin = this.camera.get().pan;
                    this.dragged = false;
                    cx.stop_propagation();
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if this.pointer_down.is_none() {
                    let hovered = if this.bounds.get().contains(&ev.position) {
                        this.hit(ev.position)
                            .and_then(|id| this.state.snapshot.as_ref()?.tree.nodes.get(id))
                            .map(|n| n.path.clone())
                    } else {
                        None
                    };
                    if hovered != this.hovered {
                        this.hovered = hovered;
                        cx.notify();
                    }
                }
                if let Some(start) = this.pointer_down {
                    let dx = f32::from(ev.position.x - start.x);
                    let dy = f32::from(ev.position.y - start.y);
                    if dx.hypot(dy) > 5.0 {
                        this.dragged = true;
                    }
                    if this.dragged {
                        let mut camera = this.camera.get();
                        camera.pan = spider::Point {
                            x: this.drag_origin.x + dx,
                            y: this.drag_origin.y + dy,
                        };
                        this.camera.set(camera);
                        cx.notify();
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseUpEvent, _, cx| {
                    if this.pointer_down.take().is_some()
                        && !this.dragged
                        && let Some(id) = this.hit(ev.position)
                    {
                        this.select(id, cx);
                        if ev.click_count >= 2 {
                            this.open_selected(false, cx);
                        }
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.pointer_down = None;
                }),
            )
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                let delta = ev.delta.pixel_delta(px(18.0));
                let b = this.bounds.get();
                let mut camera = this.camera.get();
                camera.zoom_at(
                    (-f32::from(delta.y) * 0.003).exp(),
                    this.pointer(ev.position),
                    f32::from(b.size.width),
                    f32::from(b.size.height),
                );
                this.camera.set(camera);
                cx.stop_propagation();
                cx.notify();
            }))
            .child(canvas)
            .into_any_element()
    }
}
