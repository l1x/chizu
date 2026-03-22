use std::collections::HashSet;
use std::fmt;
use std::path::Path;

use grakno_core::model::{Edge, EdgeKind, Entity, EntityKind, FileRecord};
use grakno_core::Store;

use crate::discover::discover;
use crate::error::IndexError;
use crate::id;
use crate::parser::parse_rust_file;

#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    pub crates_found: usize,
    pub files_indexed: usize,
    pub files_skipped: usize,
    pub files_removed: usize,
    pub symbols_extracted: usize,
    pub edges_created: usize,
}

impl fmt::Display for IndexStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "crates:  {}", self.crates_found)?;
        writeln!(f, "files:   {}", self.files_indexed)?;
        writeln!(f, "skipped: {}", self.files_skipped)?;
        writeln!(f, "removed: {}", self.files_removed)?;
        writeln!(f, "symbols: {}", self.symbols_extracted)?;
        write!(f, "edges:   {}", self.edges_created)
    }
}

pub fn index_project(store: &Store, path: &Path) -> Result<IndexStats, IndexError> {
    let workspace = discover(path)?;
    let mut stats = IndexStats::default();

    let repo_id = id::repo_id(&workspace.name);
    store.insert_entity(&Entity {
        id: repo_id.clone(),
        kind: EntityKind::Repo,
        name: workspace.name.clone(),
        component_id: None,
        path: Some(workspace.root.display().to_string()),
        language: Some("rust".to_string()),
        line_start: None,
        line_end: None,
        visibility: Some("pub".to_string()),
        exported: true,
    })?;

    for krate in &workspace.crates {
        stats.crates_found += 1;

        let comp_id = id::component_id(&krate.name);
        store.insert_entity(&Entity {
            id: comp_id.clone(),
            kind: EntityKind::Component,
            name: krate.name.clone(),
            component_id: None,
            path: Some(
                krate
                    .manifest_dir
                    .strip_prefix(&workspace.root)
                    .unwrap_or(&krate.manifest_dir)
                    .display()
                    .to_string(),
            ),
            language: Some("rust".to_string()),
            line_start: None,
            line_end: None,
            visibility: Some("pub".to_string()),
            exported: true,
        })?;

        // Repo → Contains → Component
        store.insert_edge(&Edge {
            src_id: repo_id.clone(),
            rel: EdgeKind::Contains,
            dst_id: comp_id.clone(),
            provenance_path: Some("Cargo.toml".to_string()),
            provenance_line: None,
        })?;
        stats.edges_created += 1;

        // Walk .rs files under src/
        let src_dir = krate.manifest_dir.join("src");
        if src_dir.is_dir() {
            let mut indexed_files = HashSet::new();
            index_directory(
                store,
                &src_dir,
                &workspace.root,
                &krate.name,
                &comp_id,
                &mut stats,
                &mut indexed_files,
            )?;
            cleanup_deleted_files(store, &comp_id, &krate.name, &indexed_files, &mut stats)?;
        }
    }

    // Component → DependsOn → Component edges
    for dep in &workspace.deps {
        store.insert_edge(&Edge {
            src_id: id::component_id(&dep.from),
            rel: EdgeKind::DependsOn,
            dst_id: id::component_id(&dep.to),
            provenance_path: Some("Cargo.toml".to_string()),
            provenance_line: None,
        })?;
        stats.edges_created += 1;
    }

    Ok(stats)
}

fn index_directory(
    store: &Store,
    dir: &Path,
    workspace_root: &Path,
    crate_name: &str,
    comp_id: &str,
    stats: &mut IndexStats,
    indexed_files: &mut HashSet<String>,
) -> Result<(), IndexError> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.path());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            index_directory(
                store,
                &path,
                workspace_root,
                crate_name,
                comp_id,
                stats,
                indexed_files,
            )?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            index_file(
                store,
                &path,
                workspace_root,
                crate_name,
                comp_id,
                stats,
                indexed_files,
            )?;
        }
    }
    Ok(())
}

fn index_file(
    store: &Store,
    path: &Path,
    workspace_root: &Path,
    crate_name: &str,
    comp_id: &str,
    stats: &mut IndexStats,
    indexed_files: &mut HashSet<String>,
) -> Result<(), IndexError> {
    let source = std::fs::read_to_string(path)?;
    let rel_path = path.strip_prefix(workspace_root).unwrap_or(path);
    let rel_path_str = rel_path.display().to_string();

    // Track this file as discovered
    indexed_files.insert(rel_path_str.clone());

    // Hash content with blake3
    let hash = format!("blake3:{}", blake3::hash(source.as_bytes()).to_hex());

    // Check if file is unchanged
    if let Ok(existing) = store.get_file(&rel_path_str) {
        if existing.hash == hash {
            stats.files_skipped += 1;
            return Ok(());
        }
        // File changed — clean up old entities before re-indexing
        let su_id = id::source_unit_id(crate_name, &rel_path_str);
        cleanup_source_unit(store, comp_id, &su_id, &rel_path_str)?;
    }

    // Insert FileRecord
    store.insert_file(&FileRecord {
        path: rel_path_str.clone(),
        component_id: Some(comp_id.to_string()),
        kind: "rust".to_string(),
        hash,
        indexed: true,
        ignore_reason: None,
    })?;

    // Insert SourceUnit entity
    let su_id = id::source_unit_id(crate_name, &rel_path_str);
    store.insert_entity(&Entity {
        id: su_id.clone(),
        kind: EntityKind::SourceUnit,
        name: rel_path_str.clone(),
        component_id: Some(comp_id.to_string()),
        path: Some(rel_path_str.clone()),
        language: Some("rust".to_string()),
        line_start: None,
        line_end: None,
        visibility: None,
        exported: false,
    })?;

    // Component → Contains → SourceUnit
    store.insert_edge(&Edge {
        src_id: comp_id.to_string(),
        rel: EdgeKind::Contains,
        dst_id: su_id.clone(),
        provenance_path: Some(rel_path_str.clone()),
        provenance_line: None,
    })?;
    stats.edges_created += 1;
    stats.files_indexed += 1;

    // Parse and extract symbols
    let symbols = parse_rust_file(&source)?;
    for sym in &symbols {
        let (entity_kind, entity_id) = if sym.is_test {
            (EntityKind::Test, id::test_id(crate_name, &sym.name))
        } else if sym.is_bench {
            (EntityKind::Bench, id::bench_id(crate_name, &sym.name))
        } else {
            (EntityKind::Symbol, id::symbol_id(crate_name, &sym.name))
        };

        let exported = sym.visibility == "pub";

        store.insert_entity(&Entity {
            id: entity_id.clone(),
            kind: entity_kind,
            name: sym.name.clone(),
            component_id: Some(comp_id.to_string()),
            path: Some(rel_path_str.clone()),
            language: Some("rust".to_string()),
            line_start: Some(sym.line_start as i64),
            line_end: Some(sym.line_end as i64),
            visibility: Some(sym.visibility.clone()),
            exported,
        })?;

        // SourceUnit → Defines → Symbol
        store.insert_edge(&Edge {
            src_id: su_id.clone(),
            rel: EdgeKind::Defines,
            dst_id: entity_id,
            provenance_path: Some(rel_path_str.clone()),
            provenance_line: Some(sym.line_start as i64),
        })?;
        stats.edges_created += 1;
        stats.symbols_extracted += 1;
    }

    Ok(())
}

/// Remove all entities and edges associated with a source unit.
fn cleanup_source_unit(
    store: &Store,
    comp_id: &str,
    su_id: &str,
    rel_path: &str,
) -> Result<(), IndexError> {
    // Delete symbols/tests/benches defined in this source unit
    let defines_edges = store.edges_from(su_id)?;
    for edge in &defines_edges {
        if edge.rel == EdgeKind::Defines {
            store.delete_edges_to(&edge.dst_id)?;
            store.delete_entity(&edge.dst_id)?;
        }
    }

    // Delete all edges from the source unit (Defines edges)
    store.delete_edges_from(su_id)?;

    // Delete Component → Contains → SourceUnit edge
    store.delete_edge(comp_id, EdgeKind::Contains, su_id)?;

    // Delete the source unit entity
    store.delete_entity(su_id)?;

    // Delete the file record
    store.delete_file(rel_path)?;

    Ok(())
}

/// Remove stored files that no longer exist on disk for a given component.
fn cleanup_deleted_files(
    store: &Store,
    comp_id: &str,
    crate_name: &str,
    indexed_files: &HashSet<String>,
    stats: &mut IndexStats,
) -> Result<(), IndexError> {
    let stored_files = store.list_files(Some(comp_id))?;
    for file in &stored_files {
        if !indexed_files.contains(&file.path) {
            let su_id = id::source_unit_id(crate_name, &file.path);
            cleanup_source_unit(store, comp_id, &su_id, &file.path)?;
            stats.files_removed += 1;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_grakno_workspace() {
        let store = Store::open_in_memory().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();

        let stats = index_project(&store, root).unwrap();

        assert!(stats.crates_found >= 2, "expected >= 2 crates");
        assert!(stats.files_indexed > 0, "expected some files");
        assert!(stats.symbols_extracted > 0, "expected some symbols");
        assert!(stats.edges_created > 0, "expected some edges");
        assert_eq!(stats.files_skipped, 0, "first run should skip nothing");

        // Verify repo entity exists
        let repo = store.get_entity("repo::grakno").unwrap();
        assert_eq!(repo.kind, EntityKind::Repo);

        // Verify component entities
        let core = store.get_entity("component::grakno-core").unwrap();
        assert_eq!(core.kind, EntityKind::Component);

        // Verify edges exist
        let repo_edges = store.edges_from("repo::grakno").unwrap();
        assert!(
            repo_edges.len() >= 2,
            "repo should have edges to components"
        );

        // Verify graph stats show non-zero counts
        let graph_stats = store.stats().unwrap();
        assert!(graph_stats.entities > 0);
        assert!(graph_stats.edges > 0);
        assert!(graph_stats.files > 0);
    }

    #[test]
    fn incremental_skips_unchanged_files() {
        let store = Store::open_in_memory().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();

        // First run: indexes everything
        let stats1 = index_project(&store, root).unwrap();
        assert!(stats1.files_indexed > 0);
        assert_eq!(stats1.files_skipped, 0);

        let entities_after_first = store.stats().unwrap().entities;

        // Second run: everything should be skipped
        let stats2 = index_project(&store, root).unwrap();
        assert_eq!(stats2.files_indexed, 0, "no files should be re-indexed");
        assert_eq!(
            stats2.files_skipped, stats1.files_indexed,
            "all files should be skipped"
        );

        // Entity count should remain the same
        let entities_after_second = store.stats().unwrap().entities;
        assert_eq!(entities_after_first, entities_after_second);
    }

    #[test]
    fn file_hash_uses_blake3_prefix() {
        let store = Store::open_in_memory().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();

        index_project(&store, root).unwrap();

        let files = store.list_files(None).unwrap();
        assert!(!files.is_empty());
        for file in &files {
            assert!(
                file.hash.starts_with("blake3:"),
                "hash should use blake3 prefix, got: {}",
                file.hash
            );
        }
    }
}
