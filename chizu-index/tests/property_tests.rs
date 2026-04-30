//! Property-based tests for chizu-index using bolero.
//!
//! Tests invariants of the registry, file walker, classification,
//! and incremental change detection logic.

use bolero::check;
use chizu_core::{ComponentId, Config, FileKind, FileRecord};
use chizu_index::registry::ComponentRegistry;
use chizu_index::walk::WalkedFile;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ── ComponentRegistry properties ─────────────────────────────────────

#[test]
fn registry_component_for_path_returns_only_registered_components() {
    // If component_for_path returns Some(id), that id must be one we registered.
    check!()
        .with_type::<(Vec<(String, String)>, String)>()
        .for_each(|(components, query_path)| {
            let mut registry = ComponentRegistry::new();
            let mut registered_ids = Vec::new();

            for (i, (dir, name)) in components.iter().enumerate() {
                // Avoid empty names and ensure unique paths
                if dir.is_empty() || name.is_empty() {
                    continue;
                }
                let path = PathBuf::from(format!("crates/{}/{}", i, dir));
                registry.register(path, name.clone(), "cargo");
                registered_ids.push(ComponentId::new("cargo", &format!("crates/{}/{}", i, dir)));
            }

            if let Some(found) = registry.component_for_path(Path::new(&query_path)) {
                assert!(
                    registered_ids.contains(found),
                    "component_for_path returned {:?} which was never registered",
                    found
                );
            }
        });
}

#[test]
fn registry_resolve_name_returns_only_registered_names() {
    check!()
        .with_type::<(Vec<(String, String)>, String)>()
        .for_each(|(components, query_name)| {
            let mut registry = ComponentRegistry::new();
            let mut registered_names = Vec::new();

            for (i, (dir, name)) in components.iter().enumerate() {
                if name.is_empty() {
                    continue;
                }
                let path = PathBuf::from(format!("pkg/{}/{}", i, dir));
                registry.register(path, name.clone(), "npm");
                registered_names.push(name.clone());
            }

            if let Some(_id) = registry.resolve_name(query_name) {
                assert!(
                    registered_names.contains(query_name),
                    "resolve_name returned a result for unregistered name {:?}",
                    query_name
                );
            }
        });
}

#[test]
fn registry_longest_prefix_wins() {
    // Given two nested components, a file deep inside should match the more specific one.
    check!().with_type::<String>().for_each(|filename| {
        let mut registry = ComponentRegistry::new();
        registry.register(PathBuf::from("crates"), "parent".to_string(), "cargo");
        registry.register(PathBuf::from("crates/child"), "child".to_string(), "cargo");

        let child_id = ComponentId::new("cargo", "crates/child");

        // Any file under crates/child/ must resolve to the child component
        let deep_path = PathBuf::from("crates/child").join(filename);
        if let Some(found) = registry.component_for_path(&deep_path) {
            assert_eq!(
                *found, child_id,
                "File {:?} should match child, not parent",
                deep_path
            );
        }
    });
}

#[test]
fn registry_merge_preserves_all_components() {
    check!()
        .with_type::<(Vec<String>, Vec<String>)>()
        .for_each(|(names_a, names_b)| {
            let mut reg_a = ComponentRegistry::new();
            let mut reg_b = ComponentRegistry::new();

            for (i, name) in names_a.iter().enumerate() {
                if name.is_empty() {
                    continue;
                }
                reg_a.register(PathBuf::from(format!("a/{}", i)), name.clone(), "cargo");
            }
            for (i, name) in names_b.iter().enumerate() {
                if name.is_empty() {
                    continue;
                }
                reg_b.register(PathBuf::from(format!("b/{}", i)), name.clone(), "npm");
            }

            let count_a = reg_a.all_components().count();
            let count_b = reg_b.all_components().count();
            reg_a.merge_from(reg_b);

            // After merge, we should have at least as many as the larger set
            // (names may collide in by_name, but by_path never will with our scheme)
            assert!(reg_a.all_components().count() >= count_a);
            assert!(reg_a.all_components().count() >= count_b);
        });
}

// ── File classification determinism ──────────────────────────────────

// We can't call classify_file directly (it's private), but we can test
// the observable behavior: the same path always produces the same FileKind
// when indexed through the pipeline. Instead, test the property at the
// observable boundary: file extension → FileKind mapping is consistent.

#[test]
fn file_walker_hash_matches_blake3() {
    // For any file content, the walker's hash must equal blake3::hash of that content.
    check!()
        .with_type::<Vec<u8>>()
        .for_each(|content: &Vec<u8>| {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path();
            let file_path = root.join("test_file.txt");
            std::fs::write(&file_path, content).unwrap();

            let mut config = Config::default();
            config.index.exclude_patterns = vec![];

            let walker = chizu_index::FileWalker::new(root, &config).unwrap();
            let files = walker.walk().unwrap();

            assert_eq!(files.len(), 1);
            let expected = blake3::hash(content).to_hex().to_string();
            assert_eq!(files[0].hash, expected);
        });
}

#[test]
fn file_walker_output_is_always_sorted() {
    // Regardless of filesystem ordering, output must be sorted by path.
    check!()
        .with_type::<Vec<String>>()
        .for_each(|filenames: &Vec<String>| {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path();

            // Create files with sanitized names
            for (i, name) in filenames.iter().enumerate() {
                let safe_name: String = name
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                    .take(20)
                    .collect();
                if safe_name.is_empty() {
                    continue;
                }
                let fname = format!("{}_{}.txt", i, safe_name);
                std::fs::write(root.join(&fname), format!("content {}", i)).unwrap();
            }

            let mut config = Config::default();
            config.index.exclude_patterns = vec![];

            let walker = chizu_index::FileWalker::new(root, &config).unwrap();
            let files = walker.walk().unwrap();

            // Verify sorted
            for window in files.windows(2) {
                assert!(
                    window[0].path <= window[1].path,
                    "Files not sorted: {:?} > {:?}",
                    window[0].path,
                    window[1].path
                );
            }
        });
}

// ── Incremental change detection properties ──────────────────────────

/// Simulate the classify_files logic to test its properties.
fn classify_files<'a>(
    current: &'a [WalkedFile],
    existing: &HashMap<String, FileRecord>,
) -> (Vec<&'a WalkedFile>, Vec<String>) {
    let current_map: HashMap<String, &WalkedFile> = current
        .iter()
        .map(|f| (f.path.to_string_lossy().to_string(), f))
        .collect();

    let mut changed = Vec::new();
    for (path, file) in &current_map {
        match existing.get(path) {
            Some(existing) => {
                let hash_changed = existing.hash != file.hash;
                let component_changed = existing.component_id != file.component_id;
                if hash_changed || component_changed {
                    changed.push(*file);
                }
            }
            None => changed.push(*file),
        }
    }

    let deleted: Vec<String> = existing
        .keys()
        .filter(|p| !current_map.contains_key(p.as_str()))
        .cloned()
        .collect();

    (changed, deleted)
}

#[test]
fn classify_identical_files_produces_no_changes() {
    // If current == existing (same paths, hashes, components), nothing is changed or deleted.
    check!()
        .with_type::<Vec<(String, String)>>()
        .for_each(|entries: &Vec<(String, String)>| {
            let mut current = Vec::new();
            let mut existing = HashMap::new();

            for (path, hash) in entries {
                if path.is_empty() {
                    continue;
                }
                current.push(WalkedFile {
                    path: PathBuf::from(path),
                    hash: hash.clone(),
                    component_id: None,
                });
                existing.insert(
                    path.clone(),
                    FileRecord::new(path.clone(), FileKind::Source, hash.clone()),
                );
            }

            let (changed, deleted) = classify_files(&current, &existing);
            assert!(
                changed.is_empty(),
                "identical files should produce no changes"
            );
            assert!(
                deleted.is_empty(),
                "identical files should produce no deletions"
            );
        });
}

#[test]
fn classify_all_new_files_are_changed() {
    // If existing is empty, every current file should appear as changed.
    check!()
        .with_type::<Vec<String>>()
        .for_each(|paths: &Vec<String>| {
            let existing = HashMap::new();
            let current: Vec<WalkedFile> = paths
                .iter()
                .filter(|p| !p.is_empty())
                .enumerate()
                .map(|(i, p)| WalkedFile {
                    path: PathBuf::from(p),
                    hash: format!("hash_{}", i),
                    component_id: None,
                })
                .collect();

            // Deduplicate by path (HashMap in classify_files will keep last)
            let unique_paths: std::collections::HashSet<String> = current
                .iter()
                .map(|f| f.path.to_string_lossy().to_string())
                .collect();

            let (changed, deleted) = classify_files(&current, &existing);
            assert_eq!(changed.len(), unique_paths.len());
            assert!(deleted.is_empty());
        });
}

#[test]
fn classify_all_removed_files_are_deleted() {
    // If current is empty but existing has files, all should be deleted.
    check!()
        .with_type::<Vec<(String, String)>>()
        .for_each(|entries: &Vec<(String, String)>| {
            let current: Vec<WalkedFile> = vec![];
            let mut existing = HashMap::new();

            for (path, hash) in entries {
                if path.is_empty() {
                    continue;
                }
                existing.insert(
                    path.clone(),
                    FileRecord::new(path.clone(), FileKind::Source, hash.clone()),
                );
            }

            let (changed, deleted) = classify_files(&current, &existing);
            assert!(changed.is_empty());
            assert_eq!(deleted.len(), existing.len());
        });
}

#[test]
fn classify_hash_change_triggers_changed() {
    // If a file exists in both but with different hash, it must appear as changed.
    check!()
        .with_type::<(String, String, String)>()
        .for_each(|(path, hash_old, hash_new)| {
            if path.is_empty() || hash_old == hash_new {
                return;
            }
            let current = vec![WalkedFile {
                path: PathBuf::from(path),
                hash: hash_new.clone(),
                component_id: None,
            }];
            let mut existing = HashMap::new();
            existing.insert(
                path.clone(),
                FileRecord::new(path.clone(), FileKind::Source, hash_old.clone()),
            );

            let (changed, deleted) = classify_files(&current, &existing);
            assert_eq!(changed.len(), 1, "hash change must trigger changed");
            assert!(deleted.is_empty());
        });
}

// ── Ownership assignment properties ──────────────────────────────────

#[test]
fn assign_ownership_is_idempotent() {
    // Running assign_ownership twice produces the same result.
    check!()
        .with_type::<Vec<String>>()
        .for_each(|filenames: &Vec<String>| {
            let mut registry = ComponentRegistry::new();
            registry.register(PathBuf::from("src"), "main".to_string(), "cargo");

            let mut files: Vec<WalkedFile> = filenames
                .iter()
                .filter(|n| !n.is_empty())
                .enumerate()
                .map(|(i, n)| {
                    let safe: String = n.chars().filter(|c| c.is_alphanumeric()).take(10).collect();
                    WalkedFile {
                        path: PathBuf::from(format!("src/{}{}.rs", safe, i)),
                        hash: format!("h{}", i),
                        component_id: None,
                    }
                })
                .collect();

            chizu_index::assign_ownership(&mut files, &registry);
            let first_pass: Vec<_> = files.iter().map(|f| f.component_id.clone()).collect();

            chizu_index::assign_ownership(&mut files, &registry);
            let second_pass: Vec<_> = files.iter().map(|f| f.component_id.clone()).collect();

            assert_eq!(first_pass, second_pass);
        });
}
