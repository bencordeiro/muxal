//! Shared bounded syntax traversal and native relative path handling.
use std::path::{Component, Path, PathBuf};
use tree_sitter::{Language, Node};

pub(super) const MAX_IMPORTS: usize = 20_000;
pub(super) const MAX_TOTAL_IMPORTS: usize = 100_000;

pub(super) fn normalize(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => (),
            Component::ParentDir if out.pop() => (),
            _ => return None,
        }
    }
    Some(out)
}
pub(super) fn text<'a>(node: Node, source: &'a str) -> &'a str {
    source.get(node.byte_range()).unwrap_or_default()
}

/// Return partial coverage on errors or limits. Cursor traversal uses bounded
/// memory even for deeply nested input; comments/strings aren't token-matched.
pub(super) fn parse(language: Language, source: &str, mut visit: impl FnMut(Node)) -> bool {
    if source.len() > 2 * 1024 * 1024 {
        return true;
    }
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&language).is_err() {
        return true;
    }
    let Some(tree) = parser.parse(source, None) else {
        return true;
    };
    let mut partial = tree.root_node().has_error();
    let mut cursor = tree.walk();
    let mut visited = 0;
    loop {
        visited += 1;
        if visited > 200_000 {
            partial = true;
            break;
        }
        let node = cursor.node();
        if !node.is_error() && !node.is_missing() {
            visit(node);
            if cursor.goto_first_child() {
                continue;
            }
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return partial;
            }
        }
    }
    partial
}
