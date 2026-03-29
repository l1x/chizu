use std::fs;

use chizu_core::{Config, Store, Summary, TaskRoute};
use chizu_index::IndexPipeline;
use tempfile::TempDir;

#[test]
fn cleanup_renamed_component_removes_old_data() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Create workspace
    fs::write(
        root.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/foo"]
resolver = "2"
"#,
    )
    .unwrap();

    fs::create_dir_all(root.join("crates/foo/src")).unwrap();
    fs::write(
        root.join("crates/foo/Cargo.toml"),
        r#"[package]
name = "foo"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();
    fs::write(
        root.join("crates/foo/src/lib.rs"),
        "pub fn foo_fn() {}\n",
    )
    .unwrap();

    let config = Config::default();
    let store = chizu_core::ChizuStore::open(&root.join(".chizu"), &config).unwrap();

    // First index
    IndexPipeline::run(root, &store, &config, None).unwrap();

    // Verify old component exists
    assert!(store
        .get_entity("component::cargo::crates/foo")
        .unwrap()
        .is_some());
    assert!(store
        .get_entity("symbol::crates/foo/src/lib.rs::foo_fn")
        .unwrap()
        .is_some());
    assert!(store.get_file("crates/foo/src/lib.rs").unwrap().is_some());

    // Simulate derived data that would exist after summarization/embedding
    store
        .insert_summary(&Summary::new(
            "symbol::crates/foo/src/lib.rs::foo_fn",
            "Does foo things",
        ))
        .unwrap();
    store
        .insert_task_route(&TaskRoute::new(
            "debug",
            "symbol::crates/foo/src/lib.rs::foo_fn",
            80,
        ))
        .unwrap();

    // Rename component directory
    fs::rename(root.join("crates/foo"), root.join("crates/foo-renamed")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/foo-renamed"]
resolver = "2"
"#,
    )
    .unwrap();
    fs::write(
        root.join("crates/foo-renamed/Cargo.toml"),
        r#"[package]
name = "foo-renamed"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();

    // Re-index
    IndexPipeline::run(root, &store, &config, None).unwrap();

    // Old component, entities, files, AND derived data should all be gone
    assert!(store
        .get_entity("component::cargo::crates/foo")
        .unwrap()
        .is_none());
    assert!(store
        .get_entity("symbol::crates/foo/src/lib.rs::foo_fn")
        .unwrap()
        .is_none());
    assert!(store.get_file("crates/foo/src/lib.rs").unwrap().is_none());
    assert!(store
        .get_summary("symbol::crates/foo/src/lib.rs::foo_fn")
        .unwrap()
        .is_none());
    assert!(store
        .get_entity_task_routes("symbol::crates/foo/src/lib.rs::foo_fn")
        .unwrap()
        .is_empty());

    // New component should exist
    assert!(store
        .get_entity("component::cargo::crates/foo-renamed")
        .unwrap()
        .is_some());
    assert!(store
        .get_entity("symbol::crates/foo-renamed/src/lib.rs::foo_fn")
        .unwrap()
        .is_some());
    assert!(store
        .get_file("crates/foo-renamed/src/lib.rs")
        .unwrap()
        .is_some());

    store.close().unwrap();
}
