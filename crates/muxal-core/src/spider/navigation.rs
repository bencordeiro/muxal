//! Expansion-first navigation and bounded geometric readability checks. This
//! runs only for deliberate navigation; resizing/panning never changes scope.
use super::{Camera, Kind, Layout, MAX_VISIBLE_NODES, Point, Tree, node_label, positions};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub width: f32,
    pub height: f32,
}
impl Viewport {
    pub fn budget(self, layout: Layout) -> usize {
        let budget = ((self.width * self.height / 2000.0) as usize).clamp(12, MAX_VISIBLE_NODES);
        if layout == Layout::Tree {
            budget.min(((self.height / 18.0) as usize).max(12))
        } else {
            budget
        }
    }
}

#[derive(Clone, Copy)]
struct Rect {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}
impl Rect {
    fn overlaps(self, other: Self) -> bool {
        self.left < other.right
            && self.right > other.left
            && self.top < other.bottom
            && self.bottom > other.top
    }
}

fn rectangles(
    tree: &Tree,
    points: &[(usize, Point)],
    camera: Camera,
    view: Viewport,
) -> Vec<(Rect, Rect)> {
    let mut children = HashMap::<usize, usize>::new();
    for (id, _) in points {
        if let Some(parent) = tree.nodes[*id].parent {
            *children.entry(parent).or_default() += 1;
        }
    }
    points
        .iter()
        .map(|(id, point)| {
            let node = &tree.nodes[*id];
            let center = camera.screen(*point, view.width, view.height);
            let radius = (node.radius() * camera.zoom).clamp(3.0, 30.0);
            let hidden = node
                .children
                .len()
                .saturating_sub(children.get(id).copied().unwrap_or(0));
            let label = node_label(node, hidden, "Project root");
            let hit_radius = (radius + 3.0).max(9.0);
            (
                Rect {
                    left: center.x - hit_radius,
                    top: center.y - hit_radius,
                    right: center.x + hit_radius,
                    bottom: center.y + hit_radius,
                },
                Rect {
                    left: center.x + radius + 5.0,
                    top: center.y - 8.0,
                    right: center.x + radius + 5.0 + label.chars().count() as f32 * 6.6,
                    bottom: center.y + 9.0,
                },
            )
        })
        .collect()
}

/// Fit node and constant-size label bounds, rather than only node centers.
pub fn fit_map(tree: &Tree, points: &[(usize, Point)], view: Viewport) -> Camera {
    let mut camera = Camera::default();
    camera.fit(points, view.width, view.height);
    if points.is_empty() {
        return camera;
    }
    for _ in 0..8 {
        let rects = rectangles(tree, points, camera, view);
        let left = rects
            .iter()
            .map(|(n, _)| n.left)
            .fold(f32::INFINITY, f32::min);
        let top = rects
            .iter()
            .map(|(n, l)| n.top.min(l.top))
            .fold(f32::INFINITY, f32::min);
        let right = rects
            .iter()
            .map(|(n, l)| n.right.max(l.right))
            .fold(f32::NEG_INFINITY, f32::max);
        let bottom = rects
            .iter()
            .map(|(n, l)| n.bottom.max(l.bottom))
            .fold(f32::NEG_INFINITY, f32::max);
        camera.pan.x += view.width * 0.5 - (left + right) * 0.5;
        camera.pan.y += view.height * 0.5 - (top + bottom) * 0.5;
        let scale = ((view.width - 24.0).max(1.0) / (right - left).max(1.0))
            .min((view.height - 24.0).max(1.0) / (bottom - top).max(1.0))
            .min(1.0);
        if scale >= 0.999 || camera.zoom <= 0.15 {
            break;
        }
        camera.zoom = (camera.zoom * scale).max(0.15);
    }
    camera
}

#[derive(Clone, Copy, Debug)]
pub struct Readability {
    pub over_budget: bool,
    pub overlapping_nodes: usize,
    pub obscured_labels: usize,
    pub labels: usize,
    pub clipped: bool,
}
impl Readability {
    pub fn crowded(self) -> bool {
        self.crowded_after_labels(0)
    }
    /// An expansion should not lose context because unrelated labels were
    /// already suppressed in the overview. Hard geometry limits still apply.
    pub fn crowded_after(self, previous: Self) -> bool {
        self.crowded_after_labels(previous.obscured_labels)
    }
    fn crowded_after_labels(self, already_obscured: usize) -> bool {
        self.over_budget
            || self.clipped
            || self.overlapping_nodes > 0
            || self.obscured_labels > (self.labels / 5).max(1).max(already_obscured)
    }
}

pub fn readability(
    tree: &Tree,
    points: &[(usize, Point)],
    view: Viewport,
    layout: Layout,
) -> Readability {
    let camera = fit_map(tree, points, view);
    let rects = rectangles(tree, points, camera, view);
    let mut overlapping_nodes = 0;
    for (i, (node, _)) in rects.iter().enumerate() {
        for (other_node, _) in rects.iter().skip(i + 1) {
            overlapping_nodes += usize::from(node.overlaps(*other_node));
        }
    }
    // Match the renderer's directory-first label suppression, rather than
    // counting all pair intersections or treating one busy cluster as one loss.
    let mut order: Vec<_> = (0..points.len()).collect();
    order.sort_by_key(|i| {
        (
            tree.nodes[points[*i].0].kind != Kind::Directory,
            points[*i].0,
        )
    });
    let mut placed: Vec<Rect> = Vec::new();
    let mut obscured_labels = 0;
    for i in order {
        let label = rects[i].1;
        if rects
            .iter()
            .enumerate()
            .any(|(j, (node, _))| i != j && label.overlaps(*node))
            || placed.iter().any(|other| label.overlaps(*other))
        {
            obscured_labels += 1;
        } else {
            placed.push(label);
        }
    }
    Readability {
        over_budget: points.len() > view.budget(layout),
        overlapping_nodes,
        obscured_labels,
        labels: points.len(),
        clipped: rects.iter().any(|(n, l)| {
            n.left < 0.0
                || n.top < 0.0
                || l.top < 0.0
                || n.right > view.width
                || l.right > view.width
                || n.bottom > view.height
                || l.bottom > view.height
        }),
    }
}

#[derive(Clone, Copy)]
pub struct MapContext<'a> {
    pub tree: &'a Tree,
    pub view: Viewport,
    pub layout: Layout,
}
impl MapContext<'_> {
    fn readability(
        self,
        root: &Path,
        expanded: &HashSet<PathBuf>,
        selected: usize,
        related: &[usize],
    ) -> Readability {
        let root = self.tree.find(root).unwrap_or(0);
        let visible = self.tree.visible_related(
            root,
            expanded,
            MAX_VISIBLE_NODES + 1,
            Some(selected),
            related,
        );
        let points = positions(self.tree, &visible, root, self.layout);
        readability(self.tree, &points, self.view, self.layout)
    }
}

#[derive(Clone)]
struct ReturnView {
    focused: PathBuf,
    expanded: HashSet<PathBuf>,
    selected: PathBuf,
    camera: Camera,
}

#[derive(Clone, Default)]
pub struct Navigation {
    pub focused: PathBuf,
    pub expanded: HashSet<PathBuf>,
    pub selected: PathBuf,
    history: Vec<ReturnView>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    None,
    Fit,
    Restore(Camera),
}

impl Navigation {
    /// Return to this checkout's collapsed overview without changing its scope.
    /// Back can restore the exact branch arrangement and camera from before it.
    pub fn overview(&mut self, camera: Camera) -> Effect {
        if !self.focused.as_os_str().is_empty()
            || !self.expanded.is_empty()
            || !self.selected.as_os_str().is_empty()
        {
            self.remember(camera);
        }
        self.focused.clear();
        self.expanded.clear();
        self.selected.clear();
        Effect::Fit
    }
    pub fn can_go_back(&self) -> bool {
        !self.history.is_empty() || !self.focused.as_os_str().is_empty()
    }
    fn remember(&mut self, camera: Camera) {
        if self.history.len() == 32 {
            self.history.remove(0);
        }
        self.history.push(ReturnView {
            focused: self.focused.clone(),
            expanded: self.expanded.clone(),
            selected: self.selected.clone(),
            camera,
        });
    }
    pub fn back(&mut self, tree: &Tree) -> Effect {
        while let Some(previous) = self.history.pop() {
            if tree.find(&previous.focused).is_none() {
                continue;
            }
            self.focused = previous.focused;
            self.expanded = previous.expanded;
            self.selected = previous.selected;
            self.reconcile(tree);
            return Effect::Restore(previous.camera);
        }
        if !self.focused.as_os_str().is_empty() {
            let closed = self.focused.clone();
            self.focused = closed.parent().unwrap_or(Path::new("")).to_path_buf();
            self.expanded.remove(&closed);
            self.selected = closed;
        } else {
            self.expanded.clear();
        }
        Effect::Fit
    }
    pub fn reconcile(&mut self, tree: &Tree) -> bool {
        let reset = tree.find(&self.focused).is_none();
        if reset {
            *self = Self::default();
        }
        if tree.find(&self.selected).is_none() {
            self.selected = self.focused.clone();
        }
        self.expanded.retain(|p| {
            tree.find(p)
                .is_some_and(|i| tree.nodes[i].kind == Kind::Directory)
        });
        reset
    }
    /// Double-click, Enter and Space all open/close the same branch. Narrowing
    /// is a fallback after evaluating the actual expanded geometry in this pane.
    pub fn toggle(&mut self, context: MapContext<'_>, camera: Camera) -> Effect {
        let Some(id) = context.tree.find(&self.selected) else {
            return Effect::None;
        };
        if context.tree.nodes[id].kind != Kind::Directory {
            return Effect::None;
        }
        if self.selected == self.focused {
            return self.back(context.tree);
        }
        if self.expanded.remove(&self.selected) {
            return Effect::Fit;
        }
        let mut expanded = self.expanded.clone();
        expanded.insert(self.selected.clone());
        expanded.extend(context.tree.focus_expansion(id));
        let previous = context.readability(&self.focused, &self.expanded, id, &[]);
        let candidate = context.readability(&self.focused, &expanded, id, &[]);
        if candidate.crowded_after(previous) {
            self.remember(camera);
            self.focused = self.selected.clone();
        }
        self.expanded = expanded;
        Effect::Fit
    }
    /// Finding a file retains the wider graph when readable, including required
    /// ancestors and connection targets; a cramped result gets a reversible focus.
    pub fn reveal(
        &mut self,
        context: MapContext<'_>,
        path: PathBuf,
        related: &[usize],
        camera: Camera,
    ) -> Effect {
        let Some(id) = context.tree.find(&path) else {
            return Effect::None;
        };
        self.remember(camera);
        self.selected = path.clone();
        self.focused.clear();
        if context
            .readability(&self.focused, &self.expanded, id, related)
            .crowded()
        {
            self.focused = if context.tree.nodes[id].kind == Kind::Directory {
                path
            } else {
                path.parent().unwrap_or(Path::new("")).to_path_buf()
            };
        }
        Effect::Fit
    }
}

#[cfg(test)]
mod tests {
    use super::{Camera, Effect, MapContext, Navigation, Viewport, readability};
    use crate::spider::{Entry, Kind, Layout, Point, Tree, positions};
    use std::path::Path;
    fn tree(paths: &[&str]) -> Tree {
        Tree::build(
            paths
                .iter()
                .map(|p| Entry::new((*p).into(), Kind::Text))
                .collect(),
        )
    }
    // A broad checkout with a busy root, tiny config folders, and a large
    // source branch. Include text sizes: zero-size dots hide the regression.
    fn broad_checkout() -> Tree {
        let mut entries: Vec<_> = [
            (".cargo/config.toml", 5),
            (".github/workflows/ci.yml", 81),
            (".github/workflows/release.yml", 351),
            (".gitignore", 48),
            ("AGENTS.md", 373),
            ("CONTRIBUTING.md", 35),
            ("Cargo.lock", 10879),
            ("Cargo.toml", 60),
            ("FEATURES.md", 818),
            ("LICENSE", 674),
            ("LICENSING.md", 43),
            ("README.md", 145),
            ("add-update.md", 95),
            ("docs/plan.md", 3353),
            ("ios/App/Main.swift", 12800),
            ("packaging/macos/Info.plist", 30),
            ("packaging/macos/app.entitlements", 10),
            ("packaging/app.desktop", 12),
            ("rust-toolchain.toml", 4),
            ("scripts/dev.sh", 1076),
        ]
        .into_iter()
        .map(|(path, lines)| {
            let mut entry = Entry::new(path.into(), Kind::Text);
            entry.lines = Some(lines);
            entry
        })
        .collect();
        for name in ["app", "core", "store", "terminal", "tray"] {
            for file in ["Cargo.toml", "src/lib.rs", "src/model.rs"] {
                let mut entry = Entry::new(format!("crates/{name}/{file}").into(), Kind::Text);
                entry.lines = Some(5000);
                entries.push(entry);
            }
        }
        Tree::build(entries)
    }
    #[test]
    fn tiny_folders_in_a_large_checkout_preserve_context_despite_existing_label_crowding() {
        let tree = broad_checkout();
        for (width, height) in [(700.0, 700.0), (1172.0, 798.0), (1600.0, 900.0)] {
            let context = MapContext {
                tree: &tree,
                view: Viewport { width, height },
                layout: Layout::Radial,
            };
            let baseline = context.readability(Path::new(""), &Default::default(), 0, &[]);
            assert!(
                baseline.crowded(),
                "fixture must exercise pre-existing crowding"
            );
            for sequence in [[".cargo", ".github"], [".github", ".cargo"]] {
                let mut nav = Navigation::default();
                for folder in sequence {
                    nav.selected = folder.into();
                    nav.toggle(context, Camera::default());
                    assert!(
                        nav.focused.as_os_str().is_empty(),
                        "{folder} unexpectedly narrowed at {width}x{height}"
                    );
                }
                let shown = tree.visible(0, &nav.expanded, 300);
                for path in [
                    ".cargo/config.toml",
                    ".github/workflows/ci.yml",
                    "README.md",
                    "crates",
                ] {
                    assert!(
                        shown.contains(&tree.find(Path::new(path)).unwrap()),
                        "missing {path}"
                    );
                }
                nav.selected = ".cargo".into();
                nav.toggle(context, Camera::default());
                assert!(!nav.expanded.contains(Path::new(".cargo")));
                assert!(nav.expanded.contains(Path::new(".github")));
                assert!(!nav.can_go_back());
            }
        }
    }
    #[test]
    fn existing_label_crowding_does_not_excuse_new_node_collisions() {
        let tree = broad_checkout();
        let context = MapContext {
            tree: &tree,
            view: Viewport {
                width: 1172.0,
                height: 798.0,
            },
            layout: Layout::Radial,
        };
        let mut nav = Navigation {
            selected: "crates".into(),
            ..Navigation::default()
        };
        nav.toggle(context, Camera::default());
        assert_eq!(nav.focused, Path::new("crates"));
        assert!(nav.can_go_back());
        nav.back(&tree);
        assert!(nav.focused.as_os_str().is_empty());
    }
    #[test]
    fn checkout_overview_is_reversible_from_expanded_and_focused_views() {
        let tree = tree(&["src/domain/a.rs", "docs/readme.md"]);
        for focused in ["", "src"] {
            let mut nav = Navigation {
                focused: focused.into(),
                expanded: ["src".into(), "src/domain".into()].into(),
                selected: "src/domain/a.rs".into(),
                ..Navigation::default()
            };
            let camera = Camera {
                zoom: 0.7,
                pan: Point { x: 42.0, y: -17.0 },
            };
            assert_eq!(nav.overview(camera), Effect::Fit);
            assert!(nav.focused.as_os_str().is_empty());
            assert!(nav.selected.as_os_str().is_empty());
            assert!(nav.expanded.is_empty());
            assert_eq!(nav.back(&tree), Effect::Restore(camera));
            assert_eq!(nav.focused, Path::new(focused));
            assert_eq!(nav.selected, Path::new("src/domain/a.rs"));
            assert!(nav.expanded.contains(Path::new("src/domain")));
        }
    }
    #[test]
    fn small_nested_projects_expand_and_collapse_without_losing_other_branches() {
        let tree = tree(&["src/domain/a.rs", "src/ui/b.rs", "docs/readme.md"]);
        for layout in [Layout::Radial, Layout::Tree] {
            let mut nav = Navigation {
                selected: "src".into(),
                ..Navigation::default()
            };
            let context = MapContext {
                tree: &tree,
                view: Viewport {
                    width: 1400.0,
                    height: 900.0,
                },
                layout,
            };
            assert_eq!(nav.toggle(context, Camera::default()), Effect::Fit);
            assert!(nav.focused.as_os_str().is_empty());
            let shown = tree.visible(0, &nav.expanded, 300);
            assert!(shown.contains(&tree.find(Path::new("docs")).unwrap()));
            assert!(shown.contains(&tree.find(Path::new("src/domain/a.rs")).unwrap()));
            nav.toggle(context, Camera::default());
            assert!(
                !tree
                    .visible(0, &nav.expanded, 300)
                    .contains(&tree.find(Path::new("src/domain")).unwrap())
            );
            assert!(!nav.can_go_back());
        }
    }
    #[test]
    fn crowded_expansion_focuses_and_center_double_click_restores_the_exact_previous_view() {
        let mut paths: Vec<_> = (0..24)
            .map(|i| format!("source/branch{i}/long_file_name_{i}.rs"))
            .collect();
        paths.push("docs/readme.md".into());
        let tree = tree(&paths.iter().map(String::as_str).collect::<Vec<_>>());
        let mut nav = Navigation {
            selected: "source".into(),
            ..Navigation::default()
        };
        let camera = Camera {
            pan: Point { x: 73.0, y: -19.0 },
            zoom: 0.83,
        };
        let context = MapContext {
            tree: &tree,
            view: Viewport {
                width: 340.0,
                height: 300.0,
            },
            layout: Layout::Radial,
        };
        nav.toggle(context, camera);
        assert_eq!(nav.focused, Path::new("source"));
        assert!(nav.can_go_back());
        assert_eq!(
            nav.toggle(context, Camera::default()),
            Effect::Restore(camera)
        );
        assert!(nav.focused.as_os_str().is_empty());
        assert!(!nav.expanded.contains(Path::new("source")));
        assert!(!nav.can_go_back());
    }
    #[test]
    fn several_nested_expansions_keep_the_whole_small_project_in_view() {
        let tree = tree(&[
            "Cargo.toml",
            "README.md",
            "docs/design.md",
            "tests/smoke.rs",
            "src/lib.rs",
            "src/domain/mod.rs",
            "src/domain/accounts.rs",
            "src/domain/orders/mod.rs",
            "src/domain/orders/pricing.rs",
            "src/ui/mod.rs",
            "src/ui/dashboard.rs",
        ]);
        let mut nav = Navigation {
            selected: "src".into(),
            ..Navigation::default()
        };
        let context = MapContext {
            tree: &tree,
            view: Viewport {
                width: 1000.0,
                height: 800.0,
            },
            layout: Layout::Radial,
        };
        nav.toggle(context, Camera::default());
        assert!(
            nav.focused.as_os_str().is_empty(),
            "first expansion narrowed"
        );
        nav.selected = "src/domain/orders".into();
        nav.toggle(context, Camera::default());
        assert!(nav.focused.as_os_str().is_empty());
        let shown = tree.visible(0, &nav.expanded, 300);
        assert!(
            shown.contains(
                &tree
                    .find(Path::new("src/domain/orders/pricing.rs"))
                    .unwrap()
            )
        );
        assert!(shown.contains(&tree.find(Path::new("src/ui/dashboard.rs")).unwrap()));
        assert!(shown.contains(&tree.find(Path::new("docs")).unwrap()));
    }
    #[test]
    fn geometry_and_viewport_determine_crowding_instead_of_total_project_size() {
        let tree = tree(&["one/a.rs", "two/b.rs", "three/c.rs"]);
        let visible = tree.visible(0, &Default::default(), 300);
        let points = positions(&tree, &visible, 0, Layout::Radial);
        assert!(
            !readability(
                &tree,
                &points,
                Viewport {
                    width: 1000.0,
                    height: 700.0
                },
                Layout::Radial
            )
            .crowded()
        );
        assert!(
            readability(
                &tree,
                &points,
                Viewport {
                    width: 180.0,
                    height: 100.0
                },
                Layout::Radial
            )
            .crowded()
        );
        let collided: Vec<_> = visible.iter().map(|i| (*i, Point::default())).collect();
        let report = readability(
            &tree,
            &collided,
            Viewport {
                width: 1000.0,
                height: 700.0,
            },
            Layout::Radial,
        );
        assert!(report.overlapping_nodes > 0 && report.crowded());
    }
    #[test]
    fn refresh_and_search_do_not_restore_deleted_paths() {
        let first = tree(&["src/a.rs", "docs/readme"]);
        let mut nav = Navigation::default();
        let context = MapContext {
            tree: &first,
            view: Viewport {
                width: 1000.0,
                height: 700.0,
            },
            layout: Layout::Radial,
        };
        nav.reveal(context, "src/a.rs".into(), &[], Camera::default());
        assert!(nav.can_go_back());
        let second = tree(&["other/b.rs"]);
        nav.reconcile(&second);
        nav.back(&second);
        assert!(second.find(&nav.selected).is_some());
        assert!(second.find(&nav.focused).is_some());
    }
}
