use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use grakno_core::model::{Edge, EdgeKind, Entity, EntityKind, FileRecord};
use grakno_core::Store;

use crate::error::IndexError;
use crate::id;
use crate::markdown::extract_mentions;
use crate::mise::parse_mise_toml;
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
    pub containerized_indexed: usize,
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
        writeln!(f, "containerized: {}", self.containerized_indexed)?;
        writeln!(f, "commands:      {}", self.commands_indexed)?;
        writeln!(f, "sites:         {}", self.sites_detected)?;
        writeln!(f, "content_pages: {}", self.content_pages_indexed)?;
        writeln!(f, "routes:        {}", self.task_routes_generated)?;
        write!(f, "edges:         {}", self.edges_created)
    }
}

/// Track an image reference found in a terraform file.
#[derive(Debug, Clone)]
struct ImageRef {
    infra_dir: String,
    image_name: String,
    line: i64,
}

/// Index a project by walking the directory and parsing supported files.
/// No assumptions about project structure - works with any codebase.
#[tracing::instrument(skip(store), fields(path = %path.display()))]

pub fn index_project(store: &Store, path: &Path) -> Result<IndexStats, IndexError> {
    tracing::info!("starting generic project indexing");
    let start = std::time::Instant::now();

    store.begin_transaction().map_err(|e| {
        IndexError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            e.to_string(),
        ))
    })?;

    let result = index_project_inner(store, path);

    match &result {
        Ok(_) => {
            store.commit_transaction().map_err(|e| {
                IndexError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;
        }
        Err(_) => {
            let _ = store.rollback_transaction();
        }
    }

    let stats = result?;

    tracing::info!(
        duration_ms = start.elapsed().as_millis() as u64,
        files = stats.files_indexed,
        symbols = stats.symbols_extracted,
        edges = stats.edges_created,
        "generic indexing complete"
    );

    Ok(stats)
}

fn index_project_inner(store: &Store, path: &Path) -> Result<IndexStats, IndexError> {
    let mut stats = IndexStats::default();
    let mut indexed_files = HashSet::new();
    let mut image_refs: Vec<ImageRef> = Vec::new();

    index_generic_walk(
        store,
        path,
        path,
        &mut stats,
        &mut indexed_files,
        &mut image_refs,
    )?;

    // Create Repo entity unconditionally
    let project_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let repo_entity_id = id::repo_id(project_name);
    store.insert_entity(&Entity {
        id: repo_entity_id.clone(),
        kind: EntityKind::Repo,
        name: project_name.to_string(),
        component_id: None,
        path: None,
        language: None,
        line_start: None,
        line_end: None,
        visibility: None,
        exported: true,
    })?;

    // Parse mise.toml and emit Task entities + OwnsTask edges
    if let Some(config) = parse_mise_toml(path)? {
        for task in &config.tasks {
            let task_entity_id = id::task_id(&task.name);
            store.insert_entity(&Entity {
                id: task_entity_id.clone(),
                kind: EntityKind::Task,
                name: task.name.clone(),
                component_id: None,
                path: Some("mise.toml".to_string()),
                language: None,
                line_start: None,
                line_end: None,
                visibility: None,
                exported: true,
            })?;

            store.insert_edge(&Edge {
                src_id: repo_entity_id.clone(),
                rel: EdgeKind::OwnsTask,
                dst_id: task_entity_id,
                provenance_path: Some("mise.toml".to_string()),
                provenance_line: None,
            })?;

            stats.tasks_extracted += 1;
            stats.edges_created += 1;
        }
    }

    create_deploys_edges(store, &image_refs, path, &mut stats)?;
    cleanup_generic_deleted_files(store, &indexed_files, &mut stats)?;

    Ok(stats)
}

fn index_generic_walk(
    store: &Store,
    dir: &Path,
    project_root: &Path,
    stats: &mut IndexStats,
    indexed_files: &mut HashSet<String>,
    image_refs: &mut Vec<ImageRef>,
) -> Result<(), IndexError> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.path());

    // Track if this directory has terraform files for InfraRoot creation
    let mut has_main_tf = false;
    let dir_rel_path = dir.strip_prefix(project_root).unwrap_or(dir);
    let dir_rel_str = dir_rel_path.display().to_string();

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
            if file_name.starts_with('.')
                || matches!(
                    file_name,
                    "target"
                        | "node_modules"
                        | "dist"
                        | "build"
                        | "out"
                        | "coverage"
                        | "__pycache__"
                )
            {
                continue;
            }
            index_generic_walk(store, &path, project_root, stats, indexed_files, image_refs)?;
        } else {
            // Index supported file types directly
            let rel_path = path.strip_prefix(project_root).unwrap_or(&path);
            let _rel_path_str = rel_path.display().to_string();

            // Check file extension for supported languages
            let ext = path.extension().and_then(|e| e.to_str());
            match ext {
                Some("rs") => {
                    index_generic_source_file(
                        store,
                        &path,
                        project_root,
                        "rust",
                        stats,
                        indexed_files,
                    )?;
                }
                Some("ts") | Some("tsx") => {
                    index_generic_source_file(
                        store,
                        &path,
                        project_root,
                        "typescript",
                        stats,
                        indexed_files,
                    )?;
                }
                Some("astro") => {
                    index_generic_source_file(
                        store,
                        &path,
                        project_root,
                        "astro",
                        stats,
                        indexed_files,
                    )?;
                }
                Some("md") => {
                    index_generic_doc_file(store, &path, project_root, stats, indexed_files)?;
                }
                Some("tf") | Some("hcl") => {
                    index_terraform_file(
                        store,
                        &path,
                        project_root,
                        stats,
                        indexed_files,
                        image_refs,
                        &dir_rel_str,
                    )?;
                }
                _ => {
                    // Check for Dockerfile patterns
                    if file_name.contains("Dockerfile")
                        || (file_name.starts_with("docker-compose")
                            && (file_name.ends_with(".yml") || file_name.ends_with(".yaml")))
                    {
                        index_containerized_file(store, &path, project_root, stats, indexed_files)?;
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

/// Resolve a TypeScript relative import path to a project-relative file path.
///
/// Returns `None` for bare/package imports (those not starting with `./` or `../`).
/// Tries the following extensions in order: "", ".ts", ".tsx", ".js", ".jsx",
/// "/index.ts", "/index.tsx".
fn resolve_ts_import(
    import_path: &str,
    importing_file_rel: &str,
    project_root: &Path,
) -> Option<String> {
    // Skip bare/package imports
    if !import_path.starts_with("./") && !import_path.starts_with("../") {
        return None;
    }

    // Get the directory of the importing file (project-relative)
    let importing_dir = Path::new(importing_file_rel)
        .parent()
        .unwrap_or(Path::new(""));

    // Resolve the relative import path against the importing file's directory
    let resolved = importing_dir.join(import_path);

    // Normalize the path (collapse .. and .)
    let normalized = normalize_path(&resolved);

    // Extensions to probe
    let probes: &[&str] = &["", ".ts", ".tsx", ".js", ".jsx", "/index.ts", "/index.tsx"];

    let normalized_str = normalized.display().to_string();

    for ext in probes {
        let candidate = format!("{normalized_str}{ext}");
        let abs_candidate = project_root.join(&candidate);
        if abs_candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

/// Normalize a path by collapsing `.` and `..` components without touching the filesystem.
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {} // skip "."
            std::path::Component::ParentDir => {
                // Pop last component unless we're at root
                if !components.is_empty() {
                    components.pop();
                }
            }
            other => components.push(other),
        }
    }
    components.iter().collect()
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
        name: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&rel_path_str)
            .to_string(),
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
                            dst_id: entity_id.clone(),
                            provenance_path: Some(rel_path_str.clone()),
                            provenance_line: Some(sym.line_start as i64),
                        })?;
                        stats.edges_created += 1;
                    }

                    // BenchmarkedBy: SourceUnit → BenchmarkedBy → Bench entity
                    if entity_kind == EntityKind::Bench {
                        store.insert_edge(&Edge {
                            src_id: su_id.clone(),
                            rel: EdgeKind::BenchmarkedBy,
                            dst_id: entity_id.clone(),
                            provenance_path: Some(rel_path_str.clone()),
                            provenance_line: Some(sym.line_start as i64),
                        })?;
                        stats.edges_created += 1;
                    }

                    // Implements: impl entity → Implements → trait entity
                    if let Some(ref trait_name) = sym.trait_name {
                        let trait_entity_id = id::symbol_in_file(&rel_path_str, trait_name);
                        store.insert_edge(&Edge {
                            src_id: entity_id,
                            rel: EdgeKind::Implements,
                            dst_id: trait_entity_id,
                            provenance_path: Some(rel_path_str.clone()),
                            provenance_line: Some(sym.line_start as i64),
                        })?;
                        stats.edges_created += 1;
                    }
                }

                // Reexports: SourceUnit → Reexports → reexported symbol entity
                for reexport in &parse_result.uses {
                    let reexport_entity_id = id::symbol_in_file(&rel_path_str, &reexport.path);
                    store.insert_entity(&Entity {
                        id: reexport_entity_id.clone(),
                        kind: EntityKind::Symbol,
                        name: reexport.path.clone(),
                        component_id: None,
                        path: Some(rel_path_str.clone()),
                        language: Some(language.to_string()),
                        line_start: Some(reexport.line as i64),
                        line_end: Some(reexport.line as i64),
                        visibility: Some(reexport.visibility.clone()),
                        exported: reexport.visibility == "pub",
                    })?;
                    store.insert_edge(&Edge {
                        src_id: su_id.clone(),
                        rel: EdgeKind::Reexports,
                        dst_id: reexport_entity_id,
                        provenance_path: Some(rel_path_str.clone()),
                        provenance_line: Some(reexport.line as i64),
                    })?;
                    stats.edges_created += 1;
                    stats.symbols_extracted += 1;
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
                        visibility: if sym.exported {
                            Some("pub".to_string())
                        } else {
                            None
                        },
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

                // Emit DependsOn edges from imports
                for imp in &parse_result.imports {
                    if let Some(target_rel) =
                        resolve_ts_import(&imp.path, &rel_path_str, project_root)
                    {
                        let target_id = id::file_entity_id(&target_rel);
                        store.insert_edge(&Edge {
                            src_id: su_id.clone(),
                            rel: EdgeKind::DependsOn,
                            dst_id: target_id,
                            provenance_path: Some(rel_path_str.clone()),
                            provenance_line: Some(imp.line as i64),
                        })?;
                        stats.edges_created += 1;
                    }
                }

                // Emit Reexports edges from re-export paths
                for reexport_path in &parse_result.exports {
                    if let Some(target_rel) =
                        resolve_ts_import(reexport_path, &rel_path_str, project_root)
                    {
                        let target_id = id::file_entity_id(&target_rel);
                        store.insert_edge(&Edge {
                            src_id: su_id.clone(),
                            rel: EdgeKind::Reexports,
                            dst_id: target_id,
                            provenance_path: Some(rel_path_str.clone()),
                            provenance_line: None,
                        })?;
                        stats.edges_created += 1;
                    }
                }
            }
        }
        "astro" => {
            if let Ok(_parse_result) = parse_astro_file(&source) {
                // Create entity for the component itself
                let comp_name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("component");
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
        .or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
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
    image_refs: &mut Vec<ImageRef>,
    infra_dir: &str,
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
        name: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&rel_path_str)
            .to_string(),
        component_id: None,
        path: Some(rel_path_str.clone()),
        language: Some("hcl".to_string()),
        line_start: None,
        line_end: None,
        visibility: None,
        exported: false,
    })?;
    stats.files_indexed += 1;

    // Extract terraform resource names as symbols and image references
    let mut line_num = 0;
    for line in content.lines() {
        line_num += 1;
        let trimmed = line.trim();

        // Look for image references
        // Common patterns: image = "...", image_uri = "...", container_image = "..."
        if let Some(image) = extract_image_from_line(trimmed) {
            image_refs.push(ImageRef {
                infra_dir: infra_dir.to_string(),
                image_name: image,
                line: line_num,
            });
        }

        if trimmed.starts_with("resource") {
            // resource "aws_ecs_service" "api" { ... }
            if let Some(open) = trimmed.find('"') {
                if let Some(close) = trimmed[open + 1..].find('"') {
                    let resource_type = &trimmed[open + 1..open + 1 + close];
                    if let Some(name_open) = trimmed[open + 1 + close + 1..].find('"') {
                        let name_start = open + 1 + close + 1 + name_open + 1;
                        if let Some(name_close) = trimmed[name_start..].find('"') {
                            let resource_name = &trimmed[name_start..name_start + name_close];
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

/// Extract image reference from a terraform line.
/// Looks for patterns like: image = "...", image_uri = "...", container_image = "..."
fn extract_image_from_line(line: &str) -> Option<String> {
    // Check if line contains image-related key
    let image_keys = ["image", "image_uri", "container_image", "docker_image"];
    let has_image_key = image_keys
        .iter()
        .any(|k| line.contains(&format!("{} =", k)) || line.contains(&format!("{}=", k)));

    if !has_image_key {
        return None;
    }

    // Extract the value after =
    if let Some(eq_pos) = line.find('=') {
        let after_eq = &line[eq_pos + 1..].trim();

        // Try to extract quoted string
        if let Some(open) = after_eq.find('"') {
            let after_open = &after_eq[open + 1..];
            if let Some(close) = after_open.find('"') {
                let image = &after_open[..close];
                // Filter out variable references and empty strings
                if !image.is_empty() && !image.starts_with("$") && !image.starts_with("var.") {
                    return Some(image.to_string());
                }
            }
        }
    }

    None
}

/// Create Deploys edges from InfraRoot to Containerized based on image references.
fn create_deploys_edges(
    store: &Store,
    image_refs: &[ImageRef],
    _project_root: &Path,
    stats: &mut IndexStats,
) -> Result<(), IndexError> {
    // Get all containerized entities
    let containerized: Vec<_> = store
        .list_entities()?
        .into_iter()
        .filter(|e| e.kind == EntityKind::Containerized)
        .collect();

    if containerized.is_empty() {
        return Ok(());
    }

    for image_ref in image_refs {
        let infra_id = id::infra_root_id(&image_ref.infra_dir);

        // Try to match image reference to a containerized entity
        // Matching strategy: look for directory name in image path
        // e.g., image "myapp:latest" matches Dockerfile in "myapp/" directory
        let image_base = image_ref
            .image_name
            .split(':')
            .next()
            .unwrap_or(&image_ref.image_name);

        // Also try matching by last path component
        let image_name_only = image_base.split('/').last().unwrap_or(image_base);

        for container in &containerized {
            if let Some(path) = &container.path {
                // Get the directory containing the Dockerfile
                let container_dir = std::path::Path::new(path)
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or("");

                let container_file = std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");

                // Match if image name contains the directory name or vice versa
                let matches = !container_dir.is_empty()
                    && (image_base.contains(container_dir)
                        || container_dir.contains(image_name_only)
                        || image_name_only.contains(container_dir));

                // Also match docker-compose services
                let is_compose_match = container_file.starts_with("docker-compose")
                    && image_ref.image_name.contains("compose");

                if matches || is_compose_match {
                    // Create Deploys edge: InfraRoot -> Containerized
                    store.insert_edge(&Edge {
                        src_id: infra_id.clone(),
                        rel: EdgeKind::Deploys,
                        dst_id: container.id.clone(),
                        provenance_path: Some(format!("{}/main.tf", image_ref.infra_dir)),
                        provenance_line: Some(image_ref.line),
                    })?;
                    stats.edges_created += 1;

                    tracing::debug!(
                        infra = %infra_id,
                        container = %container.id,
                        image = %image_ref.image_name,
                        "created deploys edge"
                    );

                    // Only create one edge per image ref
                    break;
                }
            }
        }
    }

    Ok(())
}

fn index_containerized_file(
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

    // Create Containerized entity
    let containerized_id = id::containerized_id(&rel_path_str);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("container")
        .to_string();

    store.insert_entity(&Entity {
        id: containerized_id.clone(),
        kind: EntityKind::Containerized,
        name,
        component_id: None,
        path: Some(rel_path_str.clone()),
        language: Some("dockerfile".to_string()),
        line_start: None,
        line_end: None,
        visibility: Some("pub".to_string()),
        exported: true,
    })?;
    stats.containerized_indexed += 1;

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

    // Note: Containerized entities are independent from Infra.
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

    #[test]
    fn index_project_emits_repo_and_mise_tasks() {
        use std::fs;

        // Create a temp directory with a unique name
        let dir = std::env::temp_dir().join("grakno_test_mise_indexer");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // Write a mise.toml with two tasks
        fs::write(
            dir.join("mise.toml"),
            "[tasks]\nbuild = \"cargo build\"\ntest = \"cargo test\"\n",
        )
        .unwrap();

        // Write a dummy .rs file so the indexer has something to walk
        fs::write(dir.join("dummy.rs"), "fn main() {}\n").unwrap();

        let store = Store::open_in_memory().unwrap();
        let stats = index_project(&store, &dir).unwrap();

        // Verify task stats
        assert_eq!(stats.tasks_extracted, 2, "should extract 2 tasks");

        let all_entities = store.list_entities().unwrap();

        // Verify Repo entity exists
        let repos: Vec<_> = all_entities
            .iter()
            .filter(|e| e.kind == EntityKind::Repo)
            .collect();
        assert_eq!(repos.len(), 1, "should have exactly one Repo entity");

        // Verify Task entities exist
        let tasks: Vec<_> = all_entities
            .iter()
            .filter(|e| e.kind == EntityKind::Task)
            .collect();
        assert_eq!(tasks.len(), 2, "should have 2 Task entities");
        let task_names: Vec<&str> = tasks.iter().map(|t| t.name.as_str()).collect();
        assert!(task_names.contains(&"build"), "should have build task");
        assert!(task_names.contains(&"test"), "should have test task");

        // Verify OwnsTask edges exist (query from the repo entity)
        let repo_edges = store.edges_from(&repos[0].id).unwrap();
        let owns_task_edges: Vec<_> = repo_edges
            .iter()
            .filter(|e| e.rel == EdgeKind::OwnsTask)
            .collect();
        assert_eq!(owns_task_edges.len(), 2, "should have 2 OwnsTask edges");
        for edge in &owns_task_edges {
            assert_eq!(edge.src_id, repos[0].id, "OwnsTask src should be the Repo");
            assert_eq!(
                edge.provenance_path.as_deref(),
                Some("mise.toml"),
                "provenance_path should be mise.toml"
            );
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn index_project_creates_repo_without_mise_toml() {
        use std::fs;

        let dir = std::env::temp_dir().join("grakno_test_no_mise_indexer");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // Write only a dummy .rs file, no mise.toml
        fs::write(dir.join("dummy.rs"), "fn main() {}\n").unwrap();

        let store = Store::open_in_memory().unwrap();
        let stats = index_project(&store, &dir).unwrap();

        // Should still create a Repo entity
        let all_entities = store.list_entities().unwrap();
        let repos: Vec<_> = all_entities
            .iter()
            .filter(|e| e.kind == EntityKind::Repo)
            .collect();
        assert_eq!(repos.len(), 1, "should have Repo even without mise.toml");

        // But no tasks
        assert_eq!(stats.tasks_extracted, 0, "no tasks without mise.toml");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn index_creates_benchmarked_by_edges() {
        let store = Store::open_in_memory().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let rs_path = tmp.path().join("bench_example.rs");
        std::fs::write(&rs_path, "#[bench]\nfn bench_foo() {}\n").unwrap();

        let mut stats = IndexStats::default();
        let mut indexed = HashSet::new();
        index_generic_source_file(
            &store,
            &rs_path,
            tmp.path(),
            "rust",
            &mut stats,
            &mut indexed,
        )
        .unwrap();

        let su_id = id::file_entity_id("bench_example.rs");
        let edges: Vec<_> = store
            .edges_from(&su_id)
            .unwrap()
            .into_iter()
            .filter(|e| e.rel == EdgeKind::BenchmarkedBy)
            .collect();
        assert_eq!(edges.len(), 1, "expected one BenchmarkedBy edge");
        assert!(
            edges[0].dst_id.contains("bench_foo"),
            "dst should reference bench_foo"
        );
    }

    #[test]
    fn index_creates_implements_edges() {
        let store = Store::open_in_memory().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let rs_path = tmp.path().join("impl_example.rs");
        std::fs::write(
            &rs_path,
            "pub trait Greet {\n    fn greet(&self);\n}\n\nimpl Greet for MyStruct {\n    fn greet(&self) {}\n}\n",
        )
        .unwrap();

        let mut stats = IndexStats::default();
        let mut indexed = HashSet::new();
        index_generic_source_file(
            &store,
            &rs_path,
            tmp.path(),
            "rust",
            &mut stats,
            &mut indexed,
        )
        .unwrap();

        // The impl entity ID is based on its display name "impl Greet for MyStruct"
        let impl_entity_id = id::symbol_in_file("impl_example.rs", "impl Greet for MyStruct");
        let edges: Vec<_> = store
            .edges_from(&impl_entity_id)
            .unwrap()
            .into_iter()
            .filter(|e| e.rel == EdgeKind::Implements)
            .collect();
        assert_eq!(edges.len(), 1, "expected one Implements edge");
        // src is the impl entity, dst is the trait entity
        assert!(
            edges[0].src_id.contains("impl Greet for MyStruct"),
            "src should be the impl entity"
        );
        assert!(
            edges[0].dst_id.contains("Greet"),
            "dst should reference the trait"
        );
    }

    #[test]
    fn index_creates_reexports_edges() {
        let store = Store::open_in_memory().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let rs_path = tmp.path().join("reexport_example.rs");
        std::fs::write(&rs_path, "pub use crate::foo::Bar;\n").unwrap();

        let mut stats = IndexStats::default();
        let mut indexed = HashSet::new();
        index_generic_source_file(
            &store,
            &rs_path,
            tmp.path(),
            "rust",
            &mut stats,
            &mut indexed,
        )
        .unwrap();

        let su_id = id::file_entity_id("reexport_example.rs");
        let edges: Vec<_> = store
            .edges_from(&su_id)
            .unwrap()
            .into_iter()
            .filter(|e| e.rel == EdgeKind::Reexports)
            .collect();
        assert_eq!(edges.len(), 1, "expected one Reexports edge");
        assert!(
            edges[0].dst_id.contains("crate::foo::Bar"),
            "dst should reference the reexported path"
        );

        // Verify the reexport symbol entity was created
        let entities: Vec<_> = store
            .list_entities()
            .unwrap()
            .into_iter()
            .filter(|e| e.name == "crate::foo::Bar")
            .collect();
        assert_eq!(entities.len(), 1, "expected reexport symbol entity");
    }

    #[test]
    fn resolve_ts_import_skips_bare_imports() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(resolve_ts_import("react", "src/app.ts", tmp.path()), None);
        assert_eq!(
            resolve_ts_import("@scope/pkg", "src/app.ts", tmp.path()),
            None
        );
    }

    #[test]
    fn resolve_ts_import_finds_relative_ts() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("utils.ts"), "export const x = 1;").unwrap();

        let result = resolve_ts_import("./utils", "src/app.ts", tmp.path());
        assert_eq!(result, Some("src/utils.ts".to_string()));
    }

    #[test]
    fn resolve_ts_import_finds_exact_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("helper.tsx"), "export const x = 1;").unwrap();

        let result = resolve_ts_import("./helper", "src/app.ts", tmp.path());
        assert_eq!(result, Some("src/helper.tsx".to_string()));
    }

    #[test]
    fn resolve_ts_import_finds_index_ts() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("src").join("components");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("index.ts"), "export {};").unwrap();

        let result = resolve_ts_import("./components", "src/app.ts", tmp.path());
        assert_eq!(result, Some("src/components/index.ts".to_string()));
    }

    #[test]
    fn resolve_ts_import_parent_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        let lib_dir = tmp.path().join("lib");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&lib_dir).unwrap();
        std::fs::write(lib_dir.join("shared.ts"), "export {};").unwrap();

        let result = resolve_ts_import("../lib/shared", "src/app.ts", tmp.path());
        assert_eq!(result, Some("lib/shared.ts".to_string()));
    }

    #[test]
    fn resolve_ts_import_returns_none_for_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();

        let result = resolve_ts_import("./nonexistent", "src/app.ts", tmp.path());
        assert_eq!(result, None);
    }

    #[test]
    fn resolve_ts_import_with_explicit_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("data.js"), "export {};").unwrap();

        // When the import already has an extension, the "" probe matches
        let result = resolve_ts_import("./data.js", "src/app.ts", tmp.path());
        assert_eq!(result, Some("src/data.js".to_string()));
    }

    #[test]
    fn ts_import_emits_depends_on_edges() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();

        // Create two TS files: app.ts imports utils.ts
        std::fs::write(
            src_dir.join("utils.ts"),
            "export function helper() { return 42; }\n",
        )
        .unwrap();
        std::fs::write(
            src_dir.join("app.ts"),
            "import { helper } from './utils';\nconsole.log(helper());\n",
        )
        .unwrap();

        let store = Store::open_in_memory().unwrap();
        let stats = index_project(&store, tmp.path()).unwrap();

        // Both files should be indexed
        assert_eq!(stats.files_indexed, 2);

        // Should have a DependsOn edge from app.ts -> utils.ts
        let app_su_id = id::file_entity_id("src/app.ts");
        let edges = store.edges_from(&app_su_id).unwrap();
        let depends_on: Vec<_> = edges
            .iter()
            .filter(|e| e.rel == EdgeKind::DependsOn)
            .collect();

        assert_eq!(
            depends_on.len(),
            1,
            "expected exactly one DependsOn edge, got: {:?}",
            depends_on
        );
        assert_eq!(depends_on[0].src_id, app_su_id);
        assert_eq!(depends_on[0].dst_id, id::file_entity_id("src/utils.ts"));
    }

    #[test]
    fn ts_reexport_emits_reexports_edge() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();

        // Create a module and an index that re-exports it
        std::fs::write(
            src_dir.join("types.ts"),
            "export interface User { name: string; }\n",
        )
        .unwrap();
        std::fs::write(
            src_dir.join("index.ts"),
            "export { User } from './types';\n",
        )
        .unwrap();

        let store = Store::open_in_memory().unwrap();
        let stats = index_project(&store, tmp.path()).unwrap();

        assert_eq!(stats.files_indexed, 2);

        // index.ts should have a Reexports edge to types.ts
        let index_su_id = id::file_entity_id("src/index.ts");
        let edges = store.edges_from(&index_su_id).unwrap();

        let reexports: Vec<_> = edges
            .iter()
            .filter(|e| e.rel == EdgeKind::Reexports)
            .collect();

        assert_eq!(
            reexports.len(),
            1,
            "expected exactly one Reexports edge, got: {:?}",
            reexports
        );
        assert_eq!(reexports[0].src_id, index_su_id);
        assert_eq!(reexports[0].dst_id, id::file_entity_id("src/types.ts"));

        // index.ts should also have a DependsOn edge (re-exports also show up in imports)
        let depends_on: Vec<_> = edges
            .iter()
            .filter(|e| e.rel == EdgeKind::DependsOn)
            .collect();
        assert_eq!(
            depends_on.len(),
            1,
            "expected exactly one DependsOn edge from re-export import, got: {:?}",
            depends_on
        );
    }

    #[test]
    fn ts_bare_import_not_emitted() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();

        // File with only bare/package imports
        std::fs::write(
            src_dir.join("app.ts"),
            "import React from 'react';\nimport { z } from 'zod';\n",
        )
        .unwrap();

        let store = Store::open_in_memory().unwrap();
        let _stats = index_project(&store, tmp.path()).unwrap();

        let app_su_id = id::file_entity_id("src/app.ts");
        let edges = store.edges_from(&app_su_id).unwrap();
        let depends_on: Vec<_> = edges
            .iter()
            .filter(|e| e.rel == EdgeKind::DependsOn)
            .collect();

        assert!(
            depends_on.is_empty(),
            "bare imports should not produce DependsOn edges"
        );
    }

    #[test]
    fn normalize_path_handles_parent() {
        let p = Path::new("src/sub/../utils.ts");
        assert_eq!(normalize_path(p), PathBuf::from("src/utils.ts"));
    }

    #[test]
    fn normalize_path_handles_current_dir() {
        let p = Path::new("src/./utils.ts");
        assert_eq!(normalize_path(p), PathBuf::from("src/utils.ts"));
    }
}
