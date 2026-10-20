//! Pure project-map model and geometry. Paths are native checkout-relative keys;
//! callers supply inventory/Git facts and own all I/O and job lifetimes.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

pub mod activity;
pub mod connections;
pub mod navigation;
pub mod refresh;

pub const MAX_VISIBLE_NODES: usize = 300;

pub fn node_label(node: &Node, hidden: usize, root_name: &str) -> String {
    let name = node.path.file_name().map_or_else(
        || root_name.to_string(),
        |name| display_path(Path::new(name)),
    );
    let short: String = name.chars().take(23).collect();
    format!(
        "{}{}{}",
        short,
        if name.chars().count() > 23 { "…" } else { "" },
        if hidden > 0 {
            format!(" +{hidden}")
        } else {
            String::new()
        }
    )
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    #[default]
    ProjectRoot,
    Worktree(Uuid),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Overlay {
    #[default]
    Structure,
    GitActivity,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layout {
    #[default]
    Radial,
    Tree,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub scope: Scope,
    #[serde(default)]
    pub overlay: Overlay,
    #[serde(default)]
    pub layout: Layout,
    #[serde(default)]
    pub activity: bool,
    #[serde(default)]
    pub connections: bool,
    #[serde(default)]
    pub indirect: bool,
}

/// Match the shown tab of each leaf without starting any runtime work.
pub fn visible_tabs(
    layout: &crate::PaneNode,
    active: Option<Uuid>,
    maximized: Option<Uuid>,
) -> Vec<Uuid> {
    fn walk(node: &crate::PaneNode, active: Option<Uuid>, out: &mut Vec<Uuid>) {
        match node {
            crate::PaneNode::Leaf(leaf) => {
                if !leaf.tabs.is_empty() {
                    out.push(
                        active
                            .filter(|id| leaf.tabs.contains(id))
                            .unwrap_or_else(|| {
                                leaf.tabs[leaf.active.min(leaf.tabs.len().saturating_sub(1))]
                            }),
                    );
                }
            }
            crate::PaneNode::Split { children, .. } => {
                for child in children {
                    walk(child, active, out);
                }
            }
        }
    }
    let root = maximized
        .and_then(|id| layout.leaf_containing(id))
        .unwrap_or(layout);
    let mut out = Vec::new();
    walk(root, active, &mut out);
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Directory,
    Text,
    Binary,
    Symlink,
    Boundary,
    Deleted,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub kind: Kind,
    pub bytes: u64,
    pub lines: Option<u64>,
    pub committed_at: Option<i64>,
    pub change: Option<String>,
}

impl Entry {
    pub fn new(path: PathBuf, kind: Kind) -> Self {
        Self {
            path,
            kind,
            bytes: 0,
            lines: None,
            committed_at: None,
            change: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub path: PathBuf,
    pub kind: Kind,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub bytes: u64,
    pub lines: u64,
    pub incomplete: bool,
    pub files: usize,
    pub recent_files: usize,
    pub changed_files: usize,
    pub committed_at: Option<i64>,
    pub change: Option<String>,
}

impl Node {
    pub fn radius(&self) -> f32 {
        // Area is monotonic in log(line count), bounded for huge generated files.
        if matches!(self.kind, Kind::Binary | Kind::Symlink | Kind::Boundary) {
            return 5.0;
        }
        (16.0 + (self.lines as f32 + 1.0).ln() * 12.0)
            .sqrt()
            .clamp(4.0, 16.0)
    }
}

#[derive(Clone, Debug)]
pub struct Tree {
    pub nodes: Vec<Node>,
    index: HashMap<PathBuf, usize>,
    search_keys: Vec<String>,
}

pub fn valid_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|c| matches!(c, Component::Normal(_)))
}

/// Escape controls/undecodable bytes for labels without changing path identity.
pub fn display_path(path: &Path) -> String {
    if let Some(text) = path.to_str() {
        text.chars()
            .map(|c| {
                if c.is_control() {
                    c.escape_default().to_string()
                } else {
                    c.to_string()
                }
            })
            .collect()
    } else {
        path.as_os_str()
            .as_encoded_bytes()
            .iter()
            .map(|b| {
                if (0x20..0x7f).contains(b) {
                    (*b as char).to_string()
                } else {
                    format!("\\x{b:02x}")
                }
            })
            .collect()
    }
}

impl Tree {
    pub fn build(entries: Vec<Entry>) -> Self {
        let entries: BTreeMap<_, _> = entries
            .into_iter()
            .filter(|e| valid_relative(&e.path))
            .map(|e| (e.path.clone(), e))
            .collect();
        let mut all = BTreeMap::new();
        all.insert(PathBuf::new(), Kind::Directory);
        for entry in entries.values() {
            all.insert(entry.path.clone(), entry.kind);
            let mut parent = entry.path.parent();
            while let Some(path) = parent {
                all.entry(path.to_path_buf()).or_insert(Kind::Directory);
                parent = path.parent();
            }
        }
        let mut nodes = Vec::with_capacity(all.len());
        let mut index = HashMap::with_capacity(all.len());
        // Native path order guarantees that parents precede their descendants.
        for (path, kind) in all {
            let entry = entries.get(&path);
            let parent = path.parent().and_then(|p| index.get(p)).copied();
            let id = nodes.len();
            index.insert(path.clone(), id);
            nodes.push(Node {
                path,
                kind,
                parent,
                children: Vec::new(),
                bytes: entry.map_or(0, |e| e.bytes),
                lines: entry.and_then(|e| e.lines).unwrap_or(0),
                incomplete: entry.is_some_and(|e| {
                    e.lines.is_none()
                        && !matches!(
                            e.kind,
                            Kind::Binary | Kind::Symlink | Kind::Boundary | Kind::Deleted
                        )
                }),
                files: usize::from(kind != Kind::Directory),
                recent_files: usize::from(entry.is_some_and(|e| e.committed_at.is_some())),
                changed_files: usize::from(entry.is_some_and(|e| e.change.is_some())),
                committed_at: entry.and_then(|e| e.committed_at),
                change: entry.and_then(|e| e.change.clone()),
            });
            if let Some(parent) = parent {
                nodes[parent].children.push(id);
            }
        }
        for id in (1..nodes.len()).rev() {
            let Some(parent) = nodes[id].parent else {
                continue;
            };
            let (before, after) = nodes.split_at_mut(id);
            let node = &after[0];
            let ancestor = &mut before[parent];
            ancestor.bytes = ancestor.bytes.saturating_add(node.bytes);
            ancestor.lines = ancestor.lines.saturating_add(node.lines);
            ancestor.files += node.files;
            ancestor.recent_files += node.recent_files;
            ancestor.changed_files += node.changed_files;
            ancestor.incomplete |= node.incomplete;
            ancestor.committed_at = ancestor.committed_at.max(node.committed_at);
        }
        let search_keys = nodes
            .iter()
            .map(|n| display_path(&n.path).to_lowercase())
            .collect();
        Self {
            nodes,
            index,
            search_keys,
        }
    }

    pub fn find(&self, path: &Path) -> Option<usize> {
        self.index.get(path).copied()
    }

    /// A focused folder should show the next layer of its structure, rather
    /// than only collapsed directory bubbles. Seed only visible child folders;
    /// ordinary branch/node limits and later manual collapse still apply.
    pub fn focus_expansion(&self, root: usize) -> HashSet<PathBuf> {
        self.nodes[root]
            .children
            .iter()
            .take(24)
            .filter(|id| self.nodes[**id].kind == Kind::Directory)
            .map(|id| self.nodes[*id].path.clone())
            .collect()
    }

    /// Breadth-first expansion with explicit per-node hidden-child counts.
    pub fn visible(&self, root: usize, expanded: &HashSet<PathBuf>, budget: usize) -> Vec<usize> {
        self.visible_selected(root, expanded, budget, None)
    }

    pub fn visible_selected(
        &self,
        root: usize,
        expanded: &HashSet<PathBuf>,
        budget: usize,
        selected: Option<usize>,
    ) -> Vec<usize> {
        let budget = budget.max(1);
        if let Some(selected) = selected {
            let ordinary = self.visible_selected(root, expanded, budget, None);
            if ordinary.contains(&selected) {
                return ordinary;
            }
        }
        let mut out = Vec::new();
        let mut forced = Vec::new();
        let mut current = selected;
        while let Some(id) = current {
            if id == root {
                forced.reverse();
                break;
            }
            forced.push(id);
            current = self.nodes[id].parent;
        }
        if current.is_none() {
            forced.clear();
        }
        let mut admitted = HashSet::new();
        admitted.insert(root);
        for id in forced.iter().take(budget.saturating_sub(1)) {
            admitted.insert(*id);
        }
        let mut queue = VecDeque::from([root]);
        queue.extend(forced.into_iter().take(budget.saturating_sub(1)));
        while let Some(id) = queue.pop_front() {
            if out.len() >= budget {
                break;
            }
            out.push(id);
            if id == root || expanded.contains(&self.nodes[id].path) {
                for child in self.nodes[id].children.iter().take(24) {
                    if out.len() + queue.len() >= budget {
                        break;
                    }
                    if admitted.insert(*child) {
                        queue.push_back(*child);
                    }
                }
            }
        }
        out
    }

    /// Reveal related paths before filling remaining space with ordinary
    /// expansion. Complete ancestor chains keep every admitted node connected;
    /// unrelated branches and oversized chains cannot exhaust the frame budget.
    pub fn visible_related(
        &self,
        root: usize,
        expanded: &HashSet<PathBuf>,
        budget: usize,
        selected: Option<usize>,
        related: &[usize],
    ) -> Vec<usize> {
        if related.is_empty() {
            return self.visible_selected(root, expanded, budget, selected);
        }
        let budget = budget.max(1);
        let mut priority = vec![root];
        let mut admitted = HashSet::from([root]);
        for id in selected.into_iter().chain(related.iter().copied().take(48)) {
            let mut chain = Vec::new();
            let mut current = Some(id);
            while let Some(id) = current {
                if admitted.contains(&id) {
                    break;
                }
                chain.push(id);
                current = self.nodes.get(id).and_then(|node| node.parent);
            }
            if current.is_none() || priority.len() + chain.len() > budget {
                continue;
            }
            for id in chain.into_iter().rev() {
                admitted.insert(id);
                priority.push(id);
            }
        }
        let mut queue: VecDeque<_> = priority.into();
        let mut out = Vec::new();
        while let Some(id) = queue.pop_front() {
            out.push(id);
            if id == root || expanded.contains(&self.nodes[id].path) {
                for child in self.nodes[id].children.iter().take(24) {
                    if admitted.len() >= budget {
                        break;
                    }
                    if admitted.insert(*child) {
                        queue.push_back(*child);
                    }
                }
            }
        }
        out
    }

    pub fn matching(&self, query: &str, limit: usize) -> Vec<usize> {
        let query = query.to_lowercase();
        self.search_keys
            .iter()
            .enumerate()
            .filter(|(_, key)| key.contains(&query))
            .map(|(i, _)| i)
            .take(limit)
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub pan: Point,
    pub zoom: f32,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            pan: Point::default(),
            zoom: 1.0,
        }
    }
}
impl Camera {
    pub fn screen(&self, p: Point, width: f32, height: f32) -> Point {
        Point {
            x: width * 0.5 + self.pan.x + p.x * self.zoom,
            y: height * 0.5 + self.pan.y + p.y * self.zoom,
        }
    }
    pub fn zoom_at(&mut self, factor: f32, anchor: Point, width: f32, height: f32) {
        let old = self.zoom;
        self.zoom = (old * factor).clamp(0.15, 5.0);
        let ratio = self.zoom / old;
        self.pan.x = anchor.x - width * 0.5 - (anchor.x - width * 0.5 - self.pan.x) * ratio;
        self.pan.y = anchor.y - height * 0.5 - (anchor.y - height * 0.5 - self.pan.y) * ratio;
    }
    pub fn fit(&mut self, points: &[(usize, Point)], width: f32, height: f32) {
        let mut min = Point { x: 0.0, y: 0.0 };
        let mut max = min;
        for (_, p) in points {
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
        }
        self.zoom = ((width - 100.0).max(30.0) / (max.x - min.x + 32.0))
            .min((height - 60.0).max(30.0) / (max.y - min.y + 32.0))
            .clamp(0.15, 1.5);
        self.pan = Point {
            x: -(min.x + max.x) * 0.5 * self.zoom,
            y: -(min.y + max.y) * 0.5 * self.zoom,
        };
    }
}

/// Deterministic sectors based only on visible structure, never metrics/colors.
pub fn positions(
    tree: &Tree,
    visible: &[usize],
    root: usize,
    layout: Layout,
) -> Vec<(usize, Point)> {
    let mut shown_children: HashMap<usize, Vec<usize>> = HashMap::new();
    for id in visible {
        if let Some(parent) = tree.nodes[*id].parent {
            shown_children.entry(parent).or_default().push(*id);
        }
    }
    for children in shown_children.values_mut() {
        children.sort_unstable();
    }
    let mut weights = HashMap::<usize, usize>::new();
    let mut bottom_up = visible.to_vec();
    bottom_up.sort_unstable_by(|a, b| b.cmp(a));
    for id in bottom_up {
        let weight = shown_children.get(&id).map_or(1, |children| {
            children.iter().map(|c| weights[c]).sum::<usize>().max(1)
        });
        weights.insert(id, weight);
    }
    let spacing = 115.0f32
        .max(weights.get(&root).copied().unwrap_or(1) as f32 * 10.0 / std::f32::consts::TAU);
    let mut points = HashMap::new();
    let mut queue = VecDeque::from([(root, 0usize, 0.0f32, std::f32::consts::TAU)]);
    while let Some((id, depth, start, span)) = queue.pop_front() {
        let angle = start + span * 0.5 - std::f32::consts::FRAC_PI_2;
        let p = if depth == 0 {
            Point::default()
        } else {
            match layout {
                Layout::Radial => Point {
                    x: angle.cos() * depth as f32 * spacing,
                    y: angle.sin() * depth as f32 * spacing,
                },
                Layout::Tree => Point {
                    x: depth as f32 * 160.0,
                    y: (start + span * 0.5 - std::f32::consts::PI) * 100.0,
                },
            }
        };
        points.insert(id, p);
        let children = shown_children.remove(&id).unwrap_or_default();
        let total = children.iter().map(|id| weights[id]).sum::<usize>().max(1);
        let mut at = start;
        for child in children {
            let step = span * weights[&child] as f32 / total as f32;
            queue.push_back((child, depth + 1, at, step));
            at += step;
        }
    }
    visible
        .iter()
        .filter_map(|id| points.get(id).map(|p| (*id, *p)))
        .collect()
}

pub fn hit_test(
    points: &[(usize, Point)],
    pointer: Point,
    tree: &Tree,
    camera: Camera,
    width: f32,
    height: f32,
) -> Option<usize> {
    points.iter().rev().find_map(|(id, p)| {
        let p = camera.screen(*p, width, height);
        let radius = (tree.nodes[*id].radius() * camera.zoom).clamp(9.0, 30.0);
        let dx = (p.x - pointer.x).abs();
        let dy = (p.y - pointer.y).abs();
        let inside = if tree.nodes[*id].kind == Kind::Directory {
            dx.hypot(dy) <= radius
        } else {
            dx <= radius && dy <= radius
        };
        inside.then_some(*id)
    })
}

#[cfg(test)]
mod tests {
    use super::{
        Camera, Config, Entry, Kind, Layout, Overlay, Point, Scope, Tree, hit_test, positions,
        valid_relative,
    };
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    fn entry(path: &str, lines: u64) -> Entry {
        let mut e = Entry::new(path.into(), Kind::Text);
        e.lines = Some(lines);
        e.bytes = lines * 3;
        e
    }
    #[test]
    fn aggregation_is_unique_and_keeps_unknowns() {
        let mut recent = entry("src/a.rs", 4);
        recent.committed_at = Some(200);
        recent.change = Some(" M".into());
        let tree = Tree::build(vec![
            entry("src/a.rs", 99),
            recent,
            Entry::new("src/b.rs".into(), Kind::Unknown),
            entry("README", 3),
        ]);
        let root = &tree.nodes[0];
        assert_eq!(
            (
                root.files,
                root.lines,
                root.recent_files,
                root.changed_files
            ),
            (3, 7, 1, 1)
        );
        assert_eq!(root.committed_at, Some(200));
        assert!(root.incomplete);
        assert_eq!(tree.nodes[tree.find(Path::new("src")).unwrap()].lines, 4);
    }
    #[test]
    fn rejects_escaping_paths_and_preserves_native_identity() {
        for path in ["../escape", "/absolute", "", "a/../../b"] {
            assert!(!valid_relative(Path::new(path)));
        }
        let tree = Tree::build(vec![entry("a/b", 1), entry("A/b", 2), entry("../bad", 8)]);
        assert_ne!(tree.find(Path::new("a/b")), tree.find(Path::new("A/b")));
        assert_eq!(tree.nodes[0].lines, 3);
    }
    #[test]
    fn missing_config_fields_default_and_round_trip() {
        let config: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(config.scope, Scope::ProjectRoot);
        assert_eq!(config.overlay, Overlay::Structure);
        assert!(!config.activity && !config.connections && !config.indirect);
        let enabled: Config =
            serde_json::from_str(r#"{"activity":true,"connections":true,"indirect":true}"#)
                .unwrap();
        assert!(enabled.activity && enabled.connections && enabled.indirect);
        assert_eq!(
            serde_json::from_str::<Config>(&serde_json::to_string(&config).unwrap()).unwrap(),
            config
        );
    }
    #[test]
    fn visibility_is_bounded_and_respects_collapse() {
        let tree = Tree::build((0..1000).map(|i| entry(&format!("src/f{i}"), 1)).collect());
        assert_eq!(tree.visible(0, &HashSet::new(), 300).len(), 2);
        let expanded = HashSet::from([PathBuf::from("src")]);
        let ordinary = tree.visible(0, &expanded, 300);
        assert_eq!(ordinary.len(), 26);
        assert_eq!(
            tree.visible_selected(0, &expanded, 300, Some(ordinary[5])),
            ordinary
        );
        assert_eq!(tree.nodes[0].files, 1000);
        let selected = tree.find(Path::new("src/f999")).unwrap();
        let shown = tree.visible_selected(0, &expanded, 12, Some(selected));
        assert!(shown.contains(&selected));
        assert_eq!(shown.len(), 12);
        assert_eq!(positions(&tree, &shown, 0, Layout::Radial).len(), 12);
    }
    #[test]
    fn folder_focus_reveals_source_depth_and_still_allows_collapse() {
        let mut entries: Vec<_> = (0..20)
            .map(|i| entry(&format!("crates/core/src/file{i}.rs"), 1))
            .collect();
        entries.extend([
            entry("crates/core/Cargo.toml", 1),
            entry("crates/core/tests/one.rs", 1),
            entry("crates/core/src/nested/deeper.rs", 1),
            entry("other/unrelated.rs", 1),
        ]);
        let tree = Tree::build(entries);
        let root = tree.find(Path::new("crates/core")).unwrap();
        let mut expanded = tree.focus_expansion(root);
        let source = tree.find(Path::new("crates/core/src/file0.rs")).unwrap();
        let deeper = tree
            .find(Path::new("crates/core/src/nested/deeper.rs"))
            .unwrap();
        let shown = tree.visible(root, &expanded, 300);
        assert!(shown.contains(&source));
        assert!(!shown.contains(&deeper));
        assert_eq!(
            positions(&tree, &shown, root, Layout::Radial).len(),
            shown.len()
        );
        assert!(tree.visible(root, &expanded, 12).len() <= 12);
        expanded.remove(Path::new("crates/core/src"));
        assert!(!tree.visible(root, &expanded, 300).contains(&source));
        assert!(!shown.contains(&tree.find(Path::new("other")).unwrap()));
    }
    #[test]
    fn related_paths_reveal_complete_chains_within_the_view_budget() {
        let tree = Tree::build(vec![
            entry("src/orders.rs", 1),
            entry("src/pricing.rs", 1),
            entry("src/tax.rs", 1),
            entry("other/deep/file.rs", 1),
        ]);
        let selected = tree.find(Path::new("src/orders.rs")).unwrap();
        let related = [
            tree.find(Path::new("src/pricing.rs")).unwrap(),
            tree.find(Path::new("src/tax.rs")).unwrap(),
            tree.find(Path::new("other/deep/file.rs")).unwrap(),
        ];
        let shown = tree.visible_related(0, &HashSet::new(), 5, Some(selected), &related);
        assert_eq!(shown.len(), 5);
        assert!(shown.contains(&selected));
        assert!(shown.contains(&related[0]) && shown.contains(&related[1]));
        assert!(!shown.contains(&related[2]));
        assert_eq!(
            positions(&tree, &shown, 0, Layout::Radial).len(),
            shown.len()
        );
        let subtree = tree.find(Path::new("src")).unwrap();
        let shown = tree.visible_related(subtree, &HashSet::new(), 12, Some(selected), &related);
        assert!(!shown.contains(&related[2]));
        assert_eq!(
            positions(&tree, &shown, subtree, Layout::Tree).len(),
            shown.len()
        );
    }
    #[test]
    fn spider_is_opt_in_and_owns_no_agent_or_worktree_membership() {
        let settings = crate::Settings::default();
        assert!(!settings.spider_enabled);
        let mut json = serde_json::to_value(settings).unwrap();
        json.as_object_mut().unwrap().remove("spider_enabled");
        let old: crate::Settings = serde_json::from_value(json).unwrap();
        assert!(!old.spider_enabled);
        let config = Config {
            scope: Scope::Worktree(uuid::Uuid::new_v4()),
            ..Config::default()
        };
        let instance = crate::Instance::spider(uuid::Uuid::new_v4(), config.clone());
        assert_eq!(instance.kind, crate::InstanceKind::Spider);
        assert!(instance.worktree_id.is_none());
        assert!(instance.worktree_path.is_none());
        assert!(instance.tmux_session.is_none());
        assert!(instance.session_id.is_none());
        assert!(!instance.auto_submit);
        let restored: crate::Instance =
            serde_json::from_str(&serde_json::to_string(&instance).unwrap()).unwrap();
        assert_eq!(restored.spider, Some(config));
    }
    #[test]
    fn layout_ignores_metrics_and_input_order() {
        let a = Tree::build(vec![entry("a/x", 1), entry("b/y", 7)]);
        let b = Tree::build(vec![entry("b/y", 800), entry("a/x", 100)]);
        let expanded = HashSet::from(["a".into(), "b".into()]);
        let v = a.visible(0, &expanded, 300);
        assert_eq!(
            positions(&a, &v, 0, Layout::Radial),
            positions(&b, &v, 0, Layout::Radial)
        );
    }
    #[test]
    fn zoom_anchor_and_hit_geometry_agree() {
        let mut camera = Camera::default();
        let p = Point { x: 15.0, y: -10.0 };
        let anchor = camera.screen(p, 400.0, 200.0);
        camera.zoom_at(2.0, anchor, 400.0, 200.0);
        assert_eq!(camera.screen(p, 400.0, 200.0), anchor);
        let tree = Tree::build(vec![entry("a", 100_000)]);
        assert_eq!(
            hit_test(&[(0, p)], anchor, &tree, camera, 400.0, 200.0),
            Some(0)
        );
        camera.fit(&[(0, p)], 340.0, 50.0);
        assert!(camera.zoom.is_finite());
        camera.zoom = 5.0;
        camera.pan = Point::default();
        assert_eq!(
            hit_test(
                &[(0, Point::default())],
                Point { x: 240.0, y: 100.0 },
                &tree,
                camera,
                400.0,
                200.0
            ),
            None
        );
        assert_eq!(
            hit_test(
                &[(1, Point::default())],
                Point { x: 220.0, y: 120.0 },
                &tree,
                camera,
                400.0,
                200.0
            ),
            Some(1)
        );
    }
    #[test]
    fn large_tree_layout_stays_bounded() {
        let start = std::time::Instant::now();
        let tree = Tree::build(
            (0..100_000)
                .map(|i| entry(&format!("area{}/f{i}", i % 100), 1))
                .collect(),
        );
        let expanded = tree
            .nodes
            .iter()
            .filter(|n| n.kind == Kind::Directory)
            .map(|n| n.path.clone())
            .collect();
        let visible = tree.visible(0, &expanded, 300);
        assert_eq!(positions(&tree, &visible, 0, Layout::Radial).len(), 300);
        eprintln!(
            "Spider 100k entries: {:?}, {} model nodes, 300 painted nodes",
            start.elapsed(),
            tree.nodes.len()
        );
    }
    #[test]
    fn shown_tabs_follow_selection_and_maximize_without_hidden_consumers() {
        use crate::{LeafData, PaneNode, SplitDirection};
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let c = uuid::Uuid::new_v4();
        let layout = PaneNode::Split {
            direction: SplitDirection::Horizontal,
            sizes: vec![1.0, 1.0],
            children: vec![
                PaneNode::Leaf(LeafData {
                    pane_id: uuid::Uuid::new_v4(),
                    tabs: vec![a, b],
                    active: 0,
                }),
                PaneNode::leaf(c),
            ],
        };
        assert_eq!(super::visible_tabs(&layout, None, None), vec![a, c]);
        assert_eq!(super::visible_tabs(&layout, Some(b), None), vec![b, c]);
        assert_eq!(super::visible_tabs(&layout, Some(b), Some(a)), vec![b]);
        assert_eq!(super::visible_tabs(&layout, Some(b), Some(c)), vec![c]);
    }
}
