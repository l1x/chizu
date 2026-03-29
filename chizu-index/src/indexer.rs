use std::collections::HashSet;
use std::path::Path;

use chizu_core::{ChizuStore, EntityKind, FileKind, FileRecord, Store, StoreError};

use crate::adapter::cargo::index_cargo_workspace;
use crate::error::Result;
use crate::ownership::{assign_ownership, discover_cargo_components};
use crate::walk::FileWalker;
use chizu_core::Config;

#[derive(Debug, Default)]
pub struct IndexStats {
    pub files_walked: usize,
    pub files_indexed: usize,
    pub entities_inserted: usize,
    pub edges_inserted: usize,
    pub components_discovered: usize,
}

pub struct IndexPipeline;

impl IndexPipeline {
    pub fn run(repo_root: &Path, store: &ChizuStore, config: &Config) -> Result<IndexStats> {
        let mut stats = IndexStats::default();

        let walker = FileWalker::new(repo_root, config)?;
        let mut files = walker.walk()?;
        stats.files_walked = files.len();

        let registry = discover_cargo_components(&files, repo_root)?;
        stats.components_discovered = registry.all_components().count();

        let cargo_facts = index_cargo_workspace(repo_root, &registry)?;

        assign_ownership(&mut files, &registry);

        // Wrap all store mutations in a single transaction for atomicity and
        // performance (avoids per-statement fsync in WAL mode).
        // Pre-compute path strings once (avoids double to_string_lossy conversion).
        let file_paths: Vec<String> = files
            .iter()
            .map(|f| f.path.to_string_lossy().into_owned())
            .collect();
        let current_paths: HashSet<&str> = file_paths.iter().map(|s| s.as_str()).collect();

        store.in_transaction(|store| {
            for existing in store.get_all_files()? {
                if !current_paths.contains(existing.path.as_str()) {
                    store.delete_file(&existing.path)?;
                }
            }

            cleanup_cargo_entities(store)?;
            cleanup_cargo_edges(store)?;

            for entity in &cargo_facts.entities {
                store.insert_entity(entity)?;
                stats.entities_inserted += 1;
            }
            for edge in &cargo_facts.edges {
                store.insert_edge(edge)?;
                stats.edges_inserted += 1;
            }

            for (file, path_str) in files.iter().zip(file_paths.iter()) {
                let kind = classify_file(&file.path);
                let mut record = FileRecord::new(path_str.clone(), kind, &file.hash);
                if let Some(ref comp_id) = file.component_id {
                    record = record.with_component(comp_id.clone());
                }
                store.insert_file(&record)?;
                stats.files_indexed += 1;
            }

            Ok(())
        })?;

        Ok(stats)
    }
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

fn cleanup_cargo_entities(store: &ChizuStore) -> std::result::Result<(), StoreError> {
    let kinds_to_clean = [EntityKind::Repo, EntityKind::Component, EntityKind::Feature];
    for kind in &kinds_to_clean {
        let entities = store.get_entities_by_kind(*kind)?;
        for entity in entities {
            if entity.id.starts_with("component::cargo::")
                || entity.id.starts_with("feature::")
                || entity.id.starts_with("repo::")
            {
                store.delete_entity(&entity.id)?;
            }
        }
    }
    Ok(())
}

fn cleanup_cargo_edges(store: &ChizuStore) -> std::result::Result<(), StoreError> {
    let rels = [
        chizu_core::EdgeKind::Contains,
        chizu_core::EdgeKind::DependsOn,
        chizu_core::EdgeKind::DeclaresFeature,
        chizu_core::EdgeKind::FeatureEnables,
    ];
    for rel in &rels {
        let edges = store.get_edges_by_rel(*rel)?;
        for edge in edges {
            if edge.src_id.starts_with("component::cargo::")
                || edge.dst_id.starts_with("component::cargo::")
                || edge.src_id.starts_with("repo::.")
                || edge.src_id.starts_with("feature::")
                || edge.dst_id.starts_with("feature::")
            {
                store.delete_edge(&edge.src_id, *rel, &edge.dst_id)?;
            }
        }
    }
    Ok(())
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
}
