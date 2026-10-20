//! Conservative Rust module/import relationships over supplied source strings.
//! No macro expansion, cfg evaluation, symbol/call inference, filesystem access,
//! Cargo execution, or language server. Unresolved local paths stay unresolved.
use super::Graph;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Component, Path, PathBuf};
use tree_sitter::Node;

const MAX_CONTEXTS: usize = 20_000;
const MAX_IMPORTS: usize = 100_000;

#[derive(Clone, Default)]
struct Facts {
    modules: Vec<Module>,
    imports: Vec<Import>,
    partial: bool,
    root_items: HashSet<String>,
}
#[derive(Clone)]
struct Module {
    scope: Vec<String>,
    name: String,
    inline: bool,
    path: Option<PathBuf>,
    unsupported: bool,
}
#[derive(Clone)]
struct Import {
    scope: Vec<String>,
    path: Vec<String>,
    binding: Option<String>,
}

#[derive(Clone, Default)]
pub struct RustIndex {
    files: BTreeMap<PathBuf, Facts>,
    manifests: BTreeMap<PathBuf, toml::Value>,
    invalid_manifests: HashSet<PathBuf>,
}

fn normalize(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Normal(part) => out.push(part),
            Component::CurDir => (),
            Component::ParentDir if out.pop() => (),
            _ => return None,
        }
    }
    Some(out)
}
fn text<'a>(node: Node, source: &'a str) -> &'a str {
    source.get(node.byte_range()).unwrap_or_default()
}
fn ident(node: Node, source: &str) -> String {
    text(node, source).trim_start_matches("r#").to_string()
}
fn segments(node: Node, source: &str) -> Vec<String> {
    if node.kind() == "scoped_identifier" {
        let mut path = node
            .child_by_field_name("path")
            .map(|n| segments(n, source))
            .unwrap_or_default();
        if let Some(name) = node.child_by_field_name("name") {
            path.push(ident(name, source));
        }
        path
    } else {
        vec![ident(node, source)]
    }
}
fn imports(node: Node, prefix: &[String], scope: &[String], source: &str, out: &mut Vec<Import>) {
    if out.len() >= MAX_IMPORTS {
        return;
    }
    match node.kind() {
        "scoped_use_list" => {
            let mut path = prefix.to_vec();
            if let Some(part) = node.child_by_field_name("path") {
                path.extend(segments(part, source));
            }
            if let Some(list) = node.child_by_field_name("list") {
                imports(list, &path, scope, source, out);
            }
        }
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                imports(child, prefix, scope, source, out);
            }
        }
        "use_as_clause" => {
            if let Some(path) = node.child_by_field_name("path") {
                let before = out.len();
                imports(path, prefix, scope, source, out);
                if out.len() > before
                    && let Some(alias) = node.child_by_field_name("alias")
                {
                    let alias = ident(alias, source);
                    out.last_mut().unwrap().binding = (alias != "_").then_some(alias);
                }
            }
        }
        "use_wildcard" => {
            let path = node
                .named_child(0)
                .map(|n| segments(n, source))
                .unwrap_or_default();
            let mut full = prefix.to_vec();
            full.extend(path);
            if !full.is_empty() {
                out.push(Import {
                    scope: scope.to_vec(),
                    path: full,
                    binding: None,
                });
            }
        }
        _ => {
            let mut path = prefix.to_vec();
            let tail = segments(node, source);
            if tail != ["self"] || path.is_empty() {
                path.extend(tail);
            }
            out.push(Import {
                scope: scope.to_vec(),
                binding: path
                    .last()
                    .filter(|s| !["crate", "self", "super"].contains(&s.as_str()))
                    .cloned(),
                path,
            });
        }
    }
}
fn string_value(node: Node, source: &str) -> Option<PathBuf> {
    let value = text(node, source);
    if node.kind() == "string_literal" {
        serde_json::from_str::<String>(value)
            .ok()
            .map(PathBuf::from)
    } else if node.kind() == "raw_string_literal" {
        let start = value.find('"')? + 1;
        let end = value.rfind('"')?;
        Some(PathBuf::from(value.get(start..end)?))
    } else {
        None
    }
}
fn collect(node: Node, source: &str, scope: &[String], facts: &mut Facts) {
    if scope.len() > 64 || facts.modules.len() >= MAX_CONTEXTS {
        facts.partial = true;
        return;
    }
    let mut attributes = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if facts.modules.len() >= MAX_CONTEXTS || facts.imports.len() >= MAX_IMPORTS {
            facts.partial = true;
            break;
        }
        if scope.is_empty()
            && matches!(
                child.kind(),
                "struct_item"
                    | "enum_item"
                    | "trait_item"
                    | "union_item"
                    | "type_item"
                    | "function_item"
                    | "const_item"
                    | "static_item"
            )
            && let Some(name) = child.child_by_field_name("name")
        {
            facts.root_items.insert(ident(name, source));
        }
        match child.kind() {
            "attribute_item" => {
                attributes.push(child);
                continue;
            }
            "mod_item" => {
                let Some(name) = child.child_by_field_name("name") else {
                    continue;
                };
                let name = ident(name, source);
                let mut path = None;
                let mut unsupported = false;
                for attr in &attributes {
                    if let Some(attr) = attr.named_child(0) {
                        let name = attr
                            .named_child(0)
                            .map(|n| text(n, source))
                            .unwrap_or_default();
                        if name == "path" {
                            path = attr
                                .child_by_field_name("value")
                                .and_then(|n| string_value(n, source));
                            unsupported |= path.is_none();
                        } else if name == "cfg_attr" {
                            unsupported = true;
                        }
                    }
                }
                let body = child.child_by_field_name("body");
                // Inline path overrides have different directory semantics;
                // leave those branches unresolved rather than guessing.
                unsupported |= body.is_some() && path.is_some();
                facts.modules.push(Module {
                    scope: scope.to_vec(),
                    name: name.clone(),
                    inline: body.is_some(),
                    path,
                    unsupported,
                });
                if let Some(body) = body
                    && !unsupported
                {
                    let mut nested = scope.to_vec();
                    nested.push(name);
                    collect(body, source, &nested, facts);
                }
            }
            "use_declaration" => {
                if let Some(argument) = child.child_by_field_name("argument") {
                    let before = facts.imports.len();
                    imports(argument, &[], scope, source, &mut facts.imports);
                    if scope.is_empty() {
                        facts.root_items.extend(
                            facts.imports[before..]
                                .iter()
                                .filter_map(|i| i.binding.clone()),
                        );
                    }
                }
            }
            "line_comment" | "block_comment" => continue,
            _ => (),
        }
        attributes.clear();
    }
}

#[derive(Clone)]
struct CrateRoot {
    file: PathBuf,
    dependencies: HashMap<String, Option<PathBuf>>,
    edition_2015: bool,
}
impl RustIndex {
    pub fn supported(path: &Path) -> bool {
        path.extension().is_some_and(|s| s == "rs")
            || path.file_name().is_some_and(|s| s == "Cargo.toml")
    }
    pub fn remove(&mut self, path: &Path) {
        self.files.remove(path);
        self.manifests.remove(path);
        self.invalid_manifests.remove(path);
    }
    pub fn update(&mut self, path: PathBuf, source: &str) {
        self.remove(&path);
        if !super::valid_relative(&path) {
            return;
        }
        if path.file_name().is_some_and(|s| s == "Cargo.toml") {
            match source.parse::<toml::Value>() {
                Ok(value) => {
                    self.manifests.insert(path, value);
                }
                Err(_) => {
                    self.invalid_manifests.insert(path);
                }
            }
        } else if path.extension().is_some_and(|s| s == "rs") {
            let mut parser = tree_sitter::Parser::new();
            let grammar = tree_sitter_rust::LANGUAGE.into();
            let mut facts = Facts::default();
            if parser.set_language(&grammar).is_ok()
                && let Some(tree) = parser.parse(source, None)
            {
                facts.partial = tree.root_node().has_error();
                collect(tree.root_node(), source, &[], &mut facts);
            } else {
                facts.partial = true;
            }
            self.files.insert(path, facts);
        }
    }
    fn library(&self, directory: &Path) -> Option<PathBuf> {
        let manifest = self.manifests.get(&directory.join("Cargo.toml"));
        let path = manifest
            .and_then(|m| m.get("lib"))
            .and_then(|m| m.get("path"))
            .and_then(|p| p.as_str())
            .unwrap_or("src/lib.rs");
        let path = normalize(&directory.join(path))?;
        self.files.contains_key(&path).then_some(path)
    }
    fn roots(&self) -> Vec<CrateRoot> {
        let mut roots = BTreeMap::new();
        for (manifest_path, manifest) in &self.manifests {
            let Some(package) = manifest.get("package") else {
                continue;
            };
            let directory = manifest_path.parent().unwrap_or(Path::new(""));
            let mut dependencies = HashMap::new();
            let mut tables = vec![manifest];
            if let Some(targets) = manifest.get("target").and_then(|v| v.as_table()) {
                tables.extend(targets.values());
            }
            for table in tables {
                for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
                    for (alias, value) in table
                        .get(section)
                        .and_then(|v| v.as_table())
                        .into_iter()
                        .flatten()
                    {
                        let mut base = directory;
                        let mut dependency = value;
                        if value.get("workspace").and_then(|v| v.as_bool()) == Some(true) {
                            for ancestor in directory.ancestors() {
                                if let Some(value) = self
                                    .manifests
                                    .get(&ancestor.join("Cargo.toml"))
                                    .and_then(|m| m.get("workspace"))
                                    .and_then(|w| w.get("dependencies"))
                                    .and_then(|d| d.get(alias))
                                {
                                    dependency = value;
                                    base = ancestor;
                                    break;
                                }
                            }
                        }
                        let target = dependency
                            .get("path")
                            .and_then(|v| v.as_str())
                            .and_then(|p| normalize(&base.join(p)))
                            .and_then(|p| self.library(&p));
                        let alias = alias.replace('-', "_");
                        dependencies
                            .entry(alias)
                            .and_modify(|old| {
                                if *old != target {
                                    *old = None;
                                }
                            })
                            .or_insert(target);
                    }
                }
            }
            let library = self.library(directory);
            if let Some(library) = &library {
                let name = manifest
                    .get("lib")
                    .and_then(|v| v.get("name"))
                    .and_then(|v| v.as_str())
                    .or_else(|| package.get("name").and_then(|v| v.as_str()));
                if let Some(name) = name {
                    dependencies.insert(name.replace('-', "_"), Some(library.clone()));
                }
            }
            let mut edition = package.get("edition").and_then(|v| v.as_str());
            if package
                .get("edition")
                .and_then(|v| v.get("workspace"))
                .and_then(|v| v.as_bool())
                == Some(true)
            {
                for ancestor in directory.ancestors() {
                    if let Some(inherited) = self
                        .manifests
                        .get(&ancestor.join("Cargo.toml"))
                        .and_then(|m| m.get("workspace"))
                        .and_then(|w| w.get("package"))
                        .and_then(|p| p.get("edition"))
                        .and_then(|e| e.as_str())
                    {
                        edition = Some(inherited);
                        break;
                    }
                }
            }
            let edition_2015 = edition.unwrap_or("2015") == "2015";
            let mut paths: Vec<_> = library.into_iter().collect();
            paths.push(directory.join("src/main.rs"));
            for bin in manifest
                .get("bin")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
            {
                if let Some(path) = bin
                    .get("path")
                    .and_then(|v| v.as_str())
                    .and_then(|p| normalize(&directory.join(p)))
                {
                    paths.push(path);
                }
            }
            for path in self.files.keys() {
                if let Ok(relative) = path.strip_prefix(directory) {
                    let parts: Vec<_> = relative.components().collect();
                    let auto = parts.first().is_some_and(|p| {
                        ["tests", "examples", "benches"]
                            .iter()
                            .any(|name| p.as_os_str() == *name)
                    }) && parts.len() == 2;
                    let bin = parts.len() >= 3
                        && parts[0].as_os_str() == "src"
                        && parts[1].as_os_str() == "bin"
                        && (parts.len() == 3
                            || (parts.len() == 4
                                && path.file_name().is_some_and(|s| s == "main.rs")));
                    if auto || bin {
                        paths.push(path.clone());
                    }
                }
            }
            for file in paths.into_iter().filter(|p| self.files.contains_key(p)) {
                roots.insert(
                    file.clone(),
                    CrateRoot {
                        file,
                        dependencies: dependencies.clone(),
                        edition_2015,
                    },
                );
            }
        }
        if roots.is_empty() {
            for file in self.files.keys().filter(|p| {
                p.file_name()
                    .is_some_and(|s| s == "lib.rs" || s == "main.rs")
            }) {
                roots.insert(
                    file.clone(),
                    CrateRoot {
                        file: file.clone(),
                        dependencies: HashMap::new(),
                        edition_2015: false,
                    },
                );
            }
        }
        roots.into_values().collect()
    }
    pub fn graph(&self) -> Graph {
        type Key = (PathBuf, Vec<String>);
        let roots = self.roots();
        let mut modules = HashMap::<Key, Option<PathBuf>>::new();
        let mut contexts = Vec::new();
        let mut edges = BTreeSet::new();
        let mut unresolved = 0;
        let mut partial =
            !self.invalid_manifests.is_empty() || self.files.values().any(|f| f.partial);
        for root in &roots {
            let mut queue = VecDeque::from([(
                Vec::<String>::new(),
                root.file.clone(),
                root.file.parent().unwrap_or(Path::new("")).to_path_buf(),
            )]);
            let mut seen = HashSet::new();
            while let Some((scope, file, directory)) = queue.pop_front() {
                if scope.len() > 64 || contexts.len() >= MAX_CONTEXTS {
                    partial = true;
                    break;
                }
                if !seen.insert((scope.clone(), file.clone())) {
                    continue;
                }
                let Some(facts) = self.files.get(&file) else {
                    continue;
                };
                let key = (root.file.clone(), scope.clone());
                modules
                    .entry(key)
                    .and_modify(|old| {
                        if old.as_ref() != Some(&file) {
                            *old = None;
                        }
                    })
                    .or_insert(Some(file.clone()));
                contexts.push((root, scope.clone(), file.clone()));
                for module in &facts.modules {
                    let mut nested = scope.clone();
                    nested.extend(module.scope.clone());
                    nested.push(module.name.clone());
                    if module.unsupported {
                        unresolved += 1;
                        continue;
                    }
                    if module.inline {
                        modules
                            .entry((root.file.clone(), nested))
                            .or_insert(Some(file.clone()));
                        continue;
                    }
                    let mut base = directory.clone();
                    for component in &module.scope {
                        base.push(component);
                    }
                    let candidates = if let Some(path) = &module.path {
                        let base = if module.scope.is_empty() {
                            file.parent().unwrap_or(Path::new(""))
                        } else {
                            &base
                        };
                        normalize(&base.join(path)).into_iter().collect::<Vec<_>>()
                    } else {
                        vec![
                            base.join(format!("{}.rs", module.name)),
                            base.join(&module.name).join("mod.rs"),
                        ]
                    };
                    let candidates: Vec<_> = candidates
                        .into_iter()
                        .filter(|p| self.files.contains_key(p))
                        .collect();
                    if candidates.len() != 1 {
                        unresolved += 1;
                        continue;
                    }
                    let target = candidates[0].clone();
                    edges.insert((file.clone(), target.clone()));
                    let next_dir = if target.file_name().is_some_and(|s| s == "mod.rs") {
                        target.parent().unwrap_or(Path::new("")).to_path_buf()
                    } else {
                        target.with_extension("")
                    };
                    queue.push_back((nested, target, next_dir));
                }
            }
        }
        let mut external = 0;
        for (root, context, file) in contexts {
            let facts = &self.files[&file];
            for import in &facts.imports {
                let mut path = import.path.clone();
                if path.is_empty() {
                    continue;
                }
                let mut module = context.clone();
                module.extend(import.scope.clone());
                let mut origin = root.file.clone();
                match path[0].as_str() {
                    "crate" => {
                        module.clear();
                        path.remove(0);
                    }
                    "self" => {
                        path.remove(0);
                    }
                    "super" => {
                        while path.first().is_some_and(|p| p == "super") {
                            if module.pop().is_none() {
                                break;
                            }
                            path.remove(0);
                        }
                    }
                    name if root.dependencies.contains_key(name) => {
                        if let Some(Some(target)) = root.dependencies.get(name) {
                            origin = target.clone();
                            module.clear();
                            path.remove(0);
                        } else {
                            external += 1;
                            continue;
                        }
                    }
                    "std" | "core" | "alloc" => {
                        external += 1;
                        continue;
                    }
                    _ if root.edition_2015 => module.clear(),
                    _ => (),
                }
                let root_item = path.first().cloned();
                module.extend(path);
                let mut target = None;
                while !module.is_empty() {
                    if let Some(value) = modules.get(&(origin.clone(), module.clone())) {
                        target = value.clone();
                        break;
                    }
                    module.pop();
                }
                // A root symbol must actually be declared there. Never turn an
                // unknown module path into a guessed edge to lib.rs/main.rs.
                if target.is_none()
                    && module.is_empty()
                    && root_item.as_ref().is_some_and(|name| {
                        self.files
                            .get(&origin)
                            .is_some_and(|f| f.root_items.contains(name))
                    })
                {
                    target = modules
                        .get(&(origin.clone(), Vec::new()))
                        .cloned()
                        .flatten();
                }
                if let Some(target) = target {
                    edges.insert((file.clone(), target));
                } else {
                    unresolved += 1;
                }
                if edges.len() >= MAX_IMPORTS {
                    partial = true;
                    break;
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
    use super::RustIndex;
    use crate::spider::connections::Direction;
    use std::path::Path;
    fn index(files: &[(&str, &str)]) -> RustIndex {
        let mut index = RustIndex::default();
        for (path, text) in files {
            index.update((*path).into(), text);
        }
        index
    }
    #[test]
    fn parses_real_modules_grouped_uses_and_ignores_comments_strings_and_macros() {
        let index = index(&[
            (
                "src/lib.rs",
                "mod a; mod b; // mod bogus;\nconst S: &str = \"mod fake;\";",
            ),
            (
                "src/a.rs",
                "use crate::b::{self, Thing as Alias}; /* use crate::fake; */",
            ),
            ("src/b/mod.rs", "pub struct Thing;"),
        ]);
        let graph = index.graph();
        let links = graph.relations(Path::new("src/a.rs"), false, 20);
        assert!(
            links
                .iter()
                .any(|r| r.path == Path::new("src/b/mod.rs") && r.direction == Direction::Uses)
        );
        assert_eq!(graph.edges, 3);
        assert_eq!(graph.unresolved, 0);
    }
    #[test]
    fn inline_modules_and_explicit_paths_resolve_without_escaping() {
        let index = index(&[
            (
                "src/lib.rs",
                "#[path = \"other.rs\"] mod alternate; mod nested { mod child; use crate::alternate::Item; } #[path=\"../../outside.rs\"] mod escape;",
            ),
            ("src/other.rs", "pub struct Item;"),
            ("src/nested/child.rs", "use super::super::alternate::Item;"),
        ]);
        let graph = index.graph();
        assert!(
            graph
                .relations(Path::new("src/nested/child.rs"), false, 20)
                .iter()
                .any(|r| r.path == Path::new("src/other.rs"))
        );
        assert_eq!(graph.unresolved, 1);
    }
    #[test]
    fn workspace_dependencies_and_renames_stay_inside_supplied_checkout() {
        let index = index(&[
            (
                "Cargo.toml",
                "[workspace]\nmembers=['app','lib']\n[workspace.dependencies]\nrenamed={path='lib'}\n",
            ),
            (
                "app/Cargo.toml",
                "[package]\nname='app'\nedition='2024'\n[dependencies]\nrenamed.workspace=true\nserde='1'\n",
            ),
            (
                "lib/Cargo.toml",
                "[package]\nname='original'\nedition='2024'\n",
            ),
            (
                "app/src/main.rs",
                "use renamed::model::Thing; use serde::Serialize;",
            ),
            ("lib/src/lib.rs", "pub mod model;"),
            ("lib/src/model.rs", "pub struct Thing;"),
        ]);
        let graph = index.graph();
        assert!(
            graph
                .relations(Path::new("app/src/main.rs"), false, 20)
                .iter()
                .any(|r| r.path == Path::new("lib/src/model.rs") && r.direction == Direction::Uses)
        );
        assert_eq!(graph.external, 1);
    }
    #[test]
    fn edits_remove_stale_edges_and_ambiguous_modules_are_not_guessed() {
        let mut index = index(&[
            ("src/lib.rs", "mod a; mod b;"),
            ("src/a.rs", "use crate::b::X;"),
            ("src/b.rs", "pub struct X;"),
        ]);
        assert!(
            index
                .graph()
                .relations(Path::new("src/a.rs"), false, 20)
                .iter()
                .any(|r| r.direction == Direction::Uses)
        );
        index.update("src/a.rs".into(), "// import removed");
        assert!(
            !index
                .graph()
                .relations(Path::new("src/a.rs"), false, 20)
                .iter()
                .any(|r| r.direction == Direction::Uses)
        );
        index.update("src/b/mod.rs".into(), "pub struct X;");
        let graph = index.graph();
        assert!(graph.unresolved > 0);
        assert!(
            !graph
                .relations(Path::new("src/lib.rs"), false, 20)
                .iter()
                .any(|r| r.path == Path::new("src/b.rs"))
        );
    }
    #[test]
    fn root_symbols_are_resolved_but_unknown_root_paths_are_not_guessed() {
        let index = index(&[
            ("src/lib.rs", "pub struct RootItem; mod a;"),
            (
                "src/a.rs",
                "use crate::RootItem; use crate::nonexistent::Fake;",
            ),
        ]);
        let graph = index.graph();
        assert_eq!(graph.unresolved, 1);
        assert!(
            graph
                .relations(Path::new("src/a.rs"), false, 20)
                .iter()
                .any(|r| r.path == Path::new("src/lib.rs") && r.direction == Direction::Uses)
        );
    }
    #[test]
    fn root_reexports_and_renamed_bindings_link_to_the_facade_file() {
        let index = index(&[
            (
                "src/lib.rs",
                "mod model; mod consumer; pub use model::Thing as PublicThing;",
            ),
            ("src/model.rs", "pub struct Thing;"),
            ("src/consumer.rs", "use crate::PublicThing;"),
        ]);
        let graph = index.graph();
        assert_eq!(graph.unresolved, 0);
        let links = graph.relations(Path::new("src/consumer.rs"), true, 20);
        assert!(links.iter().any(|r| r.path == Path::new("src/lib.rs")
            && r.direction == Direction::Uses
            && r.depth == 1));
        assert!(links.iter().any(|r| r.path == Path::new("src/model.rs")
            && r.direction == Direction::Uses
            && r.depth == 2));
    }
}
