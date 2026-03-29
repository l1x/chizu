use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use chizu_core::model::{Edge, EdgeKind, Entity, EntityKind, FileRecord};
use chizu_core::Store;

use crate::error::IndexError;
use crate::id;
use crate::markdown::extract_mentions;
use crate::mise::parse_mise_toml;
use crate::parser::parse_rust_file;
use crate::parser_astro::parse_astro_file;
use crate::parser_package_json::{parse_package_json, resolve_workspaces};
use crate::parser_ts::parse_ts_file;

#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    pub crates_found: usize,
    pub components_found: usize,
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
    pub packages_indexed: usize,
    pub parse_failures: usize,
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
        writeln!(f, "packages:      {}", self.packages_indexed)?;
        writeln!(f, "parse_errors:  {}", self.parse_failures)?;
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

/// Discovered component root information.
/// Used in two-phase indexing: first discover all roots, then assign component IDs.
#[derive(Debug, Clone)]
struct ComponentRoot {
    /// Canonical component ID: component::{ecosystem}::{root_path}
    id: String,
    /// Ecosystem: "cargo", "npm", etc.
    ecosystem: String,
    /// Repo-relative path to component root (e.g., "crates/chizu-core")
    root_path: String,
    /// Display name from manifest (Cargo.toml package.name or package.json name)
    display_name: String,
    /// For npm: workspace globs; for cargo: could be workspace members
    #[allow(dead_code)]
    workspace_globs: Option<Vec<String>>,
}

/// Registry of all component roots in the repo.
/// Maps root_path -> ComponentRoot for lookup by path.
/// Also maps (ecosystem, display_name) -> id for ecosystem-scoped dependency resolution.
#[derive(Debug, Default)]
struct ComponentRegistry {
    /// Sorted by root_path length (descending) for efficient nearest-match lookup
    by_path: BTreeMap<String, ComponentRoot>,
    /// Maps (ecosystem, manifest_display_name) -> canonical_component_id
    /// This is nested to avoid collisions between ecosystems (e.g., npm "utils" vs cargo "utils")
    by_display_name: HashMap<String, HashMap<String, String>>,
}

impl ComponentRegistry {
    /// Find the nearest enclosing component for a file path.
    /// Returns the ComponentRoot if the file is inside a component.
    #[allow(dead_code)]
    fn find_for_file(&self, file_path: &str) -> Option<&ComponentRoot> {
        // Find the longest root_path that is a prefix of file_path
        // Since BTreeMap is sorted, we can use range to find candidates
        let candidates: Vec<_> = self
            .by_path
            .range(..file_path.to_string())
            .filter(|(root_path, _)| {
                // Check if file_path starts with root_path
                file_path.starts_with(*root_path)
                    || (root_path.as_str() == "." && !file_path.starts_with("component::"))
            })
            .map(|(_, comp)| comp)
            .collect();
        
        // Return the one with longest path (most specific match)
        candidates.last().copied()
    }
    
    /// Resolve a manifest display name to a canonical component ID within an ecosystem.
    /// Returns None if the name refers to an external dependency.
    /// 
    /// # Arguments
    /// * `name` - The display name from the manifest (e.g., package.json "name" field)
    /// * `ecosystem` - The ecosystem to resolve within ("npm", "cargo", etc.)
    fn resolve_name(&self, name: &str, ecosystem: &str) -> Option<&String> {
        self.by_display_name
            .get(ecosystem)
            .and_then(|eco_map| eco_map.get(name))
    }
    
    /// Insert a component root into the registry.
    fn insert(&mut self, root: ComponentRoot) {
        // Insert into ecosystem-scoped alias map
        self.by_display_name
            .entry(root.ecosystem.clone())
            .or_default()
            .insert(root.display_name.clone(), root.id.clone());
        
        self.by_path.insert(root.root_path.clone(), root);
    }
    
    /// Get all component IDs.
    #[allow(dead_code)]
    fn all_ids(&self) -> Vec<&String> {
        self.by_path.values().map(|r| &r.id).collect()
    }
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

/// Phase 1: Discover all component roots in the project.
/// Walks the directory tree and identifies all Cargo.toml and package.json roots.
fn discover_component_roots(project_root: &Path) -> Result<ComponentRegistry, IndexError> {
    let mut registry = ComponentRegistry::default();
    discover_component_roots_recursive(project_root, project_root, &mut registry)?;
    Ok(registry)
}

fn discover_component_roots_recursive(
    dir: &Path,
    project_root: &Path,
    registry: &mut ComponentRegistry,
) -> Result<(), IndexError> {
    let entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    
    // Check for manifest files to identify component roots
    let has_cargo_toml = entries.iter().any(|e| {
        e.file_name().to_str() == Some("Cargo.toml")
    });
    let has_package_json = entries.iter().any(|e| {
        e.file_name().to_str() == Some("package.json")
    });
    
    let dir_rel_path = dir.strip_prefix(project_root).unwrap_or(dir);
    let dir_rel_str = if dir_rel_path.as_os_str().is_empty() {
        ".".to_string()
    } else {
        dir_rel_path.display().to_string()
    };
    
    if has_cargo_toml {
        // Parse Cargo.toml to get package name and workspace members
        let cargo_path = dir.join("Cargo.toml");
        if let Ok(content) = std::fs::read_to_string(&cargo_path) {
            if let Ok(manifest) = content.parse::<toml::Table>() {
                let package_name = manifest
                    .get("package")
                    .and_then(|p| p.as_table())
                    .and_then(|p| p.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                
                let canonical_id = id::component_id_from_path("cargo", &dir_rel_str);
                
                // Check if this is a workspace root
                let _is_workspace_root = manifest.get("workspace").is_some();
                
                let workspace_globs = manifest
                    .get("workspace")
                    .and_then(|w| w.as_table())
                    .and_then(|w| w.get("members"))
                    .and_then(|m| m.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    });
                
                registry.insert(ComponentRoot {
                    id: canonical_id,
                    ecosystem: "cargo".to_string(),
                    root_path: dir_rel_str.clone(),
                    display_name: package_name,
                    workspace_globs,
                });
                
                // NOTE: We do NOT skip recursion for Cargo workspace roots.
                // Unlike npm workspaces where members have their own package.json,
                // Cargo workspace members are discovered by recursing into their directories.
                // The early return here was causing all workspace members to be missed.
            }
        }
    }
    
    if has_package_json {
        // Parse package.json to get name and workspaces
        let pkg_path = dir.join("package.json");
        if let Ok(content) = std::fs::read_to_string(&pkg_path) {
            if let Ok(pkg) = parse_package_json(&content) {
                let package_name = pkg.name.clone().unwrap_or_else(|| {
                    dir.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string()
                });
                
                let canonical_id = id::component_id_from_path("npm", &dir_rel_str);
                
                // Convert WorkspacesConfig to Vec<String>
                let workspace_globs = pkg.workspaces.as_ref().map(|ws| match ws {
                    crate::parser_package_json::WorkspacesConfig::Array(arr) => arr.clone(),
                    crate::parser_package_json::WorkspacesConfig::Object { packages } => packages.clone(),
                });
                
                registry.insert(ComponentRoot {
                    id: canonical_id,
                    ecosystem: "npm".to_string(),
                    root_path: dir_rel_str.clone(),
                    display_name: package_name,
                    workspace_globs,
                });
                
                // For workspace roots, workspace members will be discovered separately
                // as their own component roots when we encounter their package.json files
            }
        }
    }
    
    // Recurse into subdirectories
    for entry in &entries {
        let path = entry.path();
        if path.is_dir() {
            // Skip common non-source directories
            let dir_name_owned = entry.file_name().to_string_lossy().into_owned();
            if should_skip_dir(&dir_name_owned) {
                continue;
            }
            discover_component_roots_recursive(&path, project_root, registry)?;
        }
    }
    
    Ok(())
}

/// Check if a directory should be skipped during traversal.
fn should_skip_dir(name: &str) -> bool {
    matches!(name,
        ".git" | ".chizu" | "target" | "node_modules" | "dist" | "build" |
        ".venv" | "venv" | "__pycache__" | ".pytest_cache" | ".mypy_cache" |
        ".idea" | ".vscode" | ".github" | ".ci" | "coverage" | ".next"
    )
}

fn index_project_inner(store: &Store, path: &Path) -> Result<IndexStats, IndexError> {
    let mut stats = IndexStats::default();
    let mut indexed_files = HashSet::new();
    let mut image_refs: Vec<ImageRef> = Vec::new();

    // Phase 1: Discover all component roots
    let component_registry = discover_component_roots(path)?;
    tracing::info!(
        components_found = component_registry.by_path.len(),
        "discovered component roots"
    );
    
    // Update stats
    stats.components_found = component_registry.by_path.len();

    // Create the Repo entity (serves as the root of the containment hierarchy)
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

    // Phase 2: Index all files, using the component registry for ID assignment
    index_generic_walk(
        store,
        path,
        path,
        &repo_entity_id,
        None, // No component at root level
        &component_registry,
        &mut stats,
        &mut indexed_files,
        &mut image_refs,
    )?;

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
    cleanup_orphaned_structural_entities(store, &component_registry, path, &mut stats)?;

    Ok(stats)
}

fn index_generic_walk(
    store: &Store,
    dir: &Path,
    project_root: &Path,
    parent_entity_id: &str,
    current_component_id: Option<String>,
    component_registry: &ComponentRegistry,
    stats: &mut IndexStats,
    indexed_files: &mut HashSet<String>,
    image_refs: &mut Vec<ImageRef>,
) -> Result<(), IndexError> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.path());

    // Track if this directory has terraform files for InfraRoot creation
    let mut has_main_tf = false;
    let dir_rel_path = dir.strip_prefix(project_root).unwrap_or(dir);
    let dir_rel_str = if dir_rel_path.as_os_str().is_empty() {
        ".".to_string()
    } else {
        dir_rel_path.display().to_string()
    };
    
    // Check if this directory is a component root (has Cargo.toml or package.json)
    // Use the component registry for consistent ID assignment
    let has_cargo_toml = entries.iter().any(|e| {
        e.file_name().to_str() == Some("Cargo.toml")
    });
    let has_package_json = entries.iter().any(|e| {
        e.file_name().to_str() == Some("package.json")
    });
    
    // Determine component ID: either this dir is a component root, or use parent's
    let component_id = if has_cargo_toml || has_package_json {
        // This directory is a component root - look it up in the registry
        if let Some(comp_root) = component_registry.by_path.get(&dir_rel_str) {
            // Always insert/overwrite the component entity to reflect manifest renames
            // The ID is canonical (path-based), but the display name comes from the manifest
            store.insert_entity(&Entity {
                id: comp_root.id.clone(),
                kind: EntityKind::Component,
                name: comp_root.display_name.clone(),
                component_id: None,
                path: Some(dir_rel_str.clone()),
                language: Some(comp_root.ecosystem.clone()),
                line_start: None,
                line_end: None,
                visibility: Some("pub".to_string()),
                exported: true,
            })?;
            Some(comp_root.id.clone())
        } else {
            // Fallback: shouldn't happen if discovery is correct
            tracing::warn!(path = %dir_rel_str, "component root not found in registry");
            current_component_id.clone()
        }
    } else {
        // Not a component root - propagate parent's component ID
        current_component_id.clone()
    };

    // Determine the current directory's entity ID.
    // For the project root, the Repo entity serves as the container (no Directory entity).
    // For subdirectories, create a Directory entity and a Contains edge from parent.
    let current_entity_id = if dir != project_root {
        let dir_id = id::dir_entity_id(&dir_rel_str);
        let dir_name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&dir_rel_str)
            .to_string();
        store.insert_entity(&Entity {
            id: dir_id.clone(),
            kind: EntityKind::Directory,
            name: dir_name,
            component_id: None,
            path: Some(dir_rel_str.clone()),
            language: None,
            line_start: None,
            line_end: None,
            visibility: None,
            exported: false,
        })?;

        // Parent → Contains → Directory
        store.insert_edge(&Edge {
            src_id: parent_entity_id.to_string(),
            rel: EdgeKind::Contains,
            dst_id: dir_id.clone(),
            provenance_path: Some(dir_rel_str.clone()),
            provenance_line: None,
        })?;
        stats.edges_created += 1;

        dir_id
    } else {
        parent_entity_id.to_string()
    };

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
            index_generic_walk(
                store,
                &path,
                project_root,
                &current_entity_id,
                component_id.clone(),
                component_registry,
                stats,
                indexed_files,
                image_refs,
            )?;
        } else {
            // Index supported file types directly
            let rel_path = path.strip_prefix(project_root).unwrap_or(&path);
            let rel_path_str = rel_path.display().to_string();

            // Detect package.json by filename before extension matching
            if file_name == "package.json" {
                index_package_json_file(store, &path, project_root, component_registry, stats, indexed_files)?;
                continue;
            }

            // Determine the file entity ID for Contains edges.
            // Each indexing function creates an entity with a deterministic ID.
            let file_entity_id: Option<String>;

            // Check file extension for supported languages
            let ext = path.extension().and_then(|e| e.to_str());
            match ext {
                Some("rs") => {
                    index_generic_source_file(
                        store,
                        &path,
                        project_root,
                        "rust",
                        component_id.clone(),
                        stats,
                        indexed_files,
                    )?;
                    file_entity_id = Some(id::file_entity_id(&rel_path_str));
                }
                Some("ts") | Some("tsx") => {
                    index_generic_source_file(
                        store,
                        &path,
                        project_root,
                        "typescript",
                        component_id.clone(),
                        stats,
                        indexed_files,
                    )?;
                    file_entity_id = Some(id::file_entity_id(&rel_path_str));
                }
                Some("astro") => {
                    index_generic_source_file(
                        store,
                        &path,
                        project_root,
                        "astro",
                        component_id.clone(),
                        stats,
                        indexed_files,
                    )?;
                    file_entity_id = Some(id::file_entity_id(&rel_path_str));
                }
                Some("md") => {
                    index_generic_doc_file(store, &path, project_root, component_id.clone(), stats, indexed_files)?;
                    file_entity_id = Some(id::doc_id("generic", &rel_path_str));
                }
                Some("tf") | Some("hcl") => {
                    index_terraform_file(
                        store,
                        &path,
                        project_root,
                        component_id.clone(),
                        stats,
                        indexed_files,
                        image_refs,
                        &dir_rel_str,
                    )?;
                    file_entity_id = Some(id::file_entity_id(&rel_path_str));
                }
                _ => {
                    // Check for Dockerfile patterns
                    if file_name.contains("Dockerfile")
                        || (file_name.starts_with("docker-compose")
                            && (file_name.ends_with(".yml") || file_name.ends_with(".yaml")))
                    {
                        index_containerized_file(store, &path, project_root, component_id.clone(), stats, indexed_files)?;
                        file_entity_id = Some(id::containerized_id(&rel_path_str));
                    } else {
                        file_entity_id = None;
                    }
                }
            }

            // Emit Contains edge: current directory (or repo) → file entity
            if let Some(ref child_id) = file_entity_id {
                store.insert_edge(&Edge {
                    src_id: current_entity_id.clone(),
                    rel: EdgeKind::Contains,
                    dst_id: child_id.clone(),
                    provenance_path: Some(rel_path_str),
                    provenance_line: None,
                })?;
                stats.edges_created += 1;
            }
        }
    }

    // Create InfraRoot entity if directory has main.tf
    if has_main_tf {
        create_infra_root(store, dir, project_root, component_id.clone(), stats)?;
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
    // Returns None if path goes outside project root
    let normalized = normalize_path(&resolved)?;

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
/// Normalize a path by resolving `.` and `..` components.
/// Returns `None` if the path goes outside the root (e.g., `../../foo` when only one level deep).
fn normalize_path(path: &Path) -> Option<PathBuf> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {} // skip "."
            std::path::Component::ParentDir => {
                // Pop last component, or return None if we're already at root
                if components.pop().is_none() {
                    return None; // Path goes outside root
                }
            }
            other => components.push(other),
        }
    }
    Some(components.iter().collect())
}

fn index_generic_source_file(
    store: &Store,
    path: &Path,
    project_root: &Path,
    language: &str,
    component_id: Option<String>,
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
    // NOTE: We must also verify component_id hasn't changed, which can happen
    // when a manifest is added/removed in a parent directory without modifying
    // the source file itself.
    let needs_reindex = if let Ok(existing) = store.get_file(&rel_path_str) {
        if existing.hash != hash {
            // Content changed - definitely need to reindex
            true
        } else if existing.component_id != component_id {
            // Content same but component assignment changed (e.g., new parent manifest)
            tracing::info!(
                path = %rel_path_str,
                old_component = ?existing.component_id,
                new_component = ?component_id,
                "component assignment changed, reindexing"
            );
            true
        } else {
            // Truly unchanged
            false
        }
    } else {
        // No existing record - need to index
        true
    };
    
    if !needs_reindex {
        stats.files_skipped += 1;
        return Ok(());
    }
    
    // Clean up old entities before reindexing
    // This handles both content changes and component reassignment
    cleanup_generic_file_entities(store, &rel_path_str)?;

    // Insert/update FileRecord
    store.insert_file(&FileRecord {
        path: rel_path_str.clone(),
        component_id: component_id.clone(),
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
        component_id: component_id.clone(),
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
                        component_id: component_id.clone(),
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
                        component_id: component_id.clone(),
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
                    component_id: component_id.clone(),
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
    component_id: Option<String>,
    stats: &mut IndexStats,
    indexed_files: &mut HashSet<String>,
) -> Result<(), IndexError> {
    let content = std::fs::read_to_string(path)?;
    let rel_path = path.strip_prefix(project_root).unwrap_or(path);
    let rel_path_str = rel_path.display().to_string();

    indexed_files.insert(rel_path_str.clone());

    let hash = format!("blake3:{}", blake3::hash(content.as_bytes()).to_hex());

    // Check if unchanged (including component_id)
    let needs_reindex = if let Ok(existing) = store.get_file(&rel_path_str) {
        if existing.hash != hash {
            true
        } else if existing.component_id != component_id {
            tracing::info!(
                path = %rel_path_str,
                old_component = ?existing.component_id,
                new_component = ?component_id,
                "component assignment changed, reindexing"
            );
            true
        } else {
            false
        }
    } else {
        true
    };
    
    if !needs_reindex {
        stats.files_skipped += 1;
        return Ok(());
    }
    
    cleanup_generic_file_entities(store, &rel_path_str)?;

    store.insert_file(&FileRecord {
        path: rel_path_str.clone(),
        component_id: component_id.clone(),
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
        component_id: component_id.clone(),
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
    component_id: Option<String>,
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

    // Check if unchanged (including component_id)
    let needs_reindex = if let Ok(existing) = store.get_file(&rel_path_str) {
        if existing.hash != hash {
            true
        } else if existing.component_id != component_id {
            tracing::info!(
                path = %rel_path_str,
                old_component = ?existing.component_id,
                new_component = ?component_id,
                "component assignment changed, reindexing"
            );
            true
        } else {
            false
        }
    } else {
        true
    };
    
    if !needs_reindex {
        stats.files_skipped += 1;
        return Ok(());
    }
    
    cleanup_generic_file_entities(store, &rel_path_str)?;

    store.insert_file(&FileRecord {
        path: rel_path_str.clone(),
        component_id: component_id.clone(),
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
        component_id: component_id.clone(),
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
                                component_id: component_id.clone(),
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
/// Create Deploys edges from InfraRoot to Containerized entities based on image references.
/// 
/// NOTE: This function clears all existing Deploys edges before creating new ones
/// to ensure the graph stays consistent when Terraform image references change.
fn create_deploys_edges(
    store: &Store,
    image_refs: &[ImageRef],
    _project_root: &Path,
    stats: &mut IndexStats,
) -> Result<(), IndexError> {
    // First, clear all existing Deploys edges to ensure consistency on re-index
    // This prevents stale edges when image references change
    // We find all InfraRoot entities and delete Deploys edges from them
    if let Ok(entities) = store.list_entities() {
        for entity in entities {
            if entity.kind == EntityKind::InfraRoot {
                if let Ok(edges) = store.edges_from(&entity.id) {
                    for edge in edges {
                        if edge.rel == EdgeKind::Deploys {
                            let _ = store.delete_edge(&edge.src_id, edge.rel, &edge.dst_id);
                        }
                    }
                }
            }
        }
    }
    
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
    component_id: Option<String>,
    stats: &mut IndexStats,
    indexed_files: &mut HashSet<String>,
) -> Result<(), IndexError> {
    let content = std::fs::read_to_string(path)?;
    let rel_path = path.strip_prefix(project_root).unwrap_or(path);
    let rel_path_str = rel_path.display().to_string();

    indexed_files.insert(rel_path_str.clone());

    let hash = format!("blake3:{}", blake3::hash(content.as_bytes()).to_hex());

    // Check if unchanged (including component_id)
    let needs_reindex = if let Ok(existing) = store.get_file(&rel_path_str) {
        if existing.hash != hash {
            true
        } else if existing.component_id != component_id {
            tracing::info!(
                path = %rel_path_str,
                old_component = ?existing.component_id,
                new_component = ?component_id,
                "component assignment changed, reindexing"
            );
            true
        } else {
            false
        }
    } else {
        true
    };
    
    if !needs_reindex {
        stats.files_skipped += 1;
        return Ok(());
    }
    
    cleanup_generic_file_entities(store, &rel_path_str)?;

    store.insert_file(&FileRecord {
        path: rel_path_str.clone(),
        component_id: component_id.clone(),
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
        component_id: component_id.clone(),
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
    component_id: Option<String>,
    stats: &mut IndexStats,
) -> Result<(), IndexError> {
    let rel_dir = dir.strip_prefix(project_root).unwrap_or(dir);
    let rel_dir_str = rel_dir.display().to_string();
    let infra_id = id::infra_root_id(&rel_dir_str);

    let name = if rel_dir_str.is_empty() {
        "root".to_string()
    } else {
        rel_dir_str.clone()
    };
    
    // Check if exists with same component_id - skip if unchanged
    if let Ok(existing) = store.get_entity(&infra_id) {
        if existing.component_id == component_id {
            return Ok(());
        }
        // Component assignment changed - will update below
        tracing::info!(
            infra_id = %infra_id,
            old_component = ?existing.component_id,
            new_component = ?component_id,
            "infra_root component assignment changed, updating"
        );
    }

    // Insert or update the InfraRoot entity
    store.insert_entity(&Entity {
        id: infra_id.clone(),
        kind: EntityKind::InfraRoot,
        name,
        component_id: component_id.clone(),
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

/// Index a package.json file.
/// Note: The component entity is created during phase 1 (discovery), not here.
/// This function only creates the file record and dependency edges.
fn index_package_json_file(
    store: &Store,
    path: &Path,
    project_root: &Path,
    component_registry: &ComponentRegistry,
    stats: &mut IndexStats,
    indexed_files: &mut HashSet<String>,
) -> Result<(), IndexError> {
    let content = std::fs::read_to_string(path)?;
    let rel_path = path.strip_prefix(project_root).unwrap_or(path);
    let rel_path_str = rel_path.display().to_string();

    indexed_files.insert(rel_path_str.clone());

    let hash = format!("blake3:{}", blake3::hash(content.as_bytes()).to_hex());

    // Get the directory path for looking up the component
    let pkg_dir_rel = path
        .parent()
        .map(|p| p.strip_prefix(project_root).unwrap_or(p))
        .map(|p| {
            let s = p.display().to_string();
            if s.is_empty() { ".".to_string() } else { s }
        })
        .unwrap_or_else(|| ".".to_string());
    
    // Look up the component in the registry
    let comp_id = component_registry
        .by_path
        .get(&pkg_dir_rel)
        .map(|c| c.id.clone());
    
    // Check if unchanged (including component_id)
    let needs_reindex = if let Ok(existing) = store.get_file(&rel_path_str) {
        if existing.hash != hash {
            true
        } else if existing.component_id != comp_id {
            tracing::info!(
                path = %rel_path_str,
                old_component = ?existing.component_id,
                new_component = ?comp_id,
                "component assignment changed, reindexing"
            );
            true
        } else {
            false
        }
    } else {
        true
    };
    
    if !needs_reindex {
        stats.files_skipped += 1;
        return Ok(());
    }
    
    cleanup_generic_file_entities(store, &rel_path_str)?;
    
    store.insert_file(&FileRecord {
        path: rel_path_str.clone(),
        component_id: comp_id.clone(),
        kind: "package_json".to_string(),
        hash,
        indexed: true,
        ignore_reason: None,
    })?;
    stats.files_indexed += 1;

    let pkg = match parse_package_json(&content) {
        Ok(pkg) => pkg,
        Err(e) => {
            tracing::warn!(path = %rel_path_str, error = %e, "failed to parse package.json");
            stats.parse_failures += 1;
            return Ok(());
        }
    };

    // Emit DependsOn edges for all dependency types
    // Use the registry to resolve local dependencies to canonical IDs
    let all_deps = pkg
        .dependencies
        .keys()
        .chain(pkg.dev_dependencies.keys())
        .chain(pkg.peer_dependencies.keys());

    for dep_name in all_deps {
        // Try to resolve as a local workspace package first (within npm ecosystem)
        let dep_comp_id = if let Some(canonical_id) = component_registry.resolve_name(dep_name, "npm") {
            // Local workspace dependency - use canonical ID
            canonical_id.clone()
        } else {
            // External dependency - create a placeholder external component ID
            // These are kept separate from local components
            format!("external::npm::{}", dep_name)
        };
        
        if let Some(ref src_id) = comp_id {
            store.insert_edge(&Edge {
                src_id: src_id.clone(),
                rel: EdgeKind::DependsOn,
                dst_id: dep_comp_id,
                provenance_path: Some(rel_path_str.clone()),
                provenance_line: None,
            })?;
            stats.edges_created += 1;
        }
    }

    // Handle workspaces: resolve globs and emit Contains edges
    if let Some(ref ws_config) = pkg.workspaces {
        let parent_dir = path.parent().unwrap_or(project_root);
        let workspace_dirs = resolve_workspaces(parent_dir, ws_config);

        for ws_dir in &workspace_dirs {
            // Get the workspace directory relative to project root
            let ws_dir_rel = ws_dir
                .strip_prefix(project_root)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| ".".to_string());
            
            // Look up the workspace component in the registry
            if let Some(ws_comp) = component_registry.by_path.get(&ws_dir_rel) {
                if let Some(ref parent_id) = comp_id {
                    store.insert_edge(&Edge {
                        src_id: parent_id.clone(),
                        rel: EdgeKind::Contains,
                        dst_id: ws_comp.id.clone(),
                        provenance_path: Some(rel_path_str.clone()),
                        provenance_line: None,
                    })?;
                    stats.edges_created += 1;
                }
            }
        }
    }

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
    // Find all entities associated with this file path
    let entities_to_cleanup: Vec<String> = store
        .list_entities()
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.path.as_deref() == Some(rel_path))
        .map(|e| e.id)
        .collect();
    
    for entity_id in entities_to_cleanup {
        // Delete outgoing edges (entity as source)
        if let Ok(edges) = store.edges_from(&entity_id) {
            for edge in edges {
                let _ = store.delete_edge(&edge.src_id, edge.rel, &edge.dst_id);
            }
        }
        
        // Delete incoming edges (entity as destination)  
        if let Ok(edges) = store.edges_to(&entity_id) {
            for edge in edges {
                let _ = store.delete_edge(&edge.src_id, edge.rel, &edge.dst_id);
            }
        }
        
        // Delete task routes for this entity
        if let Ok(routes) = store.routes_for_entity(&entity_id) {
            for route in routes {
                let _ = store.delete_task_route(&route.task_name, &route.entity_id);
            }
        }
        
        // Delete summary for this entity
        let _ = store.delete_summary(&entity_id);
        
        // Delete embedding for this entity
        let _ = store.delete_embedding(&entity_id);
        
        // Finally delete the entity itself
        let _ = store.delete_entity(&entity_id);
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

/// Clean up orphaned structural entities (Components and Directories) after indexing.
/// 
/// This function removes Component and Directory entities that no longer correspond to
/// existing filesystem paths. This happens when:
/// - An entire component directory is deleted
/// - A directory is renamed or moved
/// 
/// The function preserves structural entities that still exist on disk.
fn cleanup_orphaned_structural_entities(
    store: &Store,
    component_registry: &ComponentRegistry,
    project_root: &Path,
    stats: &mut IndexStats,
) -> Result<(), IndexError> {
    // Get all component and directory entities from the store
    let all_entities = store.list_entities().unwrap_or_default();
    
    // Build set of valid component IDs from the registry
    let valid_component_ids: HashSet<&str> = component_registry
        .by_path
        .values()
        .map(|c| c.id.as_str())
        .collect();
    
    for entity in &all_entities {
        match entity.kind {
            EntityKind::Component => {
                // Check if this component ID is in the registry
                if !valid_component_ids.contains(entity.id.as_str()) {
                    tracing::debug!(component_id = %entity.id, "removing orphaned component");
                    
                    // Remove all edges connected to this component
                    if let Ok(edges) = store.edges_from(&entity.id) {
                        for edge in edges {
                            let _ = store.delete_edge(&edge.src_id, edge.rel, &edge.dst_id);
                        }
                    }
                    if let Ok(edges) = store.edges_to(&entity.id) {
                        for edge in edges {
                            let _ = store.delete_edge(&edge.src_id, edge.rel, &edge.dst_id);
                        }
                    }
                    
                    // Remove per-entity metadata (same as file cleanup)
                    // These may not exist for all components, but we clean them up just in case
                    // Delete task routes for this component
                    if let Ok(routes) = store.routes_for_entity(&entity.id) {
                        for route in routes {
                            let _ = store.delete_task_route(&route.task_name, &route.entity_id);
                        }
                    }
                    let _ = store.delete_summary(&entity.id);
                    let _ = store.delete_embedding(&entity.id);
                    
                    // Remove the component entity
                    let _ = store.delete_entity(&entity.id);
                    stats.components_found = stats.components_found.saturating_sub(1);
                }
            }
            EntityKind::Directory | EntityKind::InfraRoot => {
                // Check if the directory still exists on disk
                // Directory and InfraRoot entities store their path in the `path` field
                if let Some(ref dir_path) = entity.path {
                    let full_path = project_root.join(dir_path);
                    if !full_path.exists() {
                        tracing::debug!(entity_id = %entity.id, path = %dir_path, "removing orphaned directory/infra_root");
                        
                        // Remove all edges connected to this entity
                        if let Ok(edges) = store.edges_from(&entity.id) {
                            for edge in edges {
                                let _ = store.delete_edge(&edge.src_id, edge.rel, &edge.dst_id);
                            }
                        }
                        if let Ok(edges) = store.edges_to(&entity.id) {
                            for edge in edges {
                                let _ = store.delete_edge(&edge.src_id, edge.rel, &edge.dst_id);
                            }
                        }
                        
                        // Remove the entity
                        let _ = store.delete_entity(&entity.id);
                    }
                }
            }
            _ => {}
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
    fn index_project_creates_containment_hierarchy() {
        let store = Store::open_in_memory().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();

        index_project(&store, root).unwrap();

        let all_entities = store.list_entities().unwrap();

        // 1. Repo entity should exist
        let repos: Vec<_> = all_entities
            .iter()
            .filter(|e| e.kind == EntityKind::Repo)
            .collect();
        assert_eq!(repos.len(), 1, "should have exactly one Repo entity");
        let repo = repos[0];

        // 2. Repo should have Contains edges to top-level items
        let repo_edges = store.edges_from(&repo.id).unwrap();
        let repo_contains: Vec<_> = repo_edges
            .iter()
            .filter(|e| e.rel == EdgeKind::Contains)
            .collect();
        assert!(
            !repo_contains.is_empty(),
            "Repo should contain top-level items"
        );

        // 3. Directory entities should exist
        let directories: Vec<_> = all_entities
            .iter()
            .filter(|e| e.kind == EntityKind::Directory)
            .collect();
        assert!(
            !directories.is_empty(),
            "should have Directory entities for subdirectories"
        );

        // 4. At least one directory should contain children
        let mut found_dir_with_children = false;
        for dir in &directories {
            let dir_edges = store.edges_from(&dir.id).unwrap();
            let contains_edges: Vec<_> = dir_edges
                .iter()
                .filter(|e| e.rel == EdgeKind::Contains)
                .collect();
            if !contains_edges.is_empty() {
                found_dir_with_children = true;
                break;
            }
        }
        assert!(
            found_dir_with_children,
            "at least one directory should contain children"
        );

        // 5. Directory entity IDs should use the "dir::" prefix
        for dir in &directories {
            assert!(
                dir.id.starts_with("dir::"),
                "directory entity ID should start with 'dir::', got: {}",
                dir.id
            );
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
        let dir = tempfile::tempdir().unwrap();

        // Write a mise.toml with two tasks
        std::fs::write(
            dir.path().join("mise.toml"),
            "[tasks]\nbuild = \"cargo build\"\ntest = \"cargo test\"\n",
        )
        .unwrap();

        // Write a dummy .rs file so the indexer has something to walk
        std::fs::write(dir.path().join("dummy.rs"), "fn main() {}\n").unwrap();

        let store = Store::open_in_memory().unwrap();
        let stats = index_project(&store, dir.path()).unwrap();

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
    }

    #[test]
    fn index_project_creates_repo_without_mise_toml() {
        let dir = tempfile::tempdir().unwrap();

        // Write only a dummy .rs file, no mise.toml
        std::fs::write(dir.path().join("dummy.rs"), "fn main() {}\n").unwrap();

        let store = Store::open_in_memory().unwrap();
        let stats = index_project(&store, dir.path()).unwrap();

        // Should still create a Repo entity
        let all_entities = store.list_entities().unwrap();
        let repos: Vec<_> = all_entities
            .iter()
            .filter(|e| e.kind == EntityKind::Repo)
            .collect();
        assert_eq!(repos.len(), 1, "should have Repo even without mise.toml");

        // But no tasks
        assert_eq!(stats.tasks_extracted, 0, "no tasks without mise.toml");
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
            None,
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
            None,
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
            None,
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
    fn resolve_ts_import_prefers_ts_over_tsx() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        // Create both .ts and .tsx with the same stem
        std::fs::write(src_dir.join("utils.ts"), "export {};").unwrap();
        std::fs::write(src_dir.join("utils.tsx"), "export {};").unwrap();

        // .ts should be found first because it appears before .tsx in the probe list
        let result = resolve_ts_import("./utils", "src/app.ts", tmp.path());
        assert_eq!(result, Some("src/utils.ts".to_string()));
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
        assert_eq!(normalize_path(p), Some(PathBuf::from("src/utils.ts")));
    }

    #[test]
    fn normalize_path_handles_current_dir() {
        let p = Path::new("src/./utils.ts");
        assert_eq!(normalize_path(p), Some(PathBuf::from("src/utils.ts")));
    }
    
    #[test]
    fn normalize_path_detects_traversal_outside_root() {
        // Path goes outside root: src/../../foo from src/file.ts
        let p = Path::new("src/../../foo");
        assert_eq!(normalize_path(p), None);
    }
    
    #[test]
    fn normalize_path_detects_deep_traversal() {
        // Many parent components from deep path
        let p = Path::new("a/b/c/../../../../../../foo");
        assert_eq!(normalize_path(p), None);
    }

    #[test]
    fn index_package_json_creates_component_and_deps() {
        let tmp = tempfile::tempdir().unwrap();

        let pkg_content = r#"{
            "name": "my-web-app",
            "version": "2.0.0",
            "dependencies": {
                "express": "^4.18.0",
                "lodash": "^4.17.21"
            },
            "devDependencies": {
                "typescript": "^5.0.0"
            }
        }"#;
        std::fs::write(tmp.path().join("package.json"), pkg_content).unwrap();

        let store = Store::open_in_memory().unwrap();
        let stats = index_project(&store, tmp.path()).unwrap();

        // packages_indexed is no longer tracked separately; components_found is the new metric
        assert_eq!(stats.components_found, 1, "should discover one component");

        // Canonical component ID uses ecosystem::path format
        let comp = store.get_entity("component::npm::.");
        assert!(comp.is_ok(), "component entity should exist with canonical ID");
        let comp = comp.unwrap();
        assert_eq!(comp.kind, EntityKind::Component);
        assert_eq!(comp.language.as_deref(), Some("npm"));
        // Display name comes from package.json
        assert_eq!(comp.name, "my-web-app");

        let edges = store.edges_from("component::npm::.").unwrap();
        let depends_on: Vec<_> = edges
            .iter()
            .filter(|e| e.rel == EdgeKind::DependsOn)
            .collect();
        assert_eq!(depends_on.len(), 3, "should have 3 DependsOn edges");

        // External dependencies use external::npm:: prefix
        let dep_targets: HashSet<_> = depends_on.iter().map(|e| e.dst_id.as_str()).collect();
        assert!(dep_targets.contains("external::npm::express"));
        assert!(dep_targets.contains("external::npm::lodash"));
        assert!(dep_targets.contains("external::npm::typescript"));
    }

    #[test]
    fn index_package_json_uses_canonical_path_id() {
        let tmp = tempfile::tempdir().unwrap();

        std::fs::write(
            tmp.path().join("package.json"),
            r#"{ "name": "my-pkg", "dependencies": { "foo": "1.0" } }"#,
        )
        .unwrap();

        let store = Store::open_in_memory().unwrap();
        let stats = index_project(&store, tmp.path()).unwrap();

        assert_eq!(stats.components_found, 1);

        // Canonical ID uses path-based format, not display name
        // Root package gets "component::npm::."
        let comp = store.get_entity("component::npm::.");
        assert!(
            comp.is_ok(),
            "component should use canonical path-based ID"
        );
        let comp = comp.unwrap();
        // Display name still comes from package.json
        assert_eq!(comp.name, "my-pkg");
    }

    // =========================================================================
    // BUG FIX TESTS
    // =========================================================================

    /// Test that entities get correct component_id assigned
    #[test]
    fn component_id_assigned_to_source_files_and_symbols() {
        let tmp = tempfile::tempdir().unwrap();
        
        // Create a crate structure
        let crate_dir = tmp.path().join("my-crate");
        std::fs::create_dir(&crate_dir).unwrap();
        std::fs::create_dir(crate_dir.join("src")).unwrap();
        
        // Cargo.toml makes this a component
        std::fs::write(
            crate_dir.join("Cargo.toml"),
            r#"[package]
name = "my-crate"
version = "0.1.0"
"#,
        ).unwrap();
        
        // Source file with symbols
        std::fs::write(
            crate_dir.join("src/lib.rs"),
            r#"pub fn hello() {}
pub struct MyStruct;"#,
        ).unwrap();

        let store = Store::open_in_memory().unwrap();
        let _stats = index_project(&store, tmp.path()).unwrap();

        // Check component exists with canonical ID (ecosystem::path format)
        let canonical_id = "component::cargo::my-crate";
        let comp = store.get_entity(canonical_id).unwrap();
        assert_eq!(comp.kind, EntityKind::Component);
        // Display name comes from Cargo.toml
        assert_eq!(comp.name, "my-crate");

        // Check source file has component_id (entity ID uses "file::" prefix)
        let source_unit = store.get_entity("file::my-crate/src/lib.rs").unwrap();
        assert_eq!(source_unit.component_id, Some(canonical_id.to_string()));

        // Check symbols have component_id
        let symbol = store.get_entity("symbol::my-crate/src/lib.rs::hello").unwrap();
        assert_eq!(symbol.component_id, Some(canonical_id.to_string()));
        
        let struct_symbol = store.get_entity("symbol::my-crate/src/lib.rs::MyStruct").unwrap();
        assert_eq!(struct_symbol.component_id, Some(canonical_id.to_string()));
    }

    /// Test that cleanup removes all related data (edges, summaries, embeddings)
    #[test]
    fn cleanup_removes_edges_summaries_and_embeddings() {
        use chizu_core::model::{EmbeddingRecord, Summary};
        
        let tmp = tempfile::tempdir().unwrap();
        
        // Create a source file
        let src_dir = tmp.path().join("src");
        std::fs::create_dir(&src_dir).unwrap();
        std::fs::write(src_dir.join("lib.rs"), "pub fn foo() {}").unwrap();

        let store = Store::open_in_memory().unwrap();
        
        // First index
        let _ = index_project(&store, tmp.path()).unwrap();
        
        // Add summary for the symbol
        let symbol_id = "symbol::src/lib.rs::foo";
        store.upsert_summary(&Summary {
            entity_id: symbol_id.to_string(),
            short_summary: "Test summary".to_string(),
            detailed_summary: None,
            keywords: vec!["test".to_string()],
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            source_hash: None,
        }).unwrap();
        
        // Add embedding for the symbol
        store.upsert_embedding(&EmbeddingRecord {
            entity_id: symbol_id.to_string(),
            model: "test".to_string(),
            dimensions: 3,
            vector: vec![1.0, 2.0, 3.0],
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }).unwrap();
        
        // Verify data exists
        assert!(store.get_summary(symbol_id).is_ok());
        assert!(store.get_embedding(symbol_id).is_ok());
        let edges_before = store.edges_from("file::src/lib.rs").unwrap();
        assert!(!edges_before.is_empty(), "Should have edges before cleanup");
        
        // Delete the source file
        std::fs::remove_file(src_dir.join("lib.rs")).unwrap();
        
        // Re-index (should trigger cleanup)
        let _ = index_project(&store, tmp.path()).unwrap();
        
        // Verify symbol is gone
        assert!(store.get_entity(symbol_id).is_err(), "Symbol should be deleted");
        
        // Verify summary is gone
        assert!(store.get_summary(symbol_id).is_err(), "Summary should be deleted");
        
        // Verify embedding is gone  
        assert!(store.get_embedding(symbol_id).is_err(), "Embedding should be deleted");
        
        // Verify source_unit is gone (entity ID uses "file::" prefix)
        assert!(store.get_entity("file::src/lib.rs").is_err(), "Source unit should be deleted");
    }

    /// Test that TypeScript imports outside repo root are rejected
    #[test]
    fn ts_import_outside_root_is_rejected() {
        // Path deep in tree trying to escape: src/a/b/c/../../../../../../foo
        // From file at src/a/b/c/main.ts
        let importing_file = "src/a/b/c/main.ts";
        let import_path = "../../../../../../foo";
        
        let result = resolve_ts_import(import_path, importing_file, Path::new("/project"));
        
        assert!(result.is_none(), "Import outside repo root should be rejected");
    }

    /// Test that valid TypeScript imports work
    #[test]
    fn ts_import_valid_resolves_correctly() {
        let tmp = tempfile::tempdir().unwrap();
        
        // Create structure
        let src_dir = tmp.path().join("src");
        std::fs::create_dir(&src_dir).unwrap();
        let utils_dir = src_dir.join("utils");
        std::fs::create_dir(&utils_dir).unwrap();
        
        // Create files
        std::fs::write(src_dir.join("main.ts"), r#"import { helper } from "./utils/helper";"#).unwrap();
        std::fs::write(utils_dir.join("helper.ts"), "export function helper() {}").unwrap();

        // Test resolution
        let resolved = resolve_ts_import("./utils/helper", "src/main.ts", tmp.path());
        assert_eq!(resolved, Some("src/utils/helper.ts".to_string()));
        
        // Test parent import
        std::fs::write(utils_dir.join("nested.ts"), r#"import { main } from "../main";"#).unwrap();
        let resolved_parent = resolve_ts_import("../main", "src/utils/nested.ts", tmp.path());
        assert_eq!(resolved_parent, Some("src/main.ts".to_string()));
    }

    /// Test workspace packages get component_id assigned
    #[test]
    fn workspace_packages_get_component_ids() {
        let tmp = tempfile::tempdir().unwrap();
        
        // Root package.json with workspaces
        std::fs::write(
            tmp.path().join("package.json"),
            r#"{
                "name": "root",
                "workspaces": ["packages/*"]
            }"#,
        ).unwrap();
        
        // Create workspace packages
        let packages_dir = tmp.path().join("packages");
        std::fs::create_dir(&packages_dir).unwrap();
        
        let pkg_a_dir = packages_dir.join("pkg-a");
        std::fs::create_dir(&pkg_a_dir).unwrap();
        std::fs::write(
            pkg_a_dir.join("package.json"),
            r#"{"name": "pkg-a", "version": "1.0.0"}"#,
        ).unwrap();
        std::fs::create_dir(pkg_a_dir.join("src")).unwrap();
        std::fs::write(pkg_a_dir.join("src/index.ts"), "export const foo = 1;").unwrap();
        
        let pkg_b_dir = packages_dir.join("pkg-b");
        std::fs::create_dir(&pkg_b_dir).unwrap();
        std::fs::write(
            pkg_b_dir.join("package.json"),
            r#"{"name": "pkg-b", "version": "1.0.0"}"#,
        ).unwrap();

        let store = Store::open_in_memory().unwrap();
        let _ = index_project(&store, tmp.path()).unwrap();

        // Check all components exist with canonical path-based IDs
        // Root component
        let comp_root = store.get_entity("component::npm::.").unwrap();
        assert_eq!(comp_root.kind, EntityKind::Component);
        assert_eq!(comp_root.name, "root");
        
        // Workspace package components use path-based IDs
        let comp_a = store.get_entity("component::npm::packages/pkg-a").unwrap();
        assert_eq!(comp_a.kind, EntityKind::Component);
        assert_eq!(comp_a.name, "pkg-a");  // Display name from package.json
        
        let comp_b = store.get_entity("component::npm::packages/pkg-b").unwrap();
        assert_eq!(comp_b.kind, EntityKind::Component);
        assert_eq!(comp_b.name, "pkg-b");

        // Check source file in pkg-a has correct component_id (entity ID uses "file::" prefix)
        let source_unit = store.get_entity("file::packages/pkg-a/src/index.ts").unwrap();
        assert_eq!(source_unit.component_id, Some("component::npm::packages/pkg-a".to_string()));
        
        // Verify workspace Contains edge exists from root to pkg-a
        let root_edges = store.edges_from("component::npm::.").unwrap();
        let contains_pkg_a = root_edges.iter().any(|e| {
            e.rel == EdgeKind::Contains && e.dst_id == "component::npm::packages/pkg-a"
        });
        assert!(contains_pkg_a, "root should contain pkg-a via Contains edge");
    }
    
    /// Test that Cargo workspace members are correctly discovered as separate components
    #[test]
    fn cargo_workspace_members_get_component_ids() {
        let tmp = tempfile::tempdir().unwrap();
        
        // Root Cargo.toml with workspace definition
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            r#"[package]
name = "root-crate"
version = "0.1.0"

[workspace]
members = ["crates/*"]
"#,
        ).unwrap();
        
        // Create a source file in root crate
        std::fs::create_dir(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src/main.rs"),
            r#"fn main() { println!("root"); }"#,
        ).unwrap();
        
        // Create workspace member crates
        let crates_dir = tmp.path().join("crates");
        std::fs::create_dir(&crates_dir).unwrap();
        
        let crate_a_dir = crates_dir.join("crate-a");
        std::fs::create_dir(&crate_a_dir).unwrap();
        std::fs::write(
            crate_a_dir.join("Cargo.toml"),
            r#"[package]
name = "crate-a"
version = "0.1.0"
"#,
        ).unwrap();
        std::fs::create_dir(crate_a_dir.join("src")).unwrap();
        std::fs::write(
            crate_a_dir.join("src/lib.rs"),
            r#"pub fn func_a() {}"#,
        ).unwrap();
        
        let crate_b_dir = crates_dir.join("crate-b");
        std::fs::create_dir(&crate_b_dir).unwrap();
        std::fs::write(
            crate_b_dir.join("Cargo.toml"),
            r#"[package]
name = "crate-b"
version = "0.1.0"
"#,
        ).unwrap();
        std::fs::create_dir(crate_b_dir.join("src")).unwrap();
        std::fs::write(
            crate_b_dir.join("src/lib.rs"),
            r#"pub fn func_b() {}"#,
        ).unwrap();

        let store = Store::open_in_memory().unwrap();
        let stats = index_project(&store, tmp.path()).unwrap();

        // Should discover 3 components: root-crate, crate-a, crate-b
        assert_eq!(stats.components_found, 3, "should discover all workspace members");

        // Check all components exist with canonical path-based IDs
        // Root crate component
        let comp_root = store.get_entity("component::cargo::.").unwrap();
        assert_eq!(comp_root.kind, EntityKind::Component);
        assert_eq!(comp_root.name, "root-crate");
        
        // Workspace member components use path-based IDs
        let comp_a = store.get_entity("component::cargo::crates/crate-a").unwrap();
        assert_eq!(comp_a.kind, EntityKind::Component);
        assert_eq!(comp_a.name, "crate-a");
        
        let comp_b = store.get_entity("component::cargo::crates/crate-b").unwrap();
        assert_eq!(comp_b.kind, EntityKind::Component);
        assert_eq!(comp_b.name, "crate-b");

        // Check source file in crate-a has correct component_id
        let source_unit_a = store.get_entity("file::crates/crate-a/src/lib.rs").unwrap();
        assert_eq!(source_unit_a.component_id, Some("component::cargo::crates/crate-a".to_string()));
        
        // Check source file in crate-b has correct component_id
        let source_unit_b = store.get_entity("file::crates/crate-b/src/lib.rs").unwrap();
        assert_eq!(source_unit_b.component_id, Some("component::cargo::crates/crate-b".to_string()));
        
        // Check root crate source file has correct component_id
        let source_unit_root = store.get_entity("file::src/main.rs").unwrap();
        assert_eq!(source_unit_root.component_id, Some("component::cargo::.".to_string()));
    }
}
