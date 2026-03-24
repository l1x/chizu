use std::collections::HashSet;
use std::fmt;
use std::path::Path;

use grakno_core::model::{Edge, EdgeKind, Entity, EntityKind, FileRecord};
use grakno_core::Store;

use crate::error::IndexError;
use crate::id;
use crate::markdown::extract_mentions;
use crate::parser::parse_rust_file;
use crate::parser_astro::parse_astro_file;
use crate::parser_ts::parse_ts_file;

#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    pub crates_found: usize,
    pub files_indexed: usize,
    pub files_skipped: usize,
    pub files_removed: usize,
    pub symbols_extracted: usize,
    pub edges_created: usize,
    pub features_extracted: usize,
    pub docs_indexed: usize,
    pub tasks_extracted: usize,
    pub task_routes_generated: usize,
    pub migrations_indexed: usize,
    pub specs_indexed: usize,
    pub workflows_indexed: usize,
    pub agent_configs_indexed: usize,
    pub templates_indexed: usize,
    pub infra_roots_indexed: usize,
    pub deployables_indexed: usize,
    pub commands_indexed: usize,
    pub sites_detected: usize,
    pub content_pages_indexed: usize,
}

impl fmt::Display for IndexStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "crates:        {}", self.crates_found)?;
        writeln!(f, "files:         {}", self.files_indexed)?;
        writeln!(f, "skipped:       {}", self.files_skipped)?;
        writeln!(f, "removed:       {}", self.files_removed)?;
        writeln!(f, "symbols:       {}", self.symbols_extracted)?;
        writeln!(f, "features:      {}", self.features_extracted)?;
        writeln!(f, "docs:          {}", self.docs_indexed)?;
        writeln!(f, "tasks:         {}", self.tasks_extracted)?;
        writeln!(f, "migrations:    {}", self.migrations_indexed)?;
        writeln!(f, "specs:         {}", self.specs_indexed)?;
        writeln!(f, "workflows:     {}", self.workflows_indexed)?;
        writeln!(f, "agent_configs: {}", self.agent_configs_indexed)?;
        writeln!(f, "templates:     {}", self.templates_indexed)?;
        writeln!(f, "infra_roots:   {}", self.infra_roots_indexed)?;
        writeln!(f, "deployables:   {}", self.deployables_indexed)?;
        writeln!(f, "commands:      {}", self.commands_indexed)?;
        writeln!(f, "sites:         {}", self.sites_detected)?;
        writeln!(f, "content_pages: {}", self.content_pages_indexed)?;
        writeln!(f, "routes:        {}", self.task_routes_generated)?;
        write!(f, "edges:         {}", self.edges_created)
    }
}



/// Index a project by walking the directory and parsing supported files.
/// No assumptions about project structure - works with any codebase.
#[tracing::instrument(skip(store), fields(path = %path.display()))]
pub fn index_project(store: &Store, path: &Path) -> Result<IndexStats, IndexError> {
    tracing::info!("starting generic project indexing");
    let start = std::time::Instant::now();

    let mut stats = IndexStats::default();
    let mut indexed_files = HashSet::new();

    // Walk and index all supported files
    index_generic_walk(
        store,
        path,
        path,
        &mut stats,
        &mut indexed_files,
    )?;

    // Clean up deleted files
    cleanup_generic_deleted_files(store, &indexed_files, &mut stats)?;

    tracing::info!(
        duration_ms = start.elapsed().as_millis() as u64,
        files = stats.files_indexed,
        symbols = stats.symbols_extracted,
        edges = stats.edges_created,
        "generic indexing complete"
    );

    Ok(stats)
}

fn index_generic_walk(
    store: &Store,
    dir: &Path,
    project_root: &Path,
    stats: &mut IndexStats,
    indexed_files: &mut HashSet<String>,
) -> Result<(), IndexError> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.path());

    // Track if this directory has terraform files for InfraRoot creation
    let mut has_main_tf = false;
    
    for entry in &entries {
        let path = entry.path();
        if path.is_file() {
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if file_name == "main.tf" {
                has_main_tf = true;
                break;
            }
        }
    }

    for entry in entries {
        let path = entry.path();
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Skip hidden dirs and common build/output directories
        if path.is_dir() {
            if file_name.starts_with('.') || matches!(file_name, "target" | "node_modules" | "dist" | "build" | "out" | "coverage" | "__pycache__") {
                continue;
            }
            index_generic_walk(store, &path, project_root, stats, indexed_files)?;
        } else {
            // Index supported file types directly
            let rel_path = path.strip_prefix(project_root).unwrap_or(&path);
            let _rel_path_str = rel_path.display().to_string();
            
            // Check file extension for supported languages
            let ext = path.extension().and_then(|e| e.to_str());
            match ext {
                Some("rs") => {
                    index_generic_source_file(store, &path, project_root, "rust", stats, indexed_files)?;
                }
                Some("ts") | Some("tsx") => {
                    index_generic_source_file(store, &path, project_root, "typescript", stats, indexed_files)?;
                }
                Some("astro") => {
                    index_generic_source_file(store, &path, project_root, "astro", stats, indexed_files)?;
                }
                Some("md") => {
                    index_generic_doc_file(store, &path, project_root, stats, indexed_files)?;
                }
                Some("tf") | Some("hcl") => {
                    index_terraform_file(store, &path, project_root, stats, indexed_files)?;
                }
                _ => {
                    // Check for Dockerfile patterns
                    if file_name.contains("Dockerfile") || 
                       (file_name.starts_with("docker-compose") && (file_name.ends_with(".yml") || file_name.ends_with(".yaml"))) {
                        index_deployable_file(store, &path, project_root, stats, indexed_files)?;
                    }
                }
            }
        }
    }
    
    // Create InfraRoot entity if directory has main.tf
    if has_main_tf {
        create_infra_root(store, dir, project_root, stats)?;
    }
    
    Ok(())
}

fn index_generic_source_file(
    store: &Store,
    path: &Path,
    project_root: &Path,
    language: &str,
    stats: &mut IndexStats,
    indexed_files: &mut HashSet<String>,
) -> Result<(), IndexError> {
    let source = std::fs::read_to_string(path)?;
    let rel_path = path.strip_prefix(project_root).unwrap_or(path);
    let rel_path_str = rel_path.display().to_string();

    // Track this file
    indexed_files.insert(rel_path_str.clone());

    // Hash content
    let hash = format!("blake3:{}", blake3::hash(source.as_bytes()).to_hex());

    // Check if unchanged
    if let Ok(existing) = store.get_file(&rel_path_str) {
        if existing.hash == hash {
            stats.files_skipped += 1;
            return Ok(());
        }
        // File changed - clean up old entities
        cleanup_generic_file_entities(store, &rel_path_str)?;
    }

    // Insert/update FileRecord
    store.insert_file(&FileRecord {
        path: rel_path_str.clone(),
        component_id: None,
        kind: language.to_string(),
        hash,
        indexed: true,
        ignore_reason: None,
    })?;

    // Create SourceUnit entity
    let su_id = id::file_entity_id(&rel_path_str);
    store.insert_entity(&Entity {
        id: su_id.clone(),
        kind: EntityKind::SourceUnit,
        name: path.file_name().and_then(|n| n.to_str()).unwrap_or(&rel_path_str).to_string(),
        component_id: None,
        path: Some(rel_path_str.clone()),
        language: Some(language.to_string()),
        line_start: None,
        line_end: None,
        visibility: None,
        exported: false,
    })?;
    stats.files_indexed += 1;

    // Parse and extract symbols based on language
    match language {
        "rust" => {
            if let Ok(parse_result) = parse_rust_file(&source) {
                // Create symbol entities and Defines edges
                for sym in &parse_result.symbols {
                    let entity_kind = if sym.is_test {
                        EntityKind::Test
                    } else if sym.is_bench {
                        EntityKind::Bench
                    } else {
                        EntityKind::Symbol
                    };

                    let entity_id = id::symbol_in_file(&rel_path_str, &sym.name);
                    let exported = sym.visibility == "pub";

                    store.insert_entity(&Entity {
                        id: entity_id.clone(),
                        kind: entity_kind,
                        name: sym.name.clone(),
                        component_id: None,
                        path: Some(rel_path_str.clone()),
                        language: Some(language.to_string()),
                        line_start: Some(sym.line_start as i64),
                        line_end: Some(sym.line_end as i64),
                        visibility: Some(sym.visibility.clone()),
                        exported,
                    })?;

                    // SourceUnit → Defines → Symbol
                    store.insert_edge(&Edge {
                        src_id: su_id.clone(),
                        rel: EdgeKind::Defines,
                        dst_id: entity_id.clone(),
                        provenance_path: Some(rel_path_str.clone()),
                        provenance_line: Some(sym.line_start as i64),
                    })?;
                    stats.edges_created += 1;
                    stats.symbols_extracted += 1;

                    // Additional edges for tests/benchmarks
                    if entity_kind == EntityKind::Test {
                        store.insert_edge(&Edge {
                            src_id: su_id.clone(),
                            rel: EdgeKind::TestedBy,
                            dst_id: entity_id,
                            provenance_path: Some(rel_path_str.clone()),
                            provenance_line: Some(sym.line_start as i64),
                        })?;
                        stats.edges_created += 1;
                    }
                }
            }
        }
        "typescript" => {
            if let Ok(parse_result) = parse_ts_file(&source) {
                for sym in &parse_result.symbols {
                    let entity_id = id::symbol_in_file(&rel_path_str, &sym.name);
                    store.insert_entity(&Entity {
                        id: entity_id.clone(),
                        kind: EntityKind::Symbol,
                        name: sym.name.clone(),
                        component_id: None,
                        path: Some(rel_path_str.clone()),
                        language: Some(language.to_string()),
                        line_start: Some(sym.line_start as i64),
                        line_end: Some(sym.line_end as i64),
                        visibility: if sym.exported { Some("pub".to_string()) } else { None },
                        exported: sym.exported,
                    })?;

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
            }
        }
        "astro" => {
            if let Ok(_parse_result) = parse_astro_file(&source) {
                // Create entity for the component itself
                let comp_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("component");
                let entity_id = id::symbol_in_file(&rel_path_str, comp_name);
                
                store.insert_entity(&Entity {
                    id: entity_id.clone(),
                    kind: EntityKind::Template,
                    name: comp_name.to_string(),
                    component_id: None,
                    path: Some(rel_path_str.clone()),
                    language: Some("astro".to_string()),
                    line_start: None,
                    line_end: None,
                    visibility: Some("pub".to_string()),
                    exported: true,
                })?;

                store.insert_edge(&Edge {
                    src_id: su_id.clone(),
                    rel: EdgeKind::Defines,
                    dst_id: entity_id,
                    provenance_path: Some(rel_path_str.clone()),
                    provenance_line: None,
                })?;
                stats.edges_created += 1;
                stats.symbols_extracted += 1;
            }
        }
        _ => {}
    }

    Ok(())
}

fn index_generic_doc_file(
    store: &Store,
    path: &Path,
    project_root: &Path,
    stats: &mut IndexStats,
    indexed_files: &mut HashSet<String>,
) -> Result<(), IndexError> {
    let content = std::fs::read_to_string(path)?;
    let rel_path = path.strip_prefix(project_root).unwrap_or(path);
    let rel_path_str = rel_path.display().to_string();

    indexed_files.insert(rel_path_str.clone());

    let hash = format!("blake3:{}", blake3::hash(content.as_bytes()).to_hex());

    if let Ok(existing) = store.get_file(&rel_path_str) {
        if existing.hash == hash {
            stats.files_skipped += 1;
            return Ok(());
        }
        cleanup_generic_file_entities(store, &rel_path_str)?;
    }

    store.insert_file(&FileRecord {
        path: rel_path_str.clone(),
        component_id: None,
        kind: "markdown".to_string(),
        hash,
        indexed: true,
        ignore_reason: None,
    })?;

    // Create Doc entity
    let doc_id = id::doc_id("generic", &rel_path_str);
    let title = parse_frontmatter_title(&content)
        .or_else(|| path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| "untitled".to_string());

    store.insert_entity(&Entity {
        id: doc_id.clone(),
        kind: EntityKind::Doc,
        name: title,
        component_id: None,
        path: Some(rel_path_str.clone()),
        language: Some("markdown".to_string()),
        line_start: None,
        line_end: None,
        visibility: None,
        exported: true,
    })?;
    stats.docs_indexed += 1;

    // Extract mentions and create edges
    let mentions = extract_mentions(&content);
    for mention in mentions {
        // Try to find a matching symbol in the graph
        if let Some(symbol_id) = find_symbol_by_name(store, &mention.symbol_name) {
            store.insert_edge(&Edge {
                src_id: doc_id.clone(),
                rel: EdgeKind::Mentions,
                dst_id: symbol_id,
                provenance_path: Some(rel_path_str.clone()),
                provenance_line: Some(mention.line as i64),
            })?;
            stats.edges_created += 1;
        }
    }

    Ok(())
}

fn index_terraform_file(
    store: &Store,
    path: &Path,
    project_root: &Path,
    stats: &mut IndexStats,
    indexed_files: &mut HashSet<String>,
) -> Result<(), IndexError> {
    let content = std::fs::read_to_string(path)?;
    let rel_path = path.strip_prefix(project_root).unwrap_or(path);
    let rel_path_str = rel_path.display().to_string();

    indexed_files.insert(rel_path_str.clone());

    let hash = format!("blake3:{}", blake3::hash(content.as_bytes()).to_hex());

    if let Ok(existing) = store.get_file(&rel_path_str) {
        if existing.hash == hash {
            stats.files_skipped += 1;
            return Ok(());
        }
        cleanup_generic_file_entities(store, &rel_path_str)?;
    }

    store.insert_file(&FileRecord {
        path: rel_path_str.clone(),
        component_id: None,
        kind: "terraform".to_string(),
        hash,
        indexed: true,
        ignore_reason: None,
    })?;

    // Create SourceUnit for the terraform file
    let su_id = id::file_entity_id(&rel_path_str);
    store.insert_entity(&Entity {
        id: su_id.clone(),
        kind: EntityKind::SourceUnit,
        name: path.file_name().and_then(|n| n.to_str()).unwrap_or(&rel_path_str).to_string(),
        component_id: None,
        path: Some(rel_path_str.clone()),
        language: Some("hcl".to_string()),
        line_start: None,
        line_end: None,
        visibility: None,
        exported: false,
    })?;
    stats.files_indexed += 1;

    // Extract terraform resource names as symbols
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("resource") {
            // resource "aws_ecs_service" "api" { ... }
            if let Some(open) = trimmed.find('"') {
                if let Some(close) = trimmed[open+1..].find('"') {
                    let resource_type = &trimmed[open+1..open+1+close];
                    if let Some(name_open) = trimmed[open+1+close+1..].find('"') {
                        let name_start = open+1+close+1+name_open+1;
                        if let Some(name_close) = trimmed[name_start..].find('"') {
                            let resource_name = &trimmed[name_start..name_start+name_close];
                            let symbol_id = format!("{su_id}::{resource_type}::{resource_name}");
                            
                            store.insert_entity(&Entity {
                                id: symbol_id.clone(),
                                kind: EntityKind::Symbol,
                                name: format!("{resource_type}.{resource_name}"),
                                component_id: None,
                                path: Some(rel_path_str.clone()),
                                language: Some("hcl".to_string()),
                                line_start: None,
                                line_end: None,
                                visibility: Some("pub".to_string()),
                                exported: true,
                            })?;
                            
                            store.insert_edge(&Edge {
                                src_id: su_id.clone(),
                                rel: EdgeKind::Defines,
                                dst_id: symbol_id,
                                provenance_path: Some(rel_path_str.clone()),
                                provenance_line: None,
                            })?;
                            stats.edges_created += 1;
                            stats.symbols_extracted += 1;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn index_deployable_file(
    store: &Store,
    path: &Path,
    project_root: &Path,
    stats: &mut IndexStats,
    indexed_files: &mut HashSet<String>,
) -> Result<(), IndexError> {
    let content = std::fs::read_to_string(path)?;
    let rel_path = path.strip_prefix(project_root).unwrap_or(path);
    let rel_path_str = rel_path.display().to_string();

    indexed_files.insert(rel_path_str.clone());

    let hash = format!("blake3:{}", blake3::hash(content.as_bytes()).to_hex());

    if let Ok(existing) = store.get_file(&rel_path_str) {
        if existing.hash == hash {
            stats.files_skipped += 1;
            return Ok(());
        }
        cleanup_generic_file_entities(store, &rel_path_str)?;
    }

    store.insert_file(&FileRecord {
        path: rel_path_str.clone(),
        component_id: None,
        kind: "docker".to_string(),
        hash,
        indexed: true,
        ignore_reason: None,
    })?;

    // Create Deployable entity
    let deployable_id = id::deployable_id(&rel_path_str);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("deployable").to_string();
    
    store.insert_entity(&Entity {
        id: deployable_id.clone(),
        kind: EntityKind::Deployable,
        name,
        component_id: None,
        path: Some(rel_path_str.clone()),
        language: Some("dockerfile".to_string()),
        line_start: None,
        line_end: None,
        visibility: Some("pub".to_string()),
        exported: true,
    })?;
    stats.deployables_indexed += 1;

    Ok(())
}

fn create_infra_root(
    store: &Store,
    dir: &Path,
    project_root: &Path,
    stats: &mut IndexStats,
) -> Result<(), IndexError> {
    let rel_dir = dir.strip_prefix(project_root).unwrap_or(dir);
    let rel_dir_str = rel_dir.display().to_string();
    let infra_id = id::infra_root_id(&rel_dir_str);
    
    // Check if already exists
    if store.get_entity(&infra_id).is_ok() {
        return Ok(());
    }

    let name = if rel_dir_str.is_empty() {
        "root".to_string()
    } else {
        rel_dir_str.clone()
    };

    store.insert_entity(&Entity {
        id: infra_id.clone(),
        kind: EntityKind::InfraRoot,
        name,
        component_id: None,
        path: Some(rel_dir_str.clone()),
        language: Some("terraform".to_string()),
        line_start: None,
        line_end: None,
        visibility: Some("pub".to_string()),
        exported: true,
    })?;
    stats.infra_roots_indexed += 1;

    // Note: Deployables are independent entities.
    // Deploys edges should be created explicitly via configuration or detected
    // through actual references in terraform code (e.g., image URIs).

    Ok(())
}

fn find_symbol_by_name(store: &Store, name: &str) -> Option<String> {
    // Simple name-based lookup - find any symbol with matching name
    if let Ok(entities) = store.list_entities() {
        for entity in entities {
            if entity.kind == EntityKind::Symbol && entity.name == name {
                return Some(entity.id);
            }
        }
    }
    None
}

fn cleanup_generic_file_entities(store: &Store, rel_path: &str) -> Result<(), IndexError> {
    // Delete all entities associated with this file path
    if let Ok(entities) = store.list_entities() {
        for entity in entities {
            if entity.path.as_deref() == Some(rel_path) {
                let _ = store.delete_entity(&entity.id);
            }
        }
    }
    Ok(())
}

fn cleanup_generic_deleted_files(
    store: &Store,
    indexed_files: &HashSet<String>,
    stats: &mut IndexStats,
) -> Result<(), IndexError> {
    let stored_files = store.list_files(None)?;
    for file in &stored_files {
        if !indexed_files.contains(&file.path) {
            cleanup_generic_file_entities(store, &file.path)?;
            let _ = store.delete_file(&file.path);
            stats.files_removed += 1;
        }
    }
    Ok(())
}

/// Extract title from TOML (+++) or YAML (---) frontmatter.
fn parse_frontmatter_title(content: &str) -> Option<String> {
    if let Some(rest) = content.strip_prefix("+++") {
        // TOML frontmatter
        let end = rest.find("+++")?;
        let fm = &rest[..end];
        for line in fm.lines() {
            let trimmed = line.trim();
            if let Some(val) = trimmed.strip_prefix("title") {
                let val = val.trim_start().strip_prefix('=')?.trim();
                return Some(val.trim_matches('"').to_string());
            }
        }
    } else if let Some(rest) = content.strip_prefix("---") {
        // YAML frontmatter
        let end = rest.find("---")?;
        let fm = &rest[..end];
        for line in fm.lines() {
            let trimmed = line.trim();
            if let Some(val) = trimmed.strip_prefix("title") {
                let val = val.trim_start().strip_prefix(':')?.trim();
                return Some(val.trim_matches('"').to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_project_finds_files() {
        let store = Store::open_in_memory().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();

        let stats = index_project(&store, root).unwrap();

        // Should find source files
        assert!(stats.files_indexed > 0, "expected some files");
        assert!(stats.symbols_extracted > 0, "expected some symbols");
        assert!(stats.edges_created > 0, "expected some edges");
        assert_eq!(stats.files_skipped, 0, "first run should skip nothing");

        // Verify entities exist
        let all_entities = store.list_entities().unwrap();
        let source_units: Vec<_> = all_entities
            .iter()
            .filter(|e| e.kind == EntityKind::SourceUnit)
            .collect();
        assert!(!source_units.is_empty(), "should have source units");

        let symbols: Vec<_> = all_entities
            .iter()
            .filter(|e| e.kind == EntityKind::Symbol)
            .collect();
        assert!(!symbols.is_empty(), "should have symbols");

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
        let files_after_first = store.stats().unwrap().files;

        // Second run: should skip most files
        let stats2 = index_project(&store, root).unwrap();
        assert_eq!(stats2.files_indexed, 0, "no files should be re-indexed");
        assert!(stats2.files_skipped > 0, "some files should be skipped");

        // Entity and file counts should remain the same
        let entities_after_second = store.stats().unwrap().entities;
        let files_after_second = store.stats().unwrap().files;
        assert_eq!(entities_after_first, entities_after_second);
        assert_eq!(files_after_first, files_after_second);
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

    #[test]
    fn index_creates_doc_entities() {
        let store = Store::open_in_memory().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();

        let stats = index_project(&store, root).unwrap();

        // The workspace root should have at least a README.md
        if root.join("README.md").exists() {
            assert!(stats.docs_indexed > 0, "should index at least README.md");
        }
    }

    #[test]
    fn parse_toml_frontmatter_title() {
        let content = "+++\ntitle = \"My Post\"\ndate = 2024-01-01\n+++\n\nBody here.";
        assert_eq!(
            parse_frontmatter_title(content),
            Some("My Post".to_string())
        );
    }

    #[test]
    fn parse_yaml_frontmatter_title() {
        let content = "---\ntitle: \"Hello World\"\ndate: 2024-01-01\n---\n\nBody here.";
        assert_eq!(
            parse_frontmatter_title(content),
            Some("Hello World".to_string())
        );
    }

    #[test]
    fn parse_yaml_frontmatter_unquoted_title() {
        let content = "---\ntitle: Hello World\ndate: 2024-01-01\n---\n\nBody.";
        assert_eq!(
            parse_frontmatter_title(content),
            Some("Hello World".to_string())
        );
    }

    #[test]
    fn parse_frontmatter_no_frontmatter() {
        assert_eq!(parse_frontmatter_title("Just plain text"), None);
    }

    #[test]
    fn parse_frontmatter_no_title() {
        let content = "+++\ndate = 2024-01-01\n+++\n\nBody.";
        assert_eq!(parse_frontmatter_title(content), None);
    }
}
