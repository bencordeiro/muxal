//! Python module/package links over the supplied checkout. No interpreter,
//! sys.path execution, installed-package discovery, or semantic symbol lookup.
use super::Graph;
use super::syntax::{MAX_IMPORTS, MAX_TOTAL_IMPORTS, parse, text};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use tree_sitter::Node;

#[derive(Default)]
struct Facts {
    imports: Vec<Import>,
    bindings: HashSet<String>,
    partial: bool,
}
struct Import {
    level: usize,
    module: Vec<String>,
    names: Vec<String>,
    from: bool,
}
#[derive(Default)]
pub(super) struct PythonIndex {
    files: BTreeMap<PathBuf, Facts>,
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Module {
    base: PathBuf,
    file: Option<PathBuf>,
    package: bool,
}

fn module_name(node: Node, source: &str) -> Vec<String> {
    let node = if node.kind() == "aliased_import" {
        let Some(name) = node.child_by_field_name("name") else {
            return Vec::new();
        };
        name
    } else {
        node
    };
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|n| n.kind() == "identifier")
        .map(|n| text(n, source).to_string())
        .collect()
}
fn join_module(base: &Path, module: &[String]) -> PathBuf {
    let mut path = base.to_path_buf();
    for name in module {
        path.push(name);
    }
    path
}
fn source_path(base: &Path) -> PathBuf {
    let mut path = base.as_os_str().to_os_string();
    path.push(".py");
    path.into()
}
impl PythonIndex {
    pub(super) fn supported(path: &Path) -> bool {
        path.extension().is_some_and(|s| s == "py")
    }
    pub(super) fn remove(&mut self, path: &Path) {
        self.files.remove(path);
    }
    pub(super) fn update(&mut self, path: PathBuf, source: &str) {
        self.remove(&path);
        if !super::valid_relative(&path) || !Self::supported(&path) {
            return;
        }
        let mut facts = Facts::default();
        let partial = parse(tree_sitter_python::LANGUAGE.into(), source, |node| {
            // A package may export a value whose name also matches a file.
            // Do not invent a child-module edge for that ordinary case.
            let top = node.parent().is_some_and(|p| {
                p.kind() == "module"
                    || (matches!(p.kind(), "expression_statement" | "decorated_definition")
                        && p.parent().is_some_and(|p| p.kind() == "module"))
            });
            if top {
                let field = match node.kind() {
                    "function_definition" | "class_definition" => "name",
                    "assignment" => "left",
                    _ => "",
                };
                if let Some(name) = node.child_by_field_name(field)
                    && name.kind() == "identifier"
                {
                    facts.bindings.insert(text(name, source).to_string());
                }
                if matches!(node.kind(), "import_statement" | "import_from_statement") {
                    let mut cursor = node.walk();
                    for name in node.children_by_field_name("name", &mut cursor) {
                        let binding = name
                            .child_by_field_name("alias")
                            .map(|n| text(n, source).to_string())
                            .or_else(|| module_name(name, source).into_iter().next());
                        if let Some(binding) = binding {
                            facts.bindings.insert(binding);
                        }
                    }
                }
            }
            if !matches!(node.kind(), "import_statement" | "import_from_statement") {
                return;
            }
            if facts.imports.len() >= MAX_IMPORTS {
                facts.partial = true;
                return;
            }
            let mut cursor = node.walk();
            if node.kind() == "import_statement" {
                for name in node.children_by_field_name("name", &mut cursor) {
                    if facts.imports.len() >= MAX_IMPORTS {
                        facts.partial = true;
                        break;
                    }
                    facts.imports.push(Import {
                        level: 0,
                        module: module_name(name, source),
                        names: Vec::new(),
                        from: false,
                    });
                }
            } else if let Some(module) = node.child_by_field_name("module_name") {
                let (level, path) = if module.kind() == "relative_import" {
                    let mut cursor = module.walk();
                    let children: Vec<_> = module.named_children(&mut cursor).collect();
                    let level = children
                        .iter()
                        .filter(|n| n.kind() == "import_prefix")
                        .map(|n| text(*n, source).chars().filter(|c| *c == '.').count())
                        .sum();
                    let path = children
                        .into_iter()
                        .find(|n| n.kind() == "dotted_name")
                        .map(|n| module_name(n, source))
                        .unwrap_or_default();
                    (level, path)
                } else {
                    (0, module_name(module, source))
                };
                let mut names: Vec<_> = node
                    .children_by_field_name("name", &mut cursor)
                    .flat_map(|n| module_name(n, source))
                    .take(MAX_IMPORTS + 1)
                    .collect();
                if names.len() > MAX_IMPORTS {
                    names.truncate(MAX_IMPORTS);
                    facts.partial = true;
                }
                facts.imports.push(Import {
                    level,
                    module: path,
                    names,
                    from: true,
                });
            }
        });
        facts.partial |= partial;
        self.files.insert(path, facts);
    }
    fn package_root(&self, file: &Path) -> PathBuf {
        let mut at = file.parent().unwrap_or(Path::new(""));
        let mut package = false;
        while !at.as_os_str().is_empty() && self.files.contains_key(&at.join("__init__.py")) {
            package = true;
            at = at.parent().unwrap_or(Path::new(""));
        }
        if package {
            at.to_path_buf()
        } else if file.starts_with("src") {
            "src".into()
        } else {
            PathBuf::new()
        }
    }
    fn roots(&self, file: &Path) -> BTreeSet<PathBuf> {
        let mut roots = BTreeSet::from([
            PathBuf::new(),
            PathBuf::from("src"),
            self.package_root(file),
        ]);
        // Loose scripts may be run by path; their directory enters sys.path.
        // If that and a checkout root both match, resolve neither by guessing.
        if let Some(parent) = file.parent()
            && !self.files.contains_key(&parent.join("__init__.py"))
        {
            roots.insert(parent.to_path_buf());
        }
        roots
    }
    fn module_at(&self, base: PathBuf, directories: &HashSet<PathBuf>) -> Vec<Module> {
        let file = source_path(&base);
        let init = base.join("__init__.py");
        let mut modules = Vec::new();
        if self.files.contains_key(&file) {
            modules.push(Module {
                base: base.clone(),
                file: Some(file),
                package: false,
            });
        }
        if directories.contains(&base) {
            modules.push(Module {
                base,
                file: self.files.contains_key(&init).then_some(init),
                package: true,
            });
        }
        modules
    }
    pub(super) fn graph(&self) -> Graph {
        let mut directories = HashSet::new();
        for file in self.files.keys() {
            let mut parent = file.parent();
            while let Some(path) = parent {
                directories.insert(path.to_path_buf());
                parent = path.parent();
            }
        }
        let mut edges = BTreeSet::new();
        let mut unresolved = 0;
        let mut external = 0;
        let mut partial = self.files.values().any(|facts| facts.partial);
        let mut count = 0;
        'files: for (file, facts) in &self.files {
            let roots = self.roots(file);
            for import in &facts.imports {
                count += 1 + import.names.len();
                if count > MAX_TOTAL_IMPORTS {
                    partial = true;
                    break 'files;
                }
                let bases: Vec<_> = if import.level > 0 {
                    let anchor = self.package_root(file);
                    let mut base = file.parent().unwrap_or(Path::new("")).to_path_buf();
                    let depth = base
                        .strip_prefix(&anchor)
                        .map(|p| p.components().count())
                        .unwrap_or(0);
                    if import.level > depth {
                        unresolved += 1;
                        continue;
                    }
                    for _ in 1..import.level {
                        base.pop();
                    }
                    vec![join_module(&base, &import.module)]
                } else {
                    roots
                        .iter()
                        .map(|base| join_module(base, &import.module))
                        .collect()
                };
                let modules: BTreeSet<_> = bases
                    .into_iter()
                    .flat_map(|base| self.module_at(base, &directories))
                    .collect();
                if modules.len() != 1 {
                    let local = import.level > 0
                        || import.module.first().is_some_and(|name| {
                            roots.iter().any(|r| {
                                self.files.contains_key(&source_path(&r.join(name)))
                                    || directories.contains(&r.join(name))
                            })
                        });
                    if !modules.is_empty() || local {
                        unresolved += 1;
                    } else {
                        external += 1;
                    }
                    continue;
                }
                let module = modules.into_iter().next().unwrap();
                let mut resolved = module.file.is_some();
                let package_facts = module.file.as_ref().and_then(|path| self.files.get(path));
                if let Some(target) = &module.file {
                    edges.insert((file.clone(), target.clone()));
                }
                if import.from && module.package {
                    for name in &import.names {
                        if package_facts.is_some_and(|facts| facts.bindings.contains(name)) {
                            continue;
                        }
                        let children = self.module_at(module.base.join(name), &directories);
                        if children.len() == 1 {
                            if let Some(target) = &children[0].file {
                                edges.insert((file.clone(), target.clone()));
                                resolved = true;
                            }
                        } else if children.len() > 1 {
                            unresolved += 1;
                        }
                    }
                }
                if !resolved {
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
    use super::PythonIndex;
    use crate::spider::connections::Direction;
    use std::path::Path;
    fn targets(index: &PythonIndex, file: &str) -> Vec<String> {
        index
            .graph()
            .relations(Path::new(file), false, 100)
            .into_iter()
            .filter(|r| r.direction == Direction::Uses)
            .map(|r| r.path.to_string_lossy().into())
            .collect()
    }
    #[test]
    fn absolute_relative_multiline_and_aliased_imports_resolve_local_packages() {
        let mut index = PythonIndex::default();
        for (path, source) in [
            (
                "main.py",
                "import pkg.tools as tools, os\nfrom pkg.tools import run\nfrom pkg import (tools as t, VALUE)\n",
            ),
            ("pkg/__init__.py", "VALUE = 1\n"),
            (
                "pkg/tools.py",
                "from . import helper\nfrom .helper import run\n",
            ),
            ("pkg/helper.py", "def run(): pass\n"),
            ("pkg/nested/__init__.py", ""),
            ("pkg/nested/work.py", "from ..helper import run\n"),
        ] {
            index.update(path.into(), source);
        }
        assert_eq!(
            targets(&index, "main.py"),
            ["pkg/__init__.py", "pkg/tools.py"]
        );
        assert_eq!(
            targets(&index, "pkg/tools.py"),
            ["pkg/__init__.py", "pkg/helper.py"]
        );
        assert_eq!(targets(&index, "pkg/nested/work.py"), ["pkg/helper.py"]);
        assert_eq!(index.graph().external, 1);
        assert!(!index.graph().partial);
    }
    #[test]
    fn src_layout_scripts_and_namespace_children_work_without_execution() {
        let mut index = PythonIndex::default();
        for (path, source) in [
            (
                "main.py",
                "from app import tools\nfrom namespace import helper\n",
            ),
            ("src/app/__init__.py", ""),
            ("src/app/tools.py", "from .model import Model\n"),
            ("src/app/model.py", "class Model: pass\n"),
            ("namespace/helper.py", ""),
            ("scripts/run.py", "import util\n"),
            ("scripts/util.py", ""),
        ] {
            index.update(path.into(), source);
        }
        assert_eq!(
            targets(&index, "main.py"),
            [
                "namespace/helper.py",
                "src/app/__init__.py",
                "src/app/tools.py"
            ]
        );
        assert_eq!(targets(&index, "src/app/tools.py"), ["src/app/model.py"]);
        assert_eq!(targets(&index, "scripts/run.py"), ["scripts/util.py"]);
    }
    #[test]
    fn ambiguous_roots_missing_locals_and_escaping_relative_imports_stay_unresolved() {
        let mut index = PythonIndex::default();
        for (path, source) in [
            ("main.py", "import util\nfrom pkg.missing import Thing\n"),
            ("util.py", ""),
            ("src/util.py", ""),
            ("pkg/__init__.py", ""),
            (
                "pkg/work.py",
                "from ..util import x\nfrom .missing import y\n",
            ),
        ] {
            index.update(path.into(), source);
        }
        assert!(targets(&index, "main.py").is_empty());
        assert!(targets(&index, "pkg/work.py").is_empty());
        assert_eq!(index.graph().unresolved, 4);
        index.remove(Path::new("src/util.py"));
        assert_eq!(targets(&index, "main.py"), ["util.py"]);
    }
    #[test]
    fn comments_docstrings_dynamic_imports_and_source_edits_do_not_leave_fake_edges() {
        let mut index = PythonIndex::default();
        index.update("main.py".into(), "# import fake\ntext = '''from fake import X'''\n__import__('fake')\nfrom real import Thing\n");
        index.update("fake.py".into(), "");
        index.update("real.py".into(), "class Thing: pass\n");
        assert_eq!(targets(&index, "main.py"), ["real.py"]);
        index.update("main.py".into(), "pass\n");
        assert!(targets(&index, "main.py").is_empty());
        index.update("main.py".into(), "import real\n");
        index.remove(Path::new("real.py"));
        assert!(targets(&index, "main.py").is_empty());
    }
    #[test]
    fn package_values_and_reexports_are_not_mistaken_for_neighboring_files() {
        let mut index = PythonIndex::default();
        for (file, source) in [
            ("main.py", "from pkg import tools, service\n"),
            (
                "pkg/__init__.py",
                "tools = 3\nfrom .real import Service as service\n",
            ),
            ("pkg/tools.py", ""),
            ("pkg/service.py", ""),
            ("pkg/real.py", "class Service: pass\n"),
        ] {
            index.update(file.into(), source);
        }
        assert_eq!(targets(&index, "main.py"), ["pkg/__init__.py"]);
        assert_eq!(targets(&index, "pkg/__init__.py"), ["pkg/real.py"]);
        assert!(
            index
                .graph()
                .relations(Path::new("main.py"), true, 100)
                .iter()
                .any(|r| r.path == Path::new("pkg/real.py") && r.depth == 2)
        );
    }
}
