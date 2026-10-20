//! Local JS/TS import links, not a bundler or TypeScript type checker. Resolve
//! only unambiguous supplied files; package aliases and loader plugins are out.
use super::Graph;
use super::syntax::{MAX_IMPORTS, MAX_TOTAL_IMPORTS, normalize, parse, text};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use tree_sitter::Node;

const EXTENSIONS: &[&str] = &["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts"];

#[derive(Default)]
struct Facts {
    imports: Vec<(Option<String>, bool)>,
    require_shadowed: bool,
    partial: bool,
}
#[derive(Default)]
pub(super) struct JavaScriptIndex {
    files: BTreeMap<PathBuf, Facts>,
}

fn literal(node: Node, source: &str) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let value = text(node, source);
    let quote = value.as_bytes().first()?;
    if !matches!(quote, b'\'' | b'"') || value.as_bytes().last()? != quote {
        return None;
    }
    let value = value.get(1..value.len() - 1)?;
    // Escaped paths are uncommon and need JavaScript string decoding. Never
    // mistake an interpolation, escape, query or URL for a native local path.
    (!value.contains(['\\', '\n', '\r', '\0'])).then(|| value.to_string())
}
fn shadows_require(node: Node, source: &str) -> bool {
    if !matches!(
        node.kind(),
        "identifier" | "shorthand_property_identifier_pattern"
    ) || text(node, source) != "require"
    {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    matches!(
        parent.kind(),
        "formal_parameters"
            | "required_parameter"
            | "optional_parameter"
            | "import_clause"
            | "import_specifier"
            | "namespace_import"
            | "object_pattern"
            | "array_pattern"
            | "rest_pattern"
    ) || ["name", "parameter", "left"].into_iter().any(|field| {
        matches!(
            parent.kind(),
            "variable_declarator"
                | "function_declaration"
                | "function_expression"
                | "generator_function_declaration"
                | "arrow_function"
                | "catch_clause"
                | "assignment_expression"
        ) && parent
            .child_by_field_name(field)
            .is_some_and(|n| n.id() == node.id())
    })
}
impl JavaScriptIndex {
    pub(super) fn supported(path: &Path) -> bool {
        path.extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| EXTENSIONS.contains(&s))
    }
    pub(super) fn remove(&mut self, path: &Path) {
        self.files.remove(path);
    }
    pub(super) fn update(&mut self, path: PathBuf, source: &str) {
        self.remove(&path);
        if !super::valid_relative(&path) || !Self::supported(&path) {
            return;
        }
        let language = match path.extension().and_then(|s| s.to_str()) {
            Some("tsx") => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Some("ts" | "mts" | "cts") => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            _ => tree_sitter_javascript::LANGUAGE.into(),
        };
        let mut facts = Facts::default();
        let partial = parse(language, source, |node| {
            facts.require_shadowed |= shadows_require(node, source);
            let import = match node.kind() {
                "import_statement" | "export_statement" | "import_require_clause" => node
                    .child_by_field_name("source")
                    .map(|source_node| (literal(source_node, source), false)),
                "call_expression" => {
                    let function = node.child_by_field_name("function");
                    let require = function
                        .is_some_and(|f| f.kind() == "identifier" && text(f, source) == "require");
                    if require || function.is_some_and(|f| f.kind() == "import") {
                        let argument = node.child_by_field_name("arguments").and_then(|args| {
                            let mut cursor = args.walk();
                            args.named_children(&mut cursor)
                                .find(|n| n.kind() != "comment")
                        });
                        Some((argument.and_then(|arg| literal(arg, source)), require))
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(import) = import {
                if facts.imports.len() < MAX_IMPORTS {
                    facts.imports.push(import);
                } else {
                    facts.partial = true;
                }
            }
        });
        facts.partial |= partial;
        self.files.insert(path, facts);
    }
    fn resolve(&self, from: &Path, specifier: &str) -> Option<PathBuf> {
        let base = normalize(&from.parent()?.join(specifier))?;
        let mut candidates = BTreeSet::new();
        let extension = base.extension().and_then(|e| e.to_str());
        if let Some(extension) = extension.filter(|e| EXTENSIONS.contains(e)) {
            let extensions: &[&str] = match extension {
                "js" | "jsx" => &["ts", "tsx", "d.ts", "js", "jsx"],
                "mjs" => &["mts", "d.mts", "mjs"],
                "cjs" => &["cts", "d.cts", "cjs"],
                _ => &[extension],
            };
            for extension in extensions {
                let path = base.with_extension(extension);
                if self.files.contains_key(&path) {
                    candidates.insert(path);
                }
            }
        } else {
            // A relative path may name a file whose basename contains dots.
            if self.files.contains_key(&base) {
                candidates.insert(base.clone());
            }
            for extension in EXTENSIONS {
                let mut path = base.as_os_str().to_os_string();
                path.push(format!(".{extension}"));
                let path = PathBuf::from(path);
                if self.files.contains_key(&path) {
                    candidates.insert(path);
                }
            }
            if candidates.is_empty() {
                for extension in EXTENSIONS {
                    let path = base.join(format!("index.{extension}"));
                    if self.files.contains_key(&path) {
                        candidates.insert(path);
                    }
                }
            }
        }
        (candidates.len() == 1).then(|| candidates.pop_first().unwrap())
    }
    pub(super) fn graph(&self) -> Graph {
        let mut edges = BTreeSet::new();
        let mut unresolved = 0;
        let mut external = 0;
        let mut partial = self.files.values().any(|facts| facts.partial);
        let mut count = 0;
        'files: for (file, facts) in &self.files {
            for (specifier, require) in &facts.imports {
                count += 1;
                if count > MAX_TOTAL_IMPORTS {
                    partial = true;
                    break 'files;
                }
                let Some(specifier) = specifier else {
                    unresolved += 1;
                    partial = true;
                    continue;
                };
                if *require && facts.require_shadowed {
                    unresolved += 1;
                    partial = true;
                    continue;
                }
                if !specifier.starts_with('.') {
                    if specifier.starts_with('/') || specifier.is_empty() {
                        unresolved += 1;
                    } else {
                        external += 1;
                    }
                    continue;
                }
                if !specifier.starts_with("./") && !specifier.starts_with("../")
                    || specifier.contains(['\\', '?', '#', ':'])
                {
                    unresolved += 1;
                    continue;
                }
                if let Some(target) = self.resolve(file, specifier) {
                    edges.insert((file.clone(), target));
                } else {
                    unresolved += 1;
                }
            }
        }
        let mut graph = Graph::new(edges);
        graph.files = self.files.len();
        graph.unresolved = unresolved;
        graph.external = external;
        graph.partial = partial;
        graph
    }
}

#[cfg(test)]
mod tests {
    use super::JavaScriptIndex;
    use crate::spider::connections::Direction;
    use std::path::Path;
    fn targets(index: &JavaScriptIndex, file: &str) -> Vec<String> {
        index
            .graph()
            .relations(Path::new(file), false, 100)
            .into_iter()
            .filter(|r| r.direction == Direction::Uses)
            .map(|r| r.path.to_string_lossy().into())
            .collect()
    }
    #[test]
    fn esm_tsx_reexports_commonjs_and_literal_imports_resolve_real_files() {
        let mut index = JavaScriptIndex::default();
        for (file, source) in [
            (
                "src/main.tsx",
                "import type { Props } from './types.js'; import Card from './Card'; export * from '../shared'; const load = () => import('./lazy');",
            ),
            ("src/types.ts", "export interface Props {}"),
            (
                "src/Card.tsx",
                "export default function Card() { return <div/>; }",
            ),
            ("shared/index.ts", "export const shared = 1;"),
            ("src/lazy.ts", "export const lazy = 1;"),
            (
                "main.cjs",
                "const shared = require('./shared'); obj.require('./fake');",
            ),
            ("src/old.cts", "import shared = require('../shared');"),
            (
                "src/view.jsx",
                "import Card from './Card.tsx'; export default <Card/>;",
            ),
        ] {
            index.update(file.into(), source);
        }
        assert_eq!(
            targets(&index, "src/main.tsx"),
            [
                "shared/index.ts",
                "src/Card.tsx",
                "src/lazy.ts",
                "src/types.ts"
            ]
        );
        assert_eq!(targets(&index, "main.cjs"), ["shared/index.ts"]);
        assert_eq!(targets(&index, "src/old.cts"), ["shared/index.ts"]);
        assert_eq!(targets(&index, "src/view.jsx"), ["src/Card.tsx"]);
        assert!(!index.graph().partial);
    }
    #[test]
    fn comments_strings_aliases_dynamic_paths_and_escapes_never_invent_links() {
        let mut index = JavaScriptIndex::default();
        index.update(
            "main.js".into(),
            r#"
            // import './fake';
            const text = "require('./fake')";
            const html = <div>import './fake'</div>;
            import 'react'; import '@/fake';
            import('../outside'); import('./fake?raw'); import('./f\u0061ke');
            import(variable); obj.require('./fake');
        "#,
        );
        index.update("fake.js".into(), "");
        assert!(targets(&index, "main.js").is_empty());
        assert_eq!(index.graph().external, 2);
        assert!(index.graph().unresolved >= 4);
    }
    #[test]
    fn ambiguous_files_and_shadowed_require_are_unresolved_and_edits_remove_edges() {
        let mut index = JavaScriptIndex::default();
        index.update("main.ts".into(), "import './a';");
        index.update("a.ts".into(), "");
        index.update("a.js".into(), "");
        assert!(targets(&index, "main.ts").is_empty());
        assert_eq!(index.graph().unresolved, 1);
        index.remove(Path::new("a.js"));
        assert_eq!(targets(&index, "main.ts"), ["a.ts"]);
        index.update(
            "main.ts".into(),
            "function run(require) { return require('./a'); }",
        );
        assert!(targets(&index, "main.ts").is_empty());
        assert!(index.graph().partial);
        index.update("main.ts".into(), "export const done = true;");
        assert!(targets(&index, "main.ts").is_empty());
        assert!(!index.graph().partial);
    }
    #[test]
    fn dotted_basenames_module_variants_and_directory_indexes_resolve_without_crossing_root() {
        let mut index = JavaScriptIndex::default();
        for (file, source) in [
            (
                "src/main.ts",
                "import './user.service'; import './esm.mjs'; import './cjs.cjs'; import './folder'; import '../../outside';",
            ),
            ("src/user.service.ts", ""),
            ("src/esm.mts", ""),
            ("src/cjs.cts", ""),
            ("src/folder/index.jsx", ""),
            ("outside.ts", ""),
        ] {
            index.update(file.into(), source);
        }
        assert_eq!(
            targets(&index, "src/main.ts"),
            [
                "src/cjs.cts",
                "src/esm.mts",
                "src/folder/index.jsx",
                "src/user.service.ts"
            ]
        );
        assert_eq!(index.graph().unresolved, 1);
        index.update("src/broken.ts".into(), "import { from './user.service'");
        assert!(index.graph().partial);
    }
}
