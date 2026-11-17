//! Resolve syntax-level imports against files in one repository snapshot.
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use tree_sitter::{Node, Parser};

pub const MAX_LINKS: usize = 64;
const MAX_TARGET_FILES: usize = 24;
const CACHE_ENTRIES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Destination {
    pub path: String,
    pub line: usize,
    pub exact: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub line: usize,
    pub import: String,
    pub symbol: Option<String>,
    pub destination: Option<Destination>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct FileLinks {
    pub path: String,
    pub links: Vec<Link>,
    pub supported: bool,
    pub truncated: bool,
    pub partial: bool,
}

#[derive(Clone, Default)]
struct Parsed {
    imports: Vec<Import>,
    definitions: HashMap<String, Vec<usize>>,
    partial: bool,
}

#[derive(Clone)]
struct Import {
    line: usize,
    path: String,
    symbol: Option<String>,
    module: bool,
}

#[derive(Default)]
pub struct Index {
    parsed: HashMap<(String, [u8; 32]), Parsed>,
}

impl Index {
    /// Only the focused file and at most 24 import targets are loaded. Call off the render thread.
    pub fn inspect(
        &mut self,
        repo: &crate::Repository,
        index: usize,
        before: bool,
    ) -> Result<FileLinks, String> {
        let path = &repo.files.get(index).ok_or("Unknown file")?.path;
        let paths = repo
            .files
            .iter()
            .enumerate()
            .filter(|(_, file)| {
                if before {
                    file.before.is_some()
                } else {
                    file.after.is_some()
                }
            })
            .map(|(index, file)| (file.path.clone(), index))
            .collect::<HashMap<_, _>>();
        self.resolve(path, &paths.keys().cloned().collect(), |path| {
            repo.source(paths[path], before)
        })
    }

    #[cfg(test)]
    fn inspect_with(&mut self, path: &str, sources: &HashMap<String, String>) -> Vec<Link> {
        self.resolve(path, &sources.keys().cloned().collect(), |path| {
            Ok(sources[path].clone())
        })
        .unwrap()
        .links
    }

    fn parse(&mut self, path: &str, source: &str) -> Parsed {
        let language = extension(path).to_owned();
        let key = (language, Sha256::digest(source.as_bytes()).into());
        if let Some(parsed) = self.parsed.get(&key) {
            return parsed.clone();
        }
        let parsed = parse(path, source);
        if self.parsed.len() >= CACHE_ENTRIES {
            self.parsed.clear();
        }
        self.parsed.insert(key, parsed.clone());
        parsed
    }

    fn resolve(
        &mut self,
        path: &str,
        files: &HashSet<String>,
        mut load: impl FnMut(&str) -> Result<String, String>,
    ) -> Result<FileLinks, String> {
        let supported = matches!(
            extension(path),
            "rs" | "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx"
        );
        let mut result = FileLinks {
            path: path.into(),
            supported,
            ..Default::default()
        };
        if !supported {
            return Ok(result);
        }
        let source = load(path)?;
        let parsed = self.parse(path, &source);
        result.partial = parsed.partial;
        result.truncated = parsed.imports.len() > MAX_LINKS;
        let mut targets = HashMap::<String, Result<Parsed, String>>::new();
        for import in parsed.imports.into_iter().take(MAX_LINKS) {
            let resolution = if extension(path) == "rs" {
                rust_target(path, &import, files)
            } else {
                javascript_target(path, &import.path, files)
                    .map(|target| (target, import.symbol.clone()))
            };
            let mut link = Link {
                line: import.line,
                import: import.path.clone(),
                symbol: import.symbol.clone(),
                destination: None,
                reason: None,
            };
            match resolution {
                Err(error) => link.reason = Some(error),
                Ok((target, symbol)) => {
                    link.symbol = symbol.clone();
                    if !targets.contains_key(&target) {
                        if targets.len() >= MAX_TARGET_FILES {
                            result.truncated = true;
                            link.reason = Some(
                                "Target limit reached; open this file to inspect its links.".into(),
                            );
                            result.links.push(link);
                            continue;
                        }
                        let parsed = load(&target).map(|source| self.parse(&target, &source));
                        targets.insert(target.clone(), parsed);
                    }
                    match &targets[&target] {
                        Err(error) => link.reason = Some(error.clone()),
                        Ok(parsed) => {
                            let line = symbol
                                .as_ref()
                                .and_then(|name| parsed.definitions.get(name))
                                .filter(|lines| lines.len() == 1)
                                .map(|lines| lines[0]);
                            if symbol.is_some() && line.is_none() {
                                link.reason = Some(
                                    "File resolved; symbol declaration is not uniquely resolved."
                                        .into(),
                                );
                            }
                            link.destination = Some(Destination {
                                path: target,
                                line: line.unwrap_or(1),
                                exact: line.is_some(),
                            });
                        }
                    }
                }
            }
            result.links.push(link);
        }
        Ok(result)
    }
}

fn extension(path: &str) -> &str {
    path.rsplit_once('.').map_or("", |(_, suffix)| suffix)
}
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}
fn join(base: &str, part: &str) -> String {
    if base.is_empty() {
        part.into()
    } else {
        format!("{base}/{part}")
    }
}

fn relative(base: &str, path: &str) -> Option<String> {
    let joined = join(base, path);
    let mut parts = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

fn unique(
    candidates: impl IntoIterator<Item = String>,
    files: &HashSet<String>,
) -> Result<String, String> {
    let candidates = candidates
        .into_iter()
        .filter(|path| files.contains(path))
        .collect::<HashSet<_>>();
    match candidates.len() {
        1 => Ok(candidates.into_iter().next().unwrap()),
        0 => Err("No local file resolved in this snapshot.".into()),
        _ => Err("Several files match; resolution is ambiguous.".into()),
    }
}

fn javascript_target(path: &str, import: &str, files: &HashSet<String>) -> Result<String, String> {
    if !import.starts_with("./") && !import.starts_with("../") {
        return Err("External package or alias; local resolution is unavailable.".into());
    }
    let base = relative(parent(path), import).ok_or("Import escapes the repository.")?;
    if files.contains(&base) {
        return Ok(base);
    }
    let mut candidates = ["js", "mjs", "cjs", "jsx", "ts", "tsx"]
        .into_iter()
        .flat_map(|extension| {
            [
                format!("{base}.{extension}"),
                format!("{base}/index.{extension}"),
            ]
        })
        .collect::<Vec<_>>();
    if matches!(extension(path), "ts" | "tsx")
        && matches!(extension(&base), "js" | "jsx" | "mjs" | "cjs")
    {
        let stem = base.rsplit_once('.').unwrap().0;
        candidates.extend([format!("{stem}.ts"), format!("{stem}.tsx")]);
    }
    unique(candidates, files)
}

fn module_base(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    if matches!(name, "main.rs" | "lib.rs" | "mod.rs") {
        parent(path).into()
    } else {
        path.trim_end_matches(".rs").into()
    }
}

fn crate_root(path: &str, files: &HashSet<String>) -> String {
    let mut directory = parent(path);
    loop {
        if files.contains(&join(directory, "lib.rs")) || files.contains(&join(directory, "main.rs"))
        {
            return directory.into();
        }
        if directory.is_empty() {
            return String::new();
        }
        directory = parent(directory);
    }
}

fn rust_target(
    path: &str,
    import: &Import,
    files: &HashSet<String>,
) -> Result<(String, Option<String>), String> {
    if import.path.starts_with("#[path]") {
        return Err("Custom module path is not resolved.".into());
    }
    let mut parts = import.path.split("::").collect::<Vec<_>>();
    let mut base = if import.module {
        module_base(path)
    } else {
        match parts.first().copied() {
            Some("crate") => {
                parts.remove(0);
                crate_root(path, files)
            }
            Some("self") => {
                parts.remove(0);
                module_base(path)
            }
            Some("super") => {
                let mut base = module_base(path);
                while parts.first() == Some(&"super") {
                    parts.remove(0);
                    base = parent(&base).into();
                }
                base
            }
            _ => {
                return Err(
                    "External crate or unqualified import; local resolution is unavailable.".into(),
                );
            }
        }
    };
    if import.module {
        base = join(&base, &parts.join("/"));
        return unique([format!("{base}.rs"), format!("{base}/mod.rs")], files)
            .map(|path| (path, None));
    }
    for split in (0..=parts.len()).rev() {
        let module = join(&base, &parts[..split].join("/"));
        let candidates = if split == 0 {
            vec![
                join(&module, "lib.rs"),
                join(&module, "main.rs"),
                join(&module, "mod.rs"),
                format!("{module}.rs"),
            ]
        } else {
            vec![format!("{module}.rs"), format!("{module}/mod.rs")]
        };
        match unique(candidates, files) {
            Ok(path) => {
                return Ok((
                    path,
                    (parts.len() == split + 1 && parts[split] != "*").then(|| parts[split].into()),
                ));
            }
            Err(error) if error.contains("ambiguous") => return Err(error),
            Err(_) => {}
        }
    }
    Err("No local Rust module resolved in this snapshot.".into())
}

fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("")
}
fn named<'a>(node: Node<'a>) -> Vec<Node<'a>> {
    node.named_children(&mut node.walk()).collect()
}
fn descendants<'a>(node: Node<'a>, kind: &str, result: &mut Vec<Node<'a>>) {
    if node.kind() == kind {
        result.push(node);
        return;
    }
    for child in named(node) {
        descendants(child, kind, result);
    }
}

fn parse(path: &str, source: &str) -> Parsed {
    let language = match extension(path) {
        "rs" => tree_sitter_rust::LANGUAGE.into(),
        "ts" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        _ => tree_sitter_javascript::LANGUAGE.into(),
    };
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return Parsed::default();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Parsed::default();
    };
    let mut parsed = Parsed {
        partial: tree.root_node().has_error(),
        ..Default::default()
    };
    for item in named(tree.root_node()) {
        if item.has_error() || item.is_missing() {
            continue;
        }
        if extension(path) == "rs" {
            rust_item(item, source, &mut parsed);
        } else {
            javascript_item(item, source, &mut parsed);
        }
    }
    parsed
}

fn declaration(node: Node<'_>, source: &str, parsed: &mut Parsed) {
    let kind = node.kind();
    if matches!(kind, "lexical_declaration" | "variable_declaration") {
        for declarator in named(node)
            .into_iter()
            .filter(|child| child.kind() == "variable_declarator")
        {
            if let Some(name) = declarator
                .child_by_field_name("name")
                .filter(|name| name.kind() == "identifier")
            {
                parsed
                    .definitions
                    .entry(text(name, source).into())
                    .or_default()
                    .push(node.start_position().row + 1);
            }
        }
    } else if matches!(
        kind,
        "function_declaration"
            | "generator_function_declaration"
            | "class_declaration"
            | "function_item"
            | "struct_item"
            | "enum_item"
            | "type_item"
            | "const_item"
            | "static_item"
            | "trait_item"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
    ) {
        if let Some(name) = node.child_by_field_name("name") {
            parsed
                .definitions
                .entry(text(name, source).into())
                .or_default()
                .push(node.start_position().row + 1);
        }
    }
}

fn javascript_item(node: Node<'_>, source: &str, parsed: &mut Parsed) {
    if matches!(node.kind(), "import_statement" | "export_statement")
        && let Some(value) = node.child_by_field_name("source")
    {
        let literal = text(value, source);
        let import = literal
            .get(1..literal.len().saturating_sub(1))
            .unwrap_or("");
        if import.contains('\\') {
            return;
        }
        let mut specifiers = Vec::new();
        descendants(
            node,
            if node.kind() == "import_statement" {
                "import_specifier"
            } else {
                "export_specifier"
            },
            &mut specifiers,
        );
        for specifier in &specifiers {
            let symbol = specifier
                .child_by_field_name("name")
                .map(|name| text(name, source).into());
            parsed.imports.push(Import {
                line: node.start_position().row + 1,
                path: import.into(),
                symbol,
                module: false,
            });
        }
        let default_import = named(node)
            .into_iter()
            .find(|child| child.kind() == "import_clause")
            .is_some_and(|clause| {
                named(clause)
                    .iter()
                    .any(|child| child.kind() == "identifier")
            });
        if default_import || specifiers.is_empty() {
            parsed.imports.push(Import {
                line: node.start_position().row + 1,
                path: import.into(),
                symbol: default_import.then(|| "default".into()),
                module: false,
            });
        }
    }
    if node.kind() == "export_statement" {
        for child in named(node) {
            declaration(child, source, parsed);
        }
        if text(node, source)
            .trim_start()
            .starts_with("export default ")
        {
            parsed
                .definitions
                .entry("default".into())
                .or_default()
                .push(node.start_position().row + 1);
        }
    }
}

fn rust_item(node: Node<'_>, source: &str, parsed: &mut Parsed) {
    declaration(node, source, parsed);
    if node.kind() == "mod_item"
        && node.child_by_field_name("body").is_none()
        && let Some(name) = node.child_by_field_name("name")
    {
        let custom = node.prev_named_sibling().is_some_and(|previous| {
            previous.kind() == "attribute_item" && text(previous, source).contains("path")
        });
        parsed.imports.push(Import {
            line: node.start_position().row + 1,
            path: format!(
                "{}{}",
                if custom { "#[path] " } else { "" },
                text(name, source)
            ),
            symbol: None,
            module: true,
        });
    }
    if node.kind() == "use_declaration"
        && let Some(argument) = node.child_by_field_name("argument")
    {
        let mut paths = Vec::new();
        rust_use(argument, "", source, &mut paths);
        for path in paths {
            parsed.imports.push(Import {
                line: node.start_position().row + 1,
                path,
                symbol: None,
                module: false,
            });
        }
    }
}

fn rust_use(node: Node<'_>, prefix: &str, source: &str, paths: &mut Vec<String>) {
    match node.kind() {
        "use_list" => {
            for child in named(node) {
                rust_use(child, prefix, source, paths);
            }
        }
        "scoped_use_list" => {
            let prefix = node
                .child_by_field_name("path")
                .map_or(prefix.into(), |path| {
                    format!("{prefix}{}::", text(path, source))
                });
            if let Some(list) = node.child_by_field_name("list") {
                rust_use(list, &prefix, source, paths);
            }
        }
        "use_as_clause" => {
            if let Some(path) = node.child_by_field_name("path") {
                rust_use(path, prefix, source, paths);
            }
        }
        _ => paths.push(format!("{prefix}{}", text(node, source))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(files: &[(&str, &str)]) -> HashMap<String, String> {
        files
            .iter()
            .map(|(path, source)| ((*path).into(), (*source).into()))
            .collect()
    }

    #[test]
    fn named_import_road_ends_at_the_exported_definition() {
        // Given a local import, when inspected, its destination is the definition's source line.
        let sources = fixture(&[
            (
                "web/app.mjs",
                "import { greet as hello } from './greet.mjs';\nhello();\n",
            ),
            (
                "web/greet.mjs",
                "// greeting\n\nexport function greet() {}\n",
            ),
        ]);
        let links = Index::default().inspect_with("web/app.mjs", &sources);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].line, 1);
        assert_eq!(links[0].symbol.as_deref(), Some("greet"));
        assert_eq!(
            links[0].destination,
            Some(Destination {
                path: "web/greet.mjs".into(),
                line: 3,
                exact: true
            })
        );
    }

    #[test]
    fn unsupported_external_and_ambiguous_imports_never_become_roads() {
        let sources = fixture(&[
            (
                "app.ts",
                "import React from 'react';\nimport './missing';\nimport './shared';\n",
            ),
            ("shared.ts", "export const value = 1;"),
            ("shared.js", "export const value = 2;"),
        ]);
        let links = Index::default().inspect_with("app.ts", &sources);
        assert_eq!(links.len(), 3);
        assert!(
            links
                .iter()
                .all(|link| link.destination.is_none() && link.reason.is_some())
        );
    }

    #[test]
    fn rust_module_and_crate_use_reach_local_files_and_definition_lines() {
        let sources = fixture(&[
            ("src/main.rs", "mod service;\nuse crate::service::run;\n"),
            ("src/service.rs", "// service\npub fn run() {}\n"),
        ]);
        let links = Index::default().inspect_with("src/main.rs", &sources);
        assert_eq!(links.len(), 2);
        assert_eq!(
            links[0].destination.as_ref().unwrap().path,
            "src/service.rs"
        );
        assert_eq!(
            links[1].destination,
            Some(Destination {
                path: "src/service.rs".into(),
                line: 2,
                exact: true
            })
        );
    }

    #[test]
    fn changing_target_contents_invalidates_its_cached_line() {
        let mut sources = fixture(&[
            ("app.mjs", "import { value } from './value.mjs';"),
            ("value.mjs", "export const value = 1;"),
        ]);
        let mut index = Index::default();
        assert_eq!(
            index.inspect_with("app.mjs", &sources)[0]
                .destination
                .as_ref()
                .unwrap()
                .line,
            1
        );
        sources.insert(
            "value.mjs".into(),
            "// moved\n\nexport const value = 2;".into(),
        );
        assert_eq!(
            index.inspect_with("app.mjs", &sources)[0]
                .destination
                .as_ref()
                .unwrap()
                .line,
            3
        );
    }

    #[test]
    fn grouped_rust_imports_and_aliases_keep_original_symbol_names() {
        let sources = fixture(&[
            (
                "src/main.rs",
                "use crate::service::{run as execute, State};",
            ),
            ("src/service.rs", "pub struct State;\n\npub fn run() {}"),
        ]);
        let links = Index::default().inspect_with("src/main.rs", &sources);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].destination.as_ref().unwrap().line, 3);
        assert_eq!(links[1].destination.as_ref().unwrap().line, 1);
        assert!(
            links
                .iter()
                .all(|link| link.destination.as_ref().unwrap().exact)
        );
    }

    #[test]
    fn syntax_avoids_imports_inside_comments_and_string_literals() {
        let sources = fixture(&[
            (
                "app.mjs",
                "// import './a.mjs';\nconst text = \"import './a.mjs';\";",
            ),
            ("a.mjs", "export default 1;"),
        ]);
        assert!(
            Index::default()
                .inspect_with("app.mjs", &sources)
                .is_empty()
        );
    }

    #[test]
    fn typescript_runtime_suffix_and_default_export_reach_real_source() {
        let sources = fixture(&[
            ("src/app.ts", "import Widget from './widget.js';"),
            (
                "src/widget.ts",
                "// component\nexport default class Widget {}",
            ),
        ]);
        let links = Index::default().inspect_with("src/app.ts", &sources);
        assert_eq!(
            links[0].destination,
            Some(Destination {
                path: "src/widget.ts".into(),
                line: 2,
                exact: true
            })
        );
    }

    #[test]
    fn missing_symbol_opens_the_module_without_claiming_an_exact_definition() {
        let sources = fixture(&[
            ("app.mjs", "import { elsewhere } from './bridge.mjs';"),
            ("bridge.mjs", "export { elsewhere } from './external.mjs';"),
        ]);
        let links = Index::default().inspect_with("app.mjs", &sources);
        assert_eq!(links[0].destination.as_ref().unwrap().line, 1);
        assert!(!links[0].destination.as_ref().unwrap().exact);
        assert!(links[0].reason.is_some());
    }

    #[test]
    fn import_expansion_has_a_fixed_target_file_read_budget() {
        let mut sources = HashMap::new();
        let mut source = String::new();
        for index in 0..40 {
            source.push_str(&format!("import './file{index}.mjs';\n"));
            sources.insert(
                format!("file{index}.mjs"),
                "export const value = 1;".to_owned(),
            );
        }
        sources.insert("app.mjs".into(), source);
        let mut loaded = Vec::new();
        let result = Index::default()
            .resolve("app.mjs", &sources.keys().cloned().collect(), |path| {
                loaded.push(path.to_owned());
                Ok(sources[path].clone())
            })
            .unwrap();
        assert_eq!(loaded.len(), 1 + MAX_TARGET_FILES);
        assert_eq!(
            result
                .links
                .iter()
                .filter(|link| link.destination.is_some())
                .count(),
            MAX_TARGET_FILES
        );
        assert!(result.truncated);
    }

    #[test]
    fn malformed_source_reports_partial_extraction_and_keeps_valid_imports() {
        let sources = fixture(&[
            ("app.mjs", "import './a.mjs';\nconst broken = ;"),
            ("a.mjs", "export default 1;"),
        ]);
        let result = Index::default()
            .resolve("app.mjs", &sources.keys().cloned().collect(), |path| {
                Ok(sources[path].clone())
            })
            .unwrap();
        assert!(result.partial);
        assert_eq!(result.links.len(), 1);
        assert!(result.links[0].destination.is_some());
    }
}
