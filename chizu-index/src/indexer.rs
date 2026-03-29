use std::collections::HashMap;
use std::path::Path;

use chizu_core::{ChizuStore, EntityKind, FileKind, FileRecord, Provider, Store, StoreError};

use crate::adapter::cargo::index_cargo_workspace;
use crate::adapter::npm::index_npm_workspace;
use crate::adapter::site::index_sites;
use crate::adapter::index_file;
use crate::cleanup::cascade_delete_file;
use crate::embedder::Embedder;
use crate::error::Result;
use crate::ownership::{assign_ownership, discover_all_components};
use crate::registry::ComponentRegistry;
use crate::summarizer::Summarizer;
use crate::walk::{FileWalker, WalkedFile};
use chizu_core::Config;

#[derive(Debug, Default)]
pub struct IndexStats {
    pub files_walked: usize,
    pub files_indexed: usize,
    pub entities_inserted: usize,
    pub edges_inserted: usize,
    pub components_discovered: usize,
    pub summaries_generated: usize,
    pub summaries_skipped: usize,
    pub summaries_failed: usize,
    pub embeddings_generated: usize,
    pub embeddings_skipped: usize,
    pub embeddings_failed: usize,
}

pub struct IndexPipeline;

impl IndexPipeline {
    pub fn run(
        repo_root: &Path,
        store: &ChizuStore,
        config: &Config,
        provider: Option<&dyn Provider>,
    ) -> Result<IndexStats> {
        let mut stats = IndexStats::default();

        let walker = FileWalker::new(repo_root, config)?;
        let mut files = walker.walk()?;
        stats.files_walked = files.len();

        let registry = discover_all_components(&files, repo_root)?;
        stats.components_discovered = registry.all_components().count();

        assign_ownership(&mut files, &registry);

        let existing_files: HashMap<String, FileRecord> = store
            .get_all_files()?
            .into_iter()
            .map(|f| (f.path.clone(), f))
            .collect();

        let (changed, deleted) = classify_files(&files, &existing_files);

        // Main transaction: cleanup + workspace adapters + file adapters
        store.in_transaction(|store| {
            for path in &deleted {
                cascade_delete_file(store, path)?;
            }

            cleanup_orphaned_components(store, "cargo", &registry)?;
            cleanup_orphaned_components(store, "npm", &registry)?;

            // Workspace-level facts (cheap to regenerate — delete all, then insert)
            cleanup_workspace_facts(store)?;

            let cargo_facts = index_cargo_workspace(repo_root, &registry)
                .map_err(|e| StoreError::Other(e.to_string()))?;
            for entity in &cargo_facts.entities {
                store.insert_entity(entity)?;
                stats.entities_inserted += 1;
            }
            for edge in &cargo_facts.edges {
                store.insert_edge(edge)?;
                stats.edges_inserted += 1;
            }

            let npm_facts = index_npm_workspace(repo_root, &registry)
                .map_err(|e| StoreError::Other(e.to_string()))?;
            for entity in &npm_facts.entities {
                store.insert_entity(entity)?;
                stats.entities_inserted += 1;
            }
            for edge in &npm_facts.edges {
                store.insert_edge(edge)?;
                stats.edges_inserted += 1;
            }

            for file in &changed {
                let path_str = file.path.to_string_lossy().to_string();
                cascade_delete_file(store, &path_str)?;
                let (entities, edges) = index_file(repo_root, file, &registry)
                    .map_err(|e| StoreError::Other(e.to_string()))?;
                for entity in &entities {
                    store.insert_entity(entity)?;
                    stats.entities_inserted += 1;
                }
                for edge in &edges {
                    store.insert_edge(edge)?;
                    stats.edges_inserted += 1;
                }
            }

            for file in &files {
                let kind = classify_file(&file.path);
                let mut record = FileRecord::new(
                    file.path.to_string_lossy().to_string(),
                    kind,
                    &file.hash,
                );
                if let Some(ref comp_id) = file.component_id {
                    record = record.with_component(comp_id.clone());
                }
                store.insert_file(&record)?;
                stats.files_indexed += 1;
            }

            Ok(())
        })?;

        // Site adapter runs after the main transaction so it can see
        // content pages and templates in the walked files.
        let site_facts = index_sites(repo_root, &files)
            .map_err(|e| StoreError::Other(e.to_string()))?;
        if !site_facts.entities.is_empty() || !site_facts.edges.is_empty() {
            store.in_transaction(|store| {
                for entity in &site_facts.entities {
                    store.insert_entity(entity)?;
                    stats.entities_inserted += 1;
                }
                for edge in &site_facts.edges {
                    store.insert_edge(edge)?;
                    stats.edges_inserted += 1;
                }
                Ok(())
            })?;
        }

        // Summary generation
        if let Some(provider) = provider {
            if config.summary.provider.is_some() {
                let summary_stats = Summarizer::new(provider, &config.summary).run(store, repo_root)?;
                stats.summaries_generated = summary_stats.generated;
                stats.summaries_skipped = summary_stats.skipped;
                stats.summaries_failed = summary_stats.failed;
            }

            // Embedding generation
            if config.embedding.provider.is_some() {
                let embedding_stats = Embedder::new(provider, &config.embedding).run(store)?;
                stats.embeddings_generated = embedding_stats.generated;
                stats.embeddings_skipped = embedding_stats.skipped;
                stats.embeddings_failed = embedding_stats.failed;
            }
        }

        Ok(stats)
    }
}

/// Classify current files against existing store records.
///
/// Returns `(changed_files, deleted_paths)`.
/// A file is unchanged only if both its hash and component_id match the stored record.
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

/// Remove orphaned components for a specific ecosystem.
///
/// Only deletes components whose IDs start with `component::{ecosystem}::` and
/// are no longer present in the registry.
fn cleanup_orphaned_components(
    store: &ChizuStore,
    ecosystem: &str,
    registry: &ComponentRegistry,
) -> std::result::Result<(), StoreError> {
    let sqlite = store.sqlite();
    let prefix = format!("component::{}::", ecosystem);
    let components = sqlite.get_entities_by_kind(EntityKind::Component)?;
    for comp in components {
        if comp.id.starts_with(&prefix) {
            let still_exists = registry.all_components().any(|(_, id)| id.as_str() == comp.id);
            if !still_exists {
                if let Some(ref cid) = comp.component_id {
                    let owned = sqlite.get_entities_by_component(cid)?;
                    for entity in owned {
                        crate::cleanup::cascade_delete_entity(store, &entity.id)?;
                    }
                    sqlite.delete_files_by_component(cid)?;
                }
                crate::cleanup::cascade_delete_entity(store, &comp.id)?;
            }
        }
    }
    Ok(())
}

/// Delete all workspace-level facts so they can be regenerated cleanly.
///
/// Cascade-deletes each entity (including its summaries, embeddings,
/// task routes, and edges) so no derived data is left behind.
fn cleanup_workspace_facts(store: &ChizuStore) -> std::result::Result<(), StoreError> {
    let kinds = [
        EntityKind::Repo,
        EntityKind::Component,
        EntityKind::Feature,
        EntityKind::Site,
    ];
    for kind in &kinds {
        let entities = store.get_entities_by_kind(*kind)?;
        for entity in entities {
            crate::cleanup::cascade_delete_entity(store, &entity.id)?;
        }
    }
    Ok(())
}

/// Basic file kind classification from extension/name.
fn classify_file(path: &Path) -> FileKind {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    // Path-based rules take priority over extension-based rules.
    if path.components().any(|c| c.as_os_str() == ".github") {
        return FileKind::Workflow;
    }
    if name == "Dockerfile" || name == "docker-compose.yml" {
        return FileKind::Config;
    }
    if name == "Makefile" || name == "Justfile" || name == "mise.toml" {
        return FileKind::Build;
    }
    if name == "Cargo.toml" || name == "package.json" {
        return FileKind::Build;
    }

    match ext {
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "c" | "cpp" | "h" | "rb"
        | "swift" | "kt" | "scala" | "zig" | "hs" | "ex" | "exs" | "erl" | "clj" => {
            FileKind::Source
        }
        "md" | "txt" | "rst" | "adoc" => FileKind::Doc,
        "toml" | "yaml" | "yml" | "json" | "ini" | "env" => FileKind::Config,
        "lock" => FileKind::Build,
        "sql" => FileKind::Migration,
        "html" | "hbs" | "astro" | "svelte" | "vue" | "njk" | "tera" => FileKind::Template,
        "tla" => FileKind::Data,
        _ => FileKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn classify_rust_source() {
        assert_eq!(classify_file(Path::new("src/main.rs")), FileKind::Source);
    }

    #[test]
    fn classify_markdown_doc() {
        assert_eq!(classify_file(Path::new("README.md")), FileKind::Doc);
    }

    #[test]
    fn classify_cargo_toml_as_build() {
        assert_eq!(classify_file(Path::new("Cargo.toml")), FileKind::Build);
    }

    #[test]
    fn classify_config_yaml() {
        assert_eq!(
            classify_file(Path::new(".chizu.toml")),
            FileKind::Config
        );
    }

    #[test]
    fn classify_github_workflow() {
        assert_eq!(
            classify_file(Path::new(".github/workflows/ci.yml")),
            FileKind::Workflow
        );
    }

    #[test]
    fn classify_sql_migration() {
        assert_eq!(
            classify_file(Path::new("migrations/001.sql")),
            FileKind::Migration
        );
    }

    #[test]
    fn classify_unknown_as_other() {
        assert_eq!(classify_file(Path::new("data.bin")), FileKind::Other);
    }

    #[test]
    fn classify_files_unchanged_when_hash_and_component_match() {
        let current = vec![WalkedFile {
            path: Path::new("src/lib.rs").to_path_buf(),
            hash: "abc".to_string(),
            component_id: Some(chizu_core::ComponentId::new("cargo", ".")),
        }];
        let mut existing = HashMap::new();
        existing.insert(
            "src/lib.rs".to_string(),
            FileRecord::new("src/lib.rs", FileKind::Source, "abc")
                .with_component(chizu_core::ComponentId::new("cargo", ".")),
        );
        let (changed, deleted) = classify_files(&current, &existing);
        assert!(changed.is_empty());
        assert!(deleted.is_empty());
    }

    #[test]
    fn classify_files_changed_when_component_differs() {
        let current = vec![WalkedFile {
            path: Path::new("src/lib.rs").to_path_buf(),
            hash: "abc".to_string(),
            component_id: Some(chizu_core::ComponentId::new("cargo", "crate1")),
        }];
        let mut existing = HashMap::new();
        existing.insert(
            "src/lib.rs".to_string(),
            FileRecord::new("src/lib.rs", FileKind::Source, "abc")
                .with_component(chizu_core::ComponentId::new("cargo", "crate2")),
        );
        let (changed, deleted) = classify_files(&current, &existing);
        assert_eq!(changed.len(), 1);
        assert!(deleted.is_empty());
    }

    #[test]
    fn classify_files_deleted_when_missing() {
        let current = vec![];
        let mut existing = HashMap::new();
        existing.insert(
            "src/lib.rs".to_string(),
            FileRecord::new("src/lib.rs", FileKind::Source, "abc"),
        );
        let (changed, deleted) = classify_files(&current, &existing);
        assert!(changed.is_empty());
        assert_eq!(deleted.len(), 1);
    }
}
