use std::fs;

use chizu_core::{Config, Store};
use chizu_index::IndexPipeline;
use tempfile::TempDir;

#[tokio::test]
async fn cleanup_deleted_file_removes_all_traces() {
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
    fs::write(
        root.join("src/lib.rs"),
        r#"pub fn helper() -> i32 { 42 }

#[test]
fn test_helper() {
    assert_eq!(helper(), 42);
}
"#,
    )
    .unwrap();

    let config = Config::default();
    let store = chizu_core::ChizuStore::open(&root.join(".chizu"), &config).unwrap();

    // First index
    IndexPipeline::run(root, &store, &config, None)
        .await
        .unwrap();

    // Verify entities exist
    assert!(
        store
            .get_entity("symbol::src/lib.rs::helper")
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .get_entity("test::src/lib.rs::test_helper")
            .unwrap()
            .is_some()
    );
    assert!(
        !store
            .get_edges_from("source_unit::src/lib.rs")
            .unwrap()
            .is_empty()
    );

    // Delete lib.rs
    fs::remove_file(root.join("src/lib.rs")).unwrap();

    // Re-index
    IndexPipeline::run(root, &store, &config, None)
        .await
        .unwrap();

    // Verify all traces are gone
    assert!(
        store
            .get_entity("symbol::src/lib.rs::helper")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get_entity("test::src/lib.rs::test_helper")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get_edges_from("source_unit::src/lib.rs")
            .unwrap()
            .is_empty()
    );
    assert!(store.get_file("src/lib.rs").unwrap().is_none());

    // Repo and component should still exist
    assert!(store.get_entity("repo::.").unwrap().is_some());
    assert!(store.get_entity("component::cargo::.").unwrap().is_some());

    store.close().unwrap();
}
