//! Directed file relationships and bounded traversal. Parsing/resolution supplies
//! the edges; this layer never interprets imports as runtime calls or authorship.
use super::valid_relative;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

mod javascript;
mod python;
pub mod rust;
mod syntax;

/// The supported language set is deliberate: Rust, JS/TS (including JSX/TSX),
/// and Python. All parsers consume caller-supplied strings; none perform I/O.
#[derive(Default)]
pub struct Index {
    rust: rust::RustIndex,
    javascript: javascript::JavaScriptIndex,
    python: python::PythonIndex,
}
impl Index {
    pub fn supported(path: &Path) -> bool {
        rust::RustIndex::supported(path)
            || javascript::JavaScriptIndex::supported(path)
            || python::PythonIndex::supported(path)
    }
    pub fn remove(&mut self, path: &Path) {
        self.rust.remove(path);
        self.javascript.remove(path);
        self.python.remove(path);
    }
    pub fn update(&mut self, path: PathBuf, source: &str) {
        if rust::RustIndex::supported(&path) {
            self.rust.update(path, source);
        } else if javascript::JavaScriptIndex::supported(&path) {
            self.javascript.update(path, source);
        } else if python::PythonIndex::supported(&path) {
            self.python.update(path, source);
        }
    }
    pub fn graph(&self) -> Graph {
        Graph::combine([
            self.rust.graph(),
            self.javascript.graph(),
            self.python.graph(),
        ])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Uses,
    UsedBy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relation {
    pub path: PathBuf,
    pub direction: Direction,
    pub depth: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Graph {
    outgoing: HashMap<PathBuf, Vec<PathBuf>>,
    incoming: HashMap<PathBuf, Vec<PathBuf>>,
    pub edges: usize,
    pub files: usize,
    pub unresolved: usize,
    pub external: usize,
    pub partial: bool,
}
impl Graph {
    fn combine(graphs: impl IntoIterator<Item = Self>) -> Self {
        let mut edges = BTreeSet::new();
        let mut files = 0;
        let mut unresolved = 0;
        let mut external = 0;
        let mut partial = false;
        for graph in graphs {
            files += graph.files;
            unresolved += graph.unresolved;
            external += graph.external;
            partial |= graph.partial;
            for (from, targets) in graph.outgoing {
                for to in targets {
                    edges.insert((from.clone(), to));
                    if edges.len() > 100_000 {
                        edges.pop_last();
                        partial = true;
                    }
                }
            }
        }
        let mut graph = Self::new(edges);
        graph.files = files;
        graph.unresolved = unresolved;
        graph.external = external;
        graph.partial = partial;
        graph
    }
    pub fn new(edges: impl IntoIterator<Item = (PathBuf, PathBuf)>) -> Self {
        let edges: BTreeSet<_> = edges
            .into_iter()
            .filter(|(a, b)| a != b && valid_relative(a) && valid_relative(b))
            .take(100_000)
            .collect();
        let mut graph = Self {
            edges: edges.len(),
            ..Self::default()
        };
        for (a, b) in edges {
            graph.outgoing.entry(a.clone()).or_default().push(b.clone());
            graph.incoming.entry(b).or_default().push(a);
        }
        graph
    }
    /// Incoming and outgoing walks are separate: don't infer a dependency from
    /// two files merely sharing an import. Cycles never return the starting file.
    pub fn relations(&self, path: &Path, indirect: bool, limit: usize) -> Vec<Relation> {
        let mut out = Vec::new();
        for direction in [Direction::Uses, Direction::UsedBy] {
            let table = if direction == Direction::Uses {
                &self.outgoing
            } else {
                &self.incoming
            };
            let mut seen = HashSet::from([path.to_path_buf()]);
            let mut queue = VecDeque::from([(path.to_path_buf(), 0usize)]);
            while let Some((at, depth)) = queue.pop_front() {
                if depth >= if indirect { 3 } else { 1 } {
                    continue;
                }
                for next in table.get(&at).into_iter().flatten() {
                    if out.len() >= limit {
                        return out;
                    }
                    if seen.insert(next.clone()) {
                        out.push(Relation {
                            path: next.clone(),
                            direction,
                            depth: depth + 1,
                        });
                        queue.push_back((next.clone(), depth + 1));
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{Direction, Graph, Index};
    use std::path::Path;
    #[test]
    fn mixed_language_index_keeps_incoming_links_updates_and_scope_limits() {
        let mut index = Index::default();
        for (path, source) in [
            (
                "Cargo.toml",
                "[package]\nname='mixed'\nversion='0.1.0'\nedition='2024'\n",
            ),
            ("src/lib.rs", "mod model;"),
            ("src/model.rs", ""),
            ("web/app.tsx", "import './view';"),
            ("web/view.jsx", "export default <div/>;"),
            ("scripts/main.py", "import helper\n"),
            ("scripts/helper.py", ""),
        ] {
            index.update(path.into(), source);
        }
        let graph = index.graph();
        assert_eq!(graph.files, 6);
        assert_eq!(graph.edges, 3);
        assert!(!graph.partial);
        let incoming = graph.relations(Path::new("web/view.jsx"), false, 10);
        assert_eq!(incoming[0].path, Path::new("web/app.tsx"));
        assert_eq!(incoming[0].direction, Direction::UsedBy);
        index.remove(Path::new("scripts/helper.py"));
        assert!(
            index
                .graph()
                .relations(Path::new("scripts/main.py"), false, 10)
                .is_empty()
        );
        for file in [
            "a.ts", "a.tsx", "a.jsx", "a.js", "a.mts", "a.cjs", "a.py", "a.rs",
        ] {
            assert!(Index::supported(Path::new(file)), "{file}");
        }
        for file in ["a.go", "a.cs", "a.rb", "a.vue", "data.json"] {
            assert!(!Index::supported(Path::new(file)), "{file}");
        }
    }
    #[test]
    fn merging_languages_keeps_the_total_edge_budget_deterministic_and_explicit() {
        let make = |prefix: &str| {
            Graph::new((0..60_000).map(|n| {
                (
                    format!("{prefix}/main").into(),
                    format!("{prefix}/file{n:05}").into(),
                )
            }))
        };
        let a = Graph::combine([make("web"), make("python")]);
        let b = Graph::combine([make("python"), make("web")]);
        assert_eq!(a.edges, 100_000);
        assert!(a.partial);
        assert_eq!(a.outgoing, b.outgoing);
        assert_eq!(a.incoming, b.incoming);
    }
    #[test]
    fn directional_relations_deduplicate_and_stop_at_cycles_and_budgets() {
        let graph = Graph::new(
            [
                ("a", "b"),
                ("a", "b"),
                ("b", "c"),
                ("c", "a"),
                ("unrelated", "b"),
                ("a", "a"),
                ("../bad", "a"),
            ]
            .map(|(a, b)| (a.into(), b.into())),
        );
        assert_eq!(graph.edges, 4);
        let direct = graph.relations(Path::new("a"), false, 100);
        assert_eq!(direct.len(), 2);
        assert_eq!(direct[0].direction, Direction::Uses);
        assert_eq!(direct[0].path, Path::new("b"));
        let all = graph.relations(Path::new("a"), true, 100);
        assert!(!all.iter().any(|r| r.path == Path::new("a")));
        assert!(
            !all.iter()
                .any(|r| r.path == Path::new("unrelated") && r.direction == Direction::Uses)
        );
        assert!(
            all.iter().any(|r| r.path == Path::new("c")
                && r.direction == Direction::Uses
                && r.depth == 2)
        );
        assert_eq!(graph.relations(Path::new("a"), true, 1).len(), 1);
    }
}
