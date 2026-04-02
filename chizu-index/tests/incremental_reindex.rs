use std::fs;

use chizu_core::{Config, EntityKind, Store};
use chizu_index::IndexPipeline;
use tempfile::TempDir;

#[test]
fn incremental_reindex_skips_unchanged() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Create a simple Rust workspace
    fs::write(
        root.join("Cargo.toml"),
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn one() -> i32 { 1 }\n").unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

    let config = Config::default();
    let store = chizu_core::ChizuStore::open(&root.join(".chizu"), &config).unwrap();

    // First index
    let stats1 = IndexPipeline::run(root, &store, &config, None).unwrap();
    assert!(stats1.entities_inserted > 0);

    let symbols_after_first: Vec<_> = store
        .get_entities_by_kind(EntityKind::Symbol)
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert!(symbols_after_first.contains(&"one".to_string()));
    assert!(!symbols_after_first.contains(&"two".to_string()));

    // Modify lib.rs to add a second function
    fs::write(
        root.join("src/lib.rs"),
        "pub fn one() -> i32 { 1 }\npub fn two() -> i32 { 2 }\n",
    )
    .unwrap();

    // Second index
    let stats2 = IndexPipeline::run(root, &store, &config, None).unwrap();
    assert!(stats2.entities_inserted > 0);

    let symbols_after_second: Vec<_> = store
        .get_entities_by_kind(EntityKind::Symbol)
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert!(symbols_after_second.contains(&"one".to_string()));
    assert!(symbols_after_second.contains(&"two".to_string()));

    // main.rs should still be present (unchanged)
    let source_units: Vec<_> = store
        .get_entities_by_kind(EntityKind::SourceUnit)
        .unwrap()
        .into_iter()
        .map(|e| e.path.unwrap_or_default())
        .collect();
    assert!(source_units.contains(&"src/main.rs".to_string()));
    assert!(source_units.contains(&"src/lib.rs".to_string()));

    store.close().unwrap();
}

/// A file whose content is unchanged but whose component_id changes
/// (because a new Cargo.toml appeared) must be re-indexed.
#[test]
fn incremental_reindex_detects_component_change() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Start as a flat crate
    fs::write(
        root.join("Cargo.toml"),
        r#"[package]
name = "app"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\n").unwrap();

    let config = Config::default();
    let store = chizu_core::ChizuStore::open(&root.join(".chizu"), &config).unwrap();

    IndexPipeline::run(root, &store, &config, None).unwrap();

    let file = store.get_file("src/lib.rs").unwrap().unwrap();
    assert_eq!(
        file.component_id.as_ref().map(|c| c.as_str()),
        Some("component::cargo::.")
    );

    // Convert to a workspace with a nested crate that owns src/lib.rs
    fs::write(
        root.join("Cargo.toml"),
        r#"[workspace]
members = ["inner"]
resolver = "2"
"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("inner")).unwrap();
    // Move src into inner/ so it's owned by the new component
    fs::rename(root.join("src"), root.join("inner/src")).unwrap();
    fs::write(
        root.join("inner/Cargo.toml"),
        r#"[package]
name = "inner"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();

    IndexPipeline::run(root, &store, &config, None).unwrap();

    // File should now be owned by the new component
    let file = store.get_file("inner/src/lib.rs").unwrap().unwrap();
    assert_eq!(
        file.component_id.as_ref().map(|c| c.as_str()),
        Some("component::cargo::inner")
    );

    // Old file record should be gone
    assert!(store.get_file("src/lib.rs").unwrap().is_none());

    store.close().unwrap();
}
