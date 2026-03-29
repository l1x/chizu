use chizu_core::{
    Edge, EdgeKind, Entity, EntityKind, Visibility, entity_id, source_unit_id, symbol_id, test_id,
};
use tree_sitter::Parser;

use crate::error::{IndexError, Result};
use crate::walk::WalkedFile;

/// Index a Rust source file using tree-sitter.
pub fn index_rust_file(
    file: &WalkedFile,
    repo_root: &std::path::Path,
) -> Result<(Vec<Entity>, Vec<Edge>)> {
    let mut entities = Vec::new();
    let mut edges = Vec::new();
    let path_str = file.path.to_string_lossy();

    let abs_path = repo_root.join(&file.path);
    let source = std::fs::read_to_string(&abs_path)?;

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .map_err(|e| IndexError::Other(format!("tree-sitter language error: {:?}", e)))?;
    let tree = parser
        .parse(&source, None)
        .ok_or_else(|| IndexError::Other("failed to parse rust file".into()))?;

    let su_id = source_unit_id(&path_str);
    entities.push(
        Entity::new(&su_id, EntityKind::SourceUnit, path_str.as_ref())
            .with_path(path_str.as_ref())
            .with_language("rust")
            .with_exported(true),
    );

    let test_names = collect_attributed_fns(tree.root_node(), &source, "test");
    let bench_names = collect_attributed_fns(tree.root_node(), &source, "bench");

    extract_items(
        tree.root_node(),
        &source,
        &path_str,
        &su_id,
        &test_names,
        &bench_names,
        &mut entities,
        &mut edges,
    );

    Ok((entities, edges))
}

/// Collect function names that have a specific attribute (#[test], #[bench]).
fn collect_attributed_fns(
    node: tree_sitter::Node,
    source: &str,
    attr_name: &str,
) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == "attribute_item" && has_attribute(child, source, attr_name) {
            let mut sibling_cursor = node.walk();
            let children: Vec<_> = node.children(&mut sibling_cursor).collect();
            if let Some(idx) = children.iter().position(|c| *c == child) {
                for next in &children[idx + 1..] {
                    if next.kind() == "function_item" {
                        if let Some(name_node) = next.child_by_field_name("name") {
                            names.insert(source[name_node.byte_range()].to_string());
                        }
                        break;
                    }
                    if !next.kind().starts_with("attribute")
                        && !next.kind().starts_with("line_comment")
                    {
                        break;
                    }
                }
            }
        }
        names.extend(collect_attributed_fns(child, source, attr_name));
    }

    names
}

fn has_attribute(node: tree_sitter::Node, source: &str, attr_name: &str) -> bool {
    if node.kind() != "attribute_item" {
        return false;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "attribute" {
            let mut inner = child.walk();
            for c in child.children(&mut inner) {
                if c.kind() == "identifier" && &source[c.byte_range()] == attr_name {
                    return true;
                }
            }
        }
    }
    false
}

fn extract_visibility(node: tree_sitter::Node, source: &str) -> Option<Visibility> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            let text = &source[child.byte_range()];
            return Some(match text {
                "pub" => Visibility::Public,
                s if s.starts_with("pub(crate)") => Visibility::Internal,
                s if s.starts_with("pub(super)") => Visibility::Internal,
                _ => Visibility::Public,
            });
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn extract_items(
    node: tree_sitter::Node,
    source: &str,
    path_str: &str,
    source_unit_id: &str,
    test_names: &std::collections::HashSet<String>,
    bench_names: &std::collections::HashSet<String>,
    entities: &mut Vec<Entity>,
    edges: &mut Vec<Edge>,
) {
    match node.kind() {
        "function_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = source[name_node.byte_range()].to_string();
                let vis = extract_visibility(node, source);
                let exported = vis == Some(Visibility::Public);

                if test_names.contains(&name) {
                    let id = test_id(path_str, &name);
                    let mut e = Entity::new(&id, EntityKind::Test, &name)
                        .with_path(path_str)
                        .with_language("rust");
                    if let Some(v) = vis {
                        e = e.with_visibility(v);
                    }
                    entities.push(e);
                    edges.push(Edge::new(source_unit_id, EdgeKind::TestedBy, &id));
                } else if bench_names.contains(&name) {
                    let id = entity_id("bench", &format!("{path_str}::{name}"));
                    let mut e = Entity::new(&id, EntityKind::Bench, &name)
                        .with_path(path_str)
                        .with_language("rust");
                    if let Some(v) = vis {
                        e = e.with_visibility(v);
                    }
                    entities.push(e);
                    edges.push(Edge::new(source_unit_id, EdgeKind::BenchmarkedBy, &id));
                } else {
                    let id = symbol_id(path_str, &name);
                    let mut e = Entity::new(&id, EntityKind::Symbol, &name)
                        .with_path(path_str)
                        .with_language("rust")
                        .with_exported(exported);
                    if let Some(v) = vis {
                        e = e.with_visibility(v);
                    }
                    entities.push(e);
                    edges.push(Edge::new(source_unit_id, EdgeKind::Defines, &id));
                }
            }
        }
        "struct_item" | "enum_item" | "trait_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = source[name_node.byte_range()].to_string();
                let vis = extract_visibility(node, source);
                let exported = vis == Some(Visibility::Public);
                let id = symbol_id(path_str, &name);
                let mut e = Entity::new(&id, EntityKind::Symbol, &name)
                    .with_path(path_str)
                    .with_language("rust")
                    .with_exported(exported);
                if let Some(v) = vis {
                    e = e.with_visibility(v);
                }
                entities.push(e);
                edges.push(Edge::new(source_unit_id, EdgeKind::Defines, &id));
            }
        }
        "impl_item" => {
            // `impl Trait for Type` → Implements edge from type to trait
            extract_impl(node, source, path_str, edges);
            // Recurse into impl body for methods
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "declaration_list" {
                    extract_items(
                        child,
                        source,
                        path_str,
                        source_unit_id,
                        test_names,
                        bench_names,
                        entities,
                        edges,
                    );
                }
            }
        }
        "use_declaration" => {
            // `pub use foo::Bar` → Reexports edge
            let vis = extract_visibility(node, source);
            if vis == Some(Visibility::Public) {
                let text = &source[node.byte_range()];
                // Extract the last path segment as the reexported name
                if let Some(last_segment) = extract_use_target(text) {
                    let reexport_id = symbol_id(path_str, last_segment);
                    edges.push(Edge::new(source_unit_id, EdgeKind::Reexports, &reexport_id));
                }
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_items(
                    child,
                    source,
                    path_str,
                    source_unit_id,
                    test_names,
                    bench_names,
                    entities,
                    edges,
                );
            }
        }
    }
}

fn extract_impl(
    node: tree_sitter::Node,
    source: &str,
    path_str: &str,
    edges: &mut Vec<Edge>,
) {
    // Look for `impl Trait for Type` pattern
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();

    // Find the "for" keyword — its presence distinguishes `impl Trait for Type`
    // from `impl Type`.
    let has_for = children.iter().any(|c| {
        c.kind() == "for" || (c.kind() == "identifier" && &source[c.byte_range()] == "for")
    });
    if !has_for {
        return;
    }

    let mut trait_name = None;
    let mut type_name = None;
    let mut saw_for = false;

    for child in &children {
        let kind = child.kind();
        if kind == "for" || (&source[child.byte_range()] == "for" && kind == "identifier") {
            saw_for = true;
            continue;
        }
        // Type identifiers before "for" are the trait; after "for" are the type
        if kind == "type_identifier" || kind == "generic_type" || kind == "scoped_type_identifier" {
            let name = &source[child.byte_range()];
            // For generic types, take just the base name
            let base = name.split('<').next().unwrap_or(name).trim();
            if !saw_for {
                trait_name = Some(base.to_string());
            } else {
                type_name = Some(base.to_string());
            }
        }
    }

    if let (Some(type_n), Some(trait_n)) = (type_name, trait_name) {
        let type_id = symbol_id(path_str, &type_n);
        let trait_id = symbol_id(path_str, &trait_n);
        edges.push(Edge::new(&type_id, EdgeKind::Implements, &trait_id));
    }
}

/// Extract the target name from a use declaration like `pub use foo::Bar;`
fn extract_use_target(text: &str) -> Option<&str> {
    let text = text.trim().strip_prefix("pub")?.trim().strip_prefix("use")?.trim();
    let text = text.strip_suffix(';').unwrap_or(text).trim();
    // Handle `foo::Bar` → "Bar", `foo::*` → None, `foo::{A, B}` → None
    if text.contains('{') || text.ends_with('*') {
        return None;
    }
    text.rsplit("::").next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn rust_adapter_extracts_symbols_and_tests() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(
            root.join("lib.rs"),
            r#"
pub struct Foo;

pub enum Bar {
    A,
    B,
}

pub trait Baz {
    fn do_something(&self);
}

pub fn helper() {}

#[test]
fn test_helper() {
    assert_eq!(1, 1);
}
"#,
        )
        .unwrap();

        let file = WalkedFile {
            path: std::path::PathBuf::from("lib.rs"),
            hash: "abc".to_string(),
            component_id: None,
        };
        let (entities, edges) = index_rust_file(&file, root).unwrap();

        assert!(entities.iter().any(|e| e.kind == EntityKind::SourceUnit));

        let symbols: Vec<_> = entities
            .iter()
            .filter(|e| e.kind == EntityKind::Symbol)
            .map(|e| e.name.as_str())
            .collect();
        assert!(symbols.contains(&"Foo"));
        assert!(symbols.contains(&"Bar"));
        assert!(symbols.contains(&"Baz"));
        assert!(symbols.contains(&"helper"));

        let tests: Vec<_> = entities
            .iter()
            .filter(|e| e.kind == EntityKind::Test)
            .map(|e| e.name.as_str())
            .collect();
        assert!(tests.contains(&"test_helper"));

        assert!(edges.iter().any(|e| {
            e.src_id == "source_unit::lib.rs"
                && e.rel == EdgeKind::Defines
                && e.dst_id == "symbol::lib.rs::Foo"
        }));
        assert!(edges.iter().any(|e| {
            e.src_id == "source_unit::lib.rs"
                && e.rel == EdgeKind::TestedBy
                && e.dst_id == "test::lib.rs::test_helper"
        }));
    }

    #[test]
    fn rust_adapter_extracts_visibility() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(
            root.join("lib.rs"),
            r#"
pub fn public_fn() {}
fn private_fn() {}
pub(crate) fn crate_fn() {}
"#,
        )
        .unwrap();

        let file = WalkedFile {
            path: std::path::PathBuf::from("lib.rs"),
            hash: "abc".to_string(),
            component_id: None,
        };
        let (entities, _) = index_rust_file(&file, root).unwrap();

        let public = entities.iter().find(|e| e.name == "public_fn").unwrap();
        assert_eq!(public.visibility, Some(Visibility::Public));
        assert!(public.exported);

        let private = entities.iter().find(|e| e.name == "private_fn").unwrap();
        assert_eq!(private.visibility, None);
        assert!(!private.exported);

        let crate_fn = entities.iter().find(|e| e.name == "crate_fn").unwrap();
        assert_eq!(crate_fn.visibility, Some(Visibility::Internal));
        assert!(!crate_fn.exported);
    }

    #[test]
    fn rust_adapter_extracts_bench() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(
            root.join("benches.rs"),
            r#"
#[bench]
fn bench_sort(b: &mut Bencher) {
    b.iter(|| {});
}

fn not_a_bench() {}
"#,
        )
        .unwrap();

        let file = WalkedFile {
            path: std::path::PathBuf::from("benches.rs"),
            hash: "abc".to_string(),
            component_id: None,
        };
        let (entities, edges) = index_rust_file(&file, root).unwrap();

        assert!(entities
            .iter()
            .any(|e| e.kind == EntityKind::Bench && e.name == "bench_sort"));
        assert!(entities
            .iter()
            .any(|e| e.kind == EntityKind::Symbol && e.name == "not_a_bench"));
        assert!(edges
            .iter()
            .any(|e| e.rel == EdgeKind::BenchmarkedBy && e.dst_id.contains("bench_sort")));
    }

    #[test]
    fn rust_adapter_extracts_impl_trait() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(
            root.join("lib.rs"),
            r#"
pub trait Display {}
pub struct Foo;
impl Display for Foo {}
"#,
        )
        .unwrap();

        let file = WalkedFile {
            path: std::path::PathBuf::from("lib.rs"),
            hash: "abc".to_string(),
            component_id: None,
        };
        let (_, edges) = index_rust_file(&file, root).unwrap();

        assert!(edges.iter().any(|e| {
            e.src_id == "symbol::lib.rs::Foo"
                && e.rel == EdgeKind::Implements
                && e.dst_id == "symbol::lib.rs::Display"
        }));
    }

    #[test]
    fn rust_adapter_extracts_reexports() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(
            root.join("lib.rs"),
            r#"
pub use crate::inner::Widget;
use std::io;
"#,
        )
        .unwrap();

        let file = WalkedFile {
            path: std::path::PathBuf::from("lib.rs"),
            hash: "abc".to_string(),
            component_id: None,
        };
        let (_, edges) = index_rust_file(&file, root).unwrap();

        assert!(edges.iter().any(|e| {
            e.src_id == "source_unit::lib.rs"
                && e.rel == EdgeKind::Reexports
                && e.dst_id == "symbol::lib.rs::Widget"
        }));
        // Non-pub use should NOT produce a reexport edge
        assert!(!edges.iter().any(|e| e.rel == EdgeKind::Reexports
            && e.dst_id.contains("io")));
    }
}
