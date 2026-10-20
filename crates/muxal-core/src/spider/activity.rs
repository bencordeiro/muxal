//! Recent changes, timed by the caller's monotonic clock. No wall-clock/FS reads.
use super::{Tree, valid_relative};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const GLOW_DURATION: Duration = Duration::from_secs(25);
const MAX_MARKS: usize = 8192;

#[derive(Clone, Debug, Default)]
pub struct Activity {
    marks: HashMap<PathBuf, Duration>,
}
impl Activity {
    /// Retain observed paths, never fabricated ancestor activity. Projection
    /// onto the current map decides which file or collapsed folder gets a halo.
    pub fn observe(&mut self, paths: impl IntoIterator<Item = PathBuf>, now: Duration) {
        self.expire(now);
        for path in paths.into_iter().take(2048).filter(|p| valid_relative(p)) {
            self.marks
                .entry(path)
                .and_modify(|at| *at = (*at).max(now))
                .or_insert(now);
        }
        if self.marks.len() > MAX_MARKS {
            let mut oldest: Vec<_> = self.marks.iter().map(|(p, t)| (p.clone(), *t)).collect();
            oldest.sort_by(|(a, at), (b, bt)| at.cmp(bt).then_with(|| a.cmp(b)));
            for (path, _) in oldest.into_iter().take(self.marks.len() - MAX_MARKS) {
                self.marks.remove(&path);
            }
        }
    }
    pub fn expire(&mut self, now: Duration) {
        self.marks
            .retain(|_, at| now.saturating_sub(*at) < GLOW_DURATION);
    }
    pub fn strength(&self, path: &Path, now: Duration) -> f32 {
        self.marks.get(path).map_or(0.0, |at| {
            (1.0 - now.saturating_sub(*at).as_secs_f32() / GLOW_DURATION.as_secs_f32())
                .clamp(0.0, 1.0)
        })
    }
    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }
    /// Each observation appears exactly once, at its nearest visible node.
    /// Unknown/new paths can reach a visible containing folder. The checkout
    /// root (node 0) is always quiet, including when it is the only candidate.
    pub fn visible(&self, tree: &Tree, shown: &[usize]) -> Self {
        let shown: HashSet<_> = shown.iter().copied().collect();
        let mut marks = HashMap::<PathBuf, Duration>::new();
        for (path, at) in &self.marks {
            let mut node = path.ancestors().find_map(|p| tree.find(p));
            while let Some(id) = node {
                if id == 0 {
                    break;
                }
                if shown.contains(&id) {
                    marks
                        .entry(tree.nodes[id].path.clone())
                        .and_modify(|old| *old = (*old).max(*at))
                        .or_insert(*at);
                    break;
                }
                node = tree.nodes[id].parent;
            }
        }
        Self { marks }
    }
    pub fn changed_under(&self, path: &Path, now: Duration) -> bool {
        !path.as_os_str().is_empty()
            && self
                .marks
                .iter()
                .any(|(p, at)| p.starts_with(path) && now.saturating_sub(*at) < GLOW_DURATION)
    }
}

#[cfg(test)]
mod tests {
    use super::{Activity, GLOW_DURATION, MAX_MARKS};
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn observation_fades_restarts_and_records_only_actual_valid_paths() {
        let mut activity = Activity::default();
        assert_eq!(
            activity.strength(Path::new("src/a.rs"), Duration::ZERO),
            0.0
        );
        activity.observe(
            ["src/a.rs".into(), "../escape".into()],
            Duration::from_secs(10),
        );
        assert_eq!(
            activity.strength(Path::new("src/a.rs"), Duration::from_secs(10)),
            1.0
        );
        assert!(
            (activity.strength(Path::new("src/a.rs"), Duration::from_secs(20)) - 0.6).abs() < 0.01
        );
        for path in ["src", ""] {
            assert_eq!(
                activity.strength(Path::new(path), Duration::from_secs(10)),
                0.0
            );
        }
        assert_eq!(
            activity.strength(Path::new("../escape"), Duration::from_secs(10)),
            0.0
        );
        activity.observe(["src/a.rs".into()], Duration::from_secs(20));
        activity.expire(Duration::from_secs(35));
        assert!(activity.strength(Path::new("src/a.rs"), Duration::from_secs(35)) > 0.0);
        activity.expire(Duration::from_secs(20) + GLOW_DURATION);
        assert!(activity.is_empty());
    }

    #[test]
    fn activity_bursts_are_bounded() {
        let mut activity = Activity::default();
        for batch in 0..10 {
            activity.observe(
                (0..2048).map(|i| format!("area{batch}/f{i}").into()),
                Duration::from_secs(batch),
            );
        }
        assert!(activity.marks.len() <= MAX_MARKS);
        assert!(activity.strength(Path::new("area9/f2047"), Duration::from_secs(9)) > 0.0);
    }
    #[test]
    fn glow_moves_between_files_and_nearest_visible_folders_without_resetting_its_age() {
        use crate::spider::{Entry, Kind, Tree};
        let tree = Tree::build(
            ["src/deep/a.rs", "src/b.rs", "docs/readme"]
                .map(|p| Entry::new(p.into(), Kind::Text))
                .to_vec(),
        );
        let id = |p: &str| tree.find(Path::new(p)).unwrap();
        let mut activity = Activity::default();
        activity.observe(
            ["src/deep/a.rs".into(), "src/b.rs".into()],
            Duration::from_secs(10),
        );
        let collapsed = activity.visible(&tree, &[0, id("src"), id("docs")]);
        assert!(collapsed.strength(Path::new("src"), Duration::from_secs(15)) > 0.0);
        assert_eq!(
            collapsed.strength(Path::new(""), Duration::from_secs(15)),
            0.0
        );
        let open = activity.visible(
            &tree,
            &[0, id("src"), id("src/deep"), id("src/b.rs"), id("docs")],
        );
        assert_eq!(
            open.strength(Path::new("src"), Duration::from_secs(15)),
            0.0
        );
        assert!(open.strength(Path::new("src/deep"), Duration::from_secs(15)) > 0.0);
        assert!(open.strength(Path::new("src/b.rs"), Duration::from_secs(15)) > 0.0);
        let expanded = activity.visible(
            &tree,
            &[
                0,
                id("src"),
                id("src/deep"),
                id("src/deep/a.rs"),
                id("src/b.rs"),
            ],
        );
        assert_eq!(
            expanded.strength(Path::new("src/deep"), Duration::from_secs(20)),
            0.0
        );
        assert!(
            (expanded.strength(Path::new("src/deep/a.rs"), Duration::from_secs(20)) - 0.6).abs()
                < 0.01
        );
        assert_eq!(
            expanded.strength(Path::new("src/deep/a.rs"), Duration::from_secs(35)),
            0.0
        );
        let focused_elsewhere = activity.visible(&tree, &[id("docs"), id("docs/readme")]);
        assert!(focused_elsewhere.is_empty());
    }
    #[test]
    fn new_paths_reach_a_visible_folder_but_never_light_up_the_project_root() {
        use crate::spider::{Entry, Kind, Tree};
        let tree = Tree::build(vec![Entry::new("src/existing.rs".into(), Kind::Text)]);
        let mut activity = Activity::default();
        activity.observe(
            ["src/new.rs".into(), "new-at-root.rs".into()],
            Duration::ZERO,
        );
        let shown = activity.visible(&tree, &[0, tree.find(Path::new("src")).unwrap()]);
        assert_eq!(shown.strength(Path::new("src"), Duration::ZERO), 1.0);
        assert_eq!(shown.strength(Path::new(""), Duration::ZERO), 0.0);
    }
}
