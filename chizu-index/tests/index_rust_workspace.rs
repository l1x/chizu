use std::fs;

use chizu_core::{ComponentId, Config, EdgeKind, EntityKind, Store};
use chizu_index::IndexPipeline;
use tempfile::TempDir;

fn comp_id(ecosystem: &str, path: &str) -> String {
    ComponentId::new(ecosystem, path).to_string()
}

#[tokio::test]
async fn index_rust_workspace_end_to_end() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Create workspace
    fs::write(
        root.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/foo", "crates/bar"]
resolver = "2"
"#,
    )
    .unwrap();

    // Create foo crate
    fs::create_dir_all(root.join("crates/foo/src")).unwrap();
    fs::write(
        root.join("crates/foo/Cargo.toml"),
        r#"[package]
name = "foo"
version = "0.1.0"
edition = "2021"

[features]
default = ["std"]
std = []
"#,
    )
    .unwrap();
    fs::write(
        root.join("crates/foo/src/lib.rs"),
        b"pub fn add(a: i32, b: i32) -> i32 { a + b }",
    )
    .unwrap();

    // Create bar crate that depends on foo
    fs::create_dir_all(root.join("crates/bar/src")).unwrap();
    fs::write(
        root.join("crates/bar/Cargo.toml"),
        r#"[package]
name = "bar"
version = "0.1.0"
edition = "2021"

[dependencies]
foo = { path = "../foo" }
"#,
    )
    .unwrap();
    fs::write(
        root.join("crates/bar/src/main.rs"),
        b"fn main() { foo::add(1, 2); }",
    )
    .unwrap();

    // Run indexing pipeline
    let mut config = Config::default();
    config
        .index
        .exclude_patterns
        .push("**/.chizu/**".to_string());
    let store = chizu_core::ChizuStore::open(&root.join(".chizu"), &config).unwrap();
    let stats = IndexPipeline::run(root, &store, &config, None)
        .await
        .unwrap();

    assert_eq!(stats.components_discovered, 2);
    assert_eq!(stats.files_indexed, 5); // 3 Cargo.toml + 2 source files
    assert!(stats.entities_inserted >= 3); // repo + 2 components + features
    assert!(stats.edges_inserted >= 2); // contains + depends_on

    // Verify repo entity
    let repo = store
        .get_entity("repo::.")
        .unwrap()
        .expect("repo entity should exist");
    assert_eq!(repo.kind, EntityKind::Repo);

    // Verify component entities with canonical path-based IDs
    let foo_comp = store
        .get_entity(&comp_id("cargo", "crates/foo"))
        .unwrap()
        .expect("foo component should exist");
    assert_eq!(foo_comp.kind, EntityKind::Component);
    assert_eq!(foo_comp.name, "foo");

    let bar_comp = store
        .get_entity(&comp_id("cargo", "crates/bar"))
        .unwrap()
        .expect("bar component should exist");
    assert_eq!(bar_comp.kind, EntityKind::Component);
    assert_eq!(bar_comp.name, "bar");

    // Verify contains edges from repo to components
    let contains_edges = store.get_edges_from("repo::.").unwrap();
    let contains_foo = contains_edges
        .iter()
        .any(|e| e.rel == EdgeKind::Contains && e.dst_id == comp_id("cargo", "crates/foo"));
    let contains_bar = contains_edges
        .iter()
        .any(|e| e.rel == EdgeKind::Contains && e.dst_id == comp_id("cargo", "crates/bar"));
    assert!(contains_foo, "repo should contain foo");
    assert!(contains_bar, "repo should contain bar");

    // Verify depends_on edge from bar to foo
    let bar_edges = store
        .get_edges_from(&comp_id("cargo", "crates/bar"))
        .unwrap();
    let depends_on_foo = bar_edges
        .iter()
        .any(|e| e.rel == EdgeKind::DependsOn && e.dst_id == comp_id("cargo", "crates/foo"));
    assert!(depends_on_foo, "bar should depend_on foo");

    // Verify file records have correct component_id
    let foo_lib = store
        .get_file("crates/foo/src/lib.rs")
        .unwrap()
        .expect("foo lib.rs should exist");
    assert_eq!(
        foo_lib.component_id,
        Some(chizu_core::ComponentId::new("cargo", "crates/foo"))
    );

    let bar_main = store
        .get_file("crates/bar/src/main.rs")
        .unwrap()
        .expect("bar main.rs should exist");
    assert_eq!(
        bar_main.component_id,
        Some(chizu_core::ComponentId::new("cargo", "crates/bar"))
    );

    let foo_source = store
        .get_entity("source_unit::crates/foo/src/lib.rs")
        .unwrap()
        .expect("foo source unit should exist");
    assert_eq!(foo_source.kind, EntityKind::SourceUnit);
    assert_eq!(
        foo_source.component_id,
        Some(chizu_core::ComponentId::new("cargo", "crates/foo"))
    );

    let foo_symbol = store
        .get_entity("symbol::crates/foo/src/lib.rs::add")
        .unwrap()
        .expect("foo symbol should exist");
    assert_eq!(foo_symbol.kind, EntityKind::Symbol);
    assert_eq!(
        foo_symbol.component_id,
        Some(chizu_core::ComponentId::new("cargo", "crates/foo"))
    );

    let foo_edges = store
        .get_edges_from(&comp_id("cargo", "crates/foo"))
        .unwrap();
    let contains_source = foo_edges
        .iter()
        .any(|e| e.rel == EdgeKind::Contains && e.dst_id == "source_unit::crates/foo/src/lib.rs");
    assert!(contains_source, "foo should contain its source unit");

    // Verify feature entities and edges
    let std_feature = store.get_entity("feature::crates/foo::std").unwrap();
    assert!(std_feature.is_some());
    assert_eq!(std_feature.unwrap().kind, EntityKind::Feature);

    let declares = store
        .get_edges_from(&comp_id("cargo", "crates/foo"))
        .unwrap();
    let declares_default = declares
        .iter()
        .any(|e| e.rel == EdgeKind::DeclaresFeature && e.dst_id == "feature::crates/foo::default");
    assert!(declares_default);

    let enables = store
        .get_edges_from("feature::crates/foo::default")
        .unwrap();
    let enables_std = enables
        .iter()
        .any(|e| e.rel == EdgeKind::FeatureEnables && e.dst_id == "feature::crates/foo::std");
    assert!(enables_std);
}
