//! Directed file relationships and bounded traversal. Parsing/resolution supplies
//! the edges; this layer never interprets imports as runtime calls or authorship.
use super::valid_relative;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

pub mod rust;

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
    use super::{Direction, Graph};
    use std::path::Path;
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
