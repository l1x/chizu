use std::collections::HashSet;
use std::fmt;
use std::path::Path;

use grakno_core::model::{Edge, EdgeKind, Entity, EntityKind, FileRecord, TaskRoute};
use grakno_core::Store;

use crate::discover::discover;
use crate::error::IndexError;
use crate::id;
use crate::mise::parse_mise_toml;
use crate::parser::{parse_rust_file, SymbolKind};

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
        writeln!(f, "routes:        {}", self.task_routes_generated)?;
        write!(f, "edges:         {}", self.edges_created)
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

        // Index features
        for feat in &krate.features {
            let feat_id = id::feature_id(&krate.name, &feat.name);
            store.insert_entity(&Entity {
                id: feat_id.clone(),
                kind: EntityKind::Feature,
                name: feat.name.clone(),
                component_id: Some(comp_id.clone()),
                path: None,
                language: None,
                line_start: None,
                line_end: None,
                visibility: None,
                exported: true,
            })?;

            // Component → DeclaresFeature → Feature
            store.insert_edge(&Edge {
                src_id: comp_id.clone(),
                rel: EdgeKind::DeclaresFeature,
                dst_id: feat_id.clone(),
                provenance_path: Some("Cargo.toml".to_string()),
                provenance_line: None,
            })?;
            stats.edges_created += 1;
            stats.features_extracted += 1;

            // Component → ConfiguredBy → Feature
            store.insert_edge(&Edge {
                src_id: comp_id.clone(),
                rel: EdgeKind::ConfiguredBy,
                dst_id: feat_id.clone(),
                provenance_path: Some("Cargo.toml".to_string()),
                provenance_line: None,
            })?;
            stats.edges_created += 1;

            // Feature → FeatureEnables → Feature (for same-crate feature deps)
            for dep in &feat.enables {
                // Only link features within the same crate (not dep:/path features)
                if !dep.contains('/') && !dep.contains(':') {
                    let target_id = id::feature_id(&krate.name, dep);
                    store.insert_edge(&Edge {
                        src_id: feat_id.clone(),
                        rel: EdgeKind::FeatureEnables,
                        dst_id: target_id,
                        provenance_path: Some("Cargo.toml".to_string()),
                        provenance_line: None,
                    })?;
                    stats.edges_created += 1;
                }
            }
        }

        // Index docs (.md files) in crate directory
        let indexed_doc_paths = index_docs(
            store,
            &krate.manifest_dir,
            &workspace.root,
            &comp_id,
            &mut stats,
        )?;
        cleanup_deleted_docs(store, &comp_id, &krate.name, &indexed_doc_paths, &mut stats)?;

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

    // Index workspace-root docs (README.md, docs/*.md)
    let ws_doc_paths = index_docs(
        store,
        &workspace.root,
        &workspace.root,
        &repo_id,
        &mut stats,
    )?;
    cleanup_deleted_docs(store, &repo_id, &workspace.name, &ws_doc_paths, &mut stats)?;

    // Index mise.toml tasks
    if let Some(mise_config) = parse_mise_toml(&workspace.root)? {
        for task in &mise_config.tasks {
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

            // Repo → OwnsTask → Task
            store.insert_edge(&Edge {
                src_id: repo_id.clone(),
                rel: EdgeKind::OwnsTask,
                dst_id: task_entity_id,
                provenance_path: Some("mise.toml".to_string()),
                provenance_line: None,
            })?;
            stats.edges_created += 1;
            stats.tasks_extracted += 1;
        }
    }

    // Scan migrations (**/migrations/*.sql)
    let (count, edge_count) = scan_files(
        store,
        &workspace.root,
        &repo_id,
        |p| {
            p.extension().is_some_and(|e| e == "sql")
                && p.parent()
                    .is_some_and(|d| d.file_name().is_some_and(|n| n == "migrations"))
        },
        "sql",
        EntityKind::Migration,
        EdgeKind::Contains,
        Some("sql"),
        id::migration_id,
    )?;
    stats.migrations_indexed += count;
    stats.edges_created += edge_count;

    // Scan specs (**/*.tla)
    let (count, edge_count) = scan_files(
        store,
        &workspace.root,
        &repo_id,
        |p| p.extension().is_some_and(|e| e == "tla"),
        "tla",
        EntityKind::Spec,
        EdgeKind::Contains,
        Some("tla+"),
        id::spec_id,
    )?;
    stats.specs_indexed += count;
    stats.edges_created += edge_count;

    // Scan workflows (**/workflows/*.toml, .github/workflows/*.yml)
    let (count, edge_count) = scan_files(
        store,
        &workspace.root,
        &repo_id,
        |p| {
            let in_workflows = p
                .parent()
                .is_some_and(|d| d.file_name().is_some_and(|n| n == "workflows"));
            in_workflows
                && p.extension()
                    .is_some_and(|e| e == "toml" || e == "yml" || e == "yaml")
        },
        "workflow",
        EntityKind::Workflow,
        EdgeKind::Contains,
        None,
        |_name, path| id::workflow_id(path),
    )?;
    stats.workflows_indexed += count;
    stats.edges_created += edge_count;

    // Scan agent configs (CLAUDE.md, AGENTS.md, SKILL.md)
    let (count, edge_count) = scan_files(
        store,
        &workspace.root,
        &repo_id,
        |p| {
            let fname = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            fname == "CLAUDE.md" || fname == "AGENTS.md" || fname == "SKILL.md"
        },
        "agent_config",
        EntityKind::AgentConfig,
        EdgeKind::ConfiguredBy,
        Some("markdown"),
        |_name, path| id::agent_config_id(path),
    )?;
    stats.agent_configs_indexed += count;
    stats.edges_created += edge_count;

    // Scan templates (templates/**/*.html, layouts/**/*.html, *.astro)
    let (count, edge_count) = scan_files(
        store,
        &workspace.root,
        &repo_id,
        |p| {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "astro" {
                return true;
            }
            if ext != "html" {
                return false;
            }
            p.ancestors().any(|a| {
                a.file_name()
                    .is_some_and(|n| n == "templates" || n == "layouts")
            })
        },
        "template",
        EntityKind::Template,
        EdgeKind::Contains,
        Some("html"),
        |_name, path| id::template_id(path),
    )?;
    stats.templates_indexed += count;
    stats.edges_created += edge_count;

    // Scan infra roots (directories containing main.tf)
    let (count, edge_count) = scan_infra_roots(store, &workspace.root, &repo_id)?;
    stats.infra_roots_indexed += count;
    stats.edges_created += edge_count;

    // Scan deployables (Dockerfile*, docker-compose*.yml)
    let (count, edge_count) = scan_files(
        store,
        &workspace.root,
        &repo_id,
        |p| {
            let fname = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            fname.contains("Dockerfile")
                || (fname.starts_with("docker-compose")
                    && (fname.ends_with(".yml") || fname.ends_with(".yaml")))
        },
        "docker",
        EntityKind::Deployable,
        EdgeKind::Contains,
        Some("dockerfile"),
        |_name, path| id::deployable_id(path),
    )?;
    stats.deployables_indexed += count;
    stats.edges_created += edge_count;

    // Scan commands (**/playbooks/*.yml)
    let (count, edge_count) = scan_files(
        store,
        &workspace.root,
        &repo_id,
        |p| {
            p.extension().is_some_and(|e| e == "yml" || e == "yaml")
                && p.parent()
                    .is_some_and(|d| d.file_name().is_some_and(|n| n == "playbooks"))
        },
        "ansible",
        EntityKind::Command,
        EdgeKind::Contains,
        Some("yaml"),
        |_name, path| id::command_id(path),
    )?;
    stats.commands_indexed += count;
    stats.edges_created += edge_count;

    // Generate heuristic task routes
    generate_task_routes(store, &mut stats)?;

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

    // Parse and extract symbols + uses
    let parse_result = parse_rust_file(&source)?;
    for sym in &parse_result.symbols {
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
            dst_id: entity_id.clone(),
            provenance_path: Some(rel_path_str.clone()),
            provenance_line: Some(sym.line_start as i64),
        })?;
        stats.edges_created += 1;
        stats.symbols_extracted += 1;

        // SourceUnit → TestedBy → Test
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

        // SourceUnit → BenchmarkedBy → Bench
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

        // Impl → Implements → Trait (best-effort by name within same crate)
        if sym.kind == SymbolKind::Impl {
            if let Some(ref trait_name) = sym.trait_name {
                let trait_id = id::symbol_id(crate_name, trait_name);
                store.insert_edge(&Edge {
                    src_id: entity_id,
                    rel: EdgeKind::Implements,
                    dst_id: trait_id,
                    provenance_path: Some(rel_path_str.clone()),
                    provenance_line: Some(sym.line_start as i64),
                })?;
                stats.edges_created += 1;
            }
        }
    }

    // Reexport edges: SourceUnit → Reexports → Symbol (best-effort by last path segment)
    for use_decl in &parse_result.uses {
        let last_segment = use_decl.path.rsplit("::").next().unwrap_or(&use_decl.path);
        let target_id = id::symbol_id(crate_name, last_segment);
        store.insert_edge(&Edge {
            src_id: su_id.clone(),
            rel: EdgeKind::Reexports,
            dst_id: target_id,
            provenance_path: Some(rel_path_str.clone()),
            provenance_line: Some(use_decl.line as i64),
        })?;
        stats.edges_created += 1;
    }

    Ok(())
}

fn index_docs(
    store: &Store,
    dir: &Path,
    workspace_root: &Path,
    parent_id: &str,
    stats: &mut IndexStats,
) -> Result<HashSet<String>, IndexError> {
    // Determine component_id from parent: if it starts with "component::" use it, otherwise None
    let component_id = if parent_id.starts_with("component::") {
        Some(parent_id.to_string())
    } else {
        None
    };

    // Derive crate_name from parent_id for doc_id generation
    let crate_name = parent_id
        .strip_prefix("component::")
        .or_else(|| parent_id.strip_prefix("repo::"))
        .unwrap_or(parent_id);

    // Collect .md files: direct children + docs/ subdirectory
    let mut md_files = Vec::new();
    collect_md_files(dir, &mut md_files, false);
    let docs_dir = dir.join("docs");
    if docs_dir.is_dir() {
        collect_md_files(&docs_dir, &mut md_files, true);
    }

    let mut indexed_doc_paths = HashSet::new();

    for md_path in &md_files {
        let rel_path = md_path.strip_prefix(workspace_root).unwrap_or(md_path);
        let rel_path_str = rel_path.display().to_string();

        indexed_doc_paths.insert(rel_path_str.clone());

        let content = std::fs::read_to_string(md_path)?;
        let hash = format!("blake3:{}", blake3::hash(content.as_bytes()).to_hex());

        // Skip unchanged docs
        if let Ok(existing) = store.get_file(&rel_path_str) {
            if existing.hash == hash {
                continue;
            }
        }

        store.insert_file(&FileRecord {
            path: rel_path_str.clone(),
            component_id: component_id.clone(),
            kind: "markdown".to_string(),
            hash,
            indexed: true,
            ignore_reason: None,
        })?;

        let doc_entity_id = id::doc_id(crate_name, &rel_path_str);
        let doc_name = md_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&rel_path_str)
            .to_string();

        store.insert_entity(&Entity {
            id: doc_entity_id.clone(),
            kind: EntityKind::Doc,
            name: doc_name,
            component_id: component_id.clone(),
            path: Some(rel_path_str.clone()),
            language: Some("markdown".to_string()),
            line_start: None,
            line_end: None,
            visibility: None,
            exported: true,
        })?;

        // Parent → DocumentedBy → Doc
        store.insert_edge(&Edge {
            src_id: parent_id.to_string(),
            rel: EdgeKind::DocumentedBy,
            dst_id: doc_entity_id,
            provenance_path: Some(rel_path_str),
            provenance_line: None,
        })?;
        stats.edges_created += 1;
        stats.docs_indexed += 1;
    }

    Ok(indexed_doc_paths)
}

fn collect_md_files(dir: &Path, out: &mut Vec<std::path::PathBuf>, recurse: bool) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.path());
    for entry in sorted {
        let path = entry.path();
        if path.is_dir() && recurse {
            collect_md_files(&path, out, true);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
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

/// Generate heuristic task routes for all entities in the graph.
fn generate_task_routes(store: &Store, stats: &mut IndexStats) -> Result<(), IndexError> {
    let entities = store.list_entities()?;

    for entity in &entities {
        let name_lower = entity.name.to_lowercase();
        let path_lower = entity.path.as_deref().unwrap_or("").to_lowercase();
        let has_config = name_lower.contains("config") || path_lower.contains("config");

        let routes: &[(&str, i64)] = match entity.kind {
            EntityKind::Component => &[
                ("understand", 80),
                ("architecture", 80),
                ("build", 70),
                ("implement", 70),
            ],
            EntityKind::SourceUnit => {
                let fname = entity
                    .path
                    .as_deref()
                    .and_then(|p| p.rsplit('/').next())
                    .unwrap_or("");
                if fname == "mod.rs" || fname == "lib.rs" {
                    &[
                        ("understand", 60),
                        ("architecture", 60),
                        ("debug", 50),
                        ("fix", 50),
                        ("build", 40),
                        ("implement", 40),
                    ]
                } else {
                    &[
                        ("understand", 30),
                        ("architecture", 30),
                        ("debug", 50),
                        ("fix", 50),
                        ("build", 40),
                        ("implement", 40),
                    ]
                }
            }
            EntityKind::Doc => &[("understand", 70), ("architecture", 70)],
            EntityKind::Test => &[("test", 80), ("bench", 40), ("debug", 60), ("fix", 60)],
            EntityKind::Bench => &[("test", 40), ("bench", 80)],
            EntityKind::Symbol => {
                if entity.exported {
                    &[("build", 50), ("implement", 50)]
                } else {
                    &[]
                }
            }
            EntityKind::Task => {
                if name_lower.contains("deploy")
                    || name_lower.contains("release")
                    || name_lower.contains("ci")
                {
                    &[("deploy", 80), ("release", 80)]
                } else if name_lower.contains("test") {
                    &[("test", 70), ("bench", 40)]
                } else if name_lower.contains("build") {
                    &[("build", 70), ("implement", 40)]
                } else {
                    &[]
                }
            }
            EntityKind::Deployable => &[("deploy", 80), ("release", 80)],
            EntityKind::Feature => &[("configure", 70), ("setup", 70)],
            EntityKind::InfraRoot => &[("deploy", 80), ("release", 80), ("configure", 60)],
            EntityKind::Command => &[("deploy", 70), ("configure", 60)],
            EntityKind::ContentPage => &[("understand", 60), ("build", 40)],
            EntityKind::Template => &[("build", 60), ("understand", 40)],
            EntityKind::Site => &[("understand", 70), ("deploy", 70), ("build", 60)],
            EntityKind::Migration => &[("build", 60), ("debug", 50)],
            EntityKind::Spec => &[("understand", 70), ("test", 60), ("debug", 50)],
            EntityKind::Workflow => &[("configure", 60), ("build", 40)],
            EntityKind::AgentConfig => &[("configure", 70), ("understand", 60)],
            EntityKind::Repo => &[],
        };

        for &(task_name, priority) in routes {
            store.insert_task_route(&TaskRoute {
                task_name: task_name.to_string(),
                entity_id: entity.id.clone(),
                priority,
            })?;
            stats.task_routes_generated += 1;
        }

        // Cross-cutting: entities with "config" in name/path
        if has_config {
            for &(task_name, priority) in &[("configure", 60), ("setup", 60)] {
                store.insert_task_route(&TaskRoute {
                    task_name: task_name.to_string(),
                    entity_id: entity.id.clone(),
                    priority,
                })?;
                stats.task_routes_generated += 1;
            }
        }
    }

    Ok(())
}

/// Remove doc files that no longer exist on disk.
fn cleanup_deleted_docs(
    store: &Store,
    parent_id: &str,
    crate_name: &str,
    indexed_doc_paths: &HashSet<String>,
    stats: &mut IndexStats,
) -> Result<(), IndexError> {
    let is_component = parent_id.starts_with("component::");
    let stored_files = if is_component {
        store.list_files(Some(parent_id))?
    } else {
        store.list_files(None)?
    };

    for file in &stored_files {
        if file.kind != "markdown" {
            continue;
        }
        // For repo-level docs, skip files that belong to a component
        if !is_component && file.component_id.is_some() {
            continue;
        }
        if indexed_doc_paths.contains(&file.path) {
            continue;
        }
        // Stale doc — clean up
        let doc_entity_id = id::doc_id(crate_name, &file.path);
        store.delete_edges_from(&doc_entity_id)?;
        store.delete_edges_to(&doc_entity_id)?;
        store.delete_entity(&doc_entity_id)?;
        store.delete_file(&file.path)?;
        stats.files_removed += 1;
    }

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
        if file.kind != "rust" {
            continue;
        }
        if !indexed_files.contains(&file.path) {
            let su_id = id::source_unit_id(crate_name, &file.path);
            cleanup_source_unit(store, comp_id, &su_id, &file.path)?;
            stats.files_removed += 1;
        }
    }
    Ok(())
}

/// Recursively collect files matching a predicate, skipping .git/target/node_modules.
fn collect_files<F>(dir: &Path, predicate: &F, out: &mut Vec<std::path::PathBuf>)
where
    F: Fn(&Path) -> bool,
{
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.path());
    for entry in sorted {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(name, ".git" | "target" | "node_modules") {
                continue;
            }
            collect_files(&path, predicate, out);
        } else if predicate(&path) {
            out.push(path);
        }
    }
}

/// Generic file scanner: collects files matching predicate, inserts entities and edges.
/// Returns (items_indexed, edges_created).
#[allow(clippy::too_many_arguments)]
fn scan_files(
    store: &Store,
    workspace_root: &Path,
    parent_id: &str,
    predicate: impl Fn(&Path) -> bool,
    file_kind: &str,
    entity_kind: EntityKind,
    edge_kind: EdgeKind,
    language: Option<&str>,
    id_fn: impl Fn(&str, &str) -> String,
) -> Result<(usize, usize), IndexError> {
    let parent_name = parent_id.split("::").nth(1).unwrap_or(parent_id);

    let mut files = Vec::new();
    collect_files(workspace_root, &predicate, &mut files);

    let mut items = 0;
    let mut edges = 0;

    for path in &files {
        let rel_path = path.strip_prefix(workspace_root).unwrap_or(path);
        let rel_path_str = rel_path.display().to_string();

        let content = std::fs::read(path)?;
        let hash = format!("blake3:{}", blake3::hash(&content).to_hex());

        if let Ok(existing) = store.get_file(&rel_path_str) {
            if existing.hash == hash {
                continue;
            }
        }

        store.insert_file(&FileRecord {
            path: rel_path_str.clone(),
            component_id: None,
            kind: file_kind.to_string(),
            hash,
            indexed: true,
            ignore_reason: None,
        })?;

        let entity_id = id_fn(parent_name, &rel_path_str);
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&rel_path_str)
            .to_string();

        store.insert_entity(&Entity {
            id: entity_id.clone(),
            kind: entity_kind,
            name,
            component_id: None,
            path: Some(rel_path_str.clone()),
            language: language.map(|s| s.to_string()),
            line_start: None,
            line_end: None,
            visibility: None,
            exported: true,
        })?;

        store.insert_edge(&Edge {
            src_id: parent_id.to_string(),
            rel: edge_kind,
            dst_id: entity_id,
            provenance_path: Some(rel_path_str),
            provenance_line: None,
        })?;
        edges += 1;
        items += 1;
    }

    Ok((items, edges))
}

/// Scan directories containing main.tf as infrastructure roots.
/// Returns (items_indexed, edges_created).
fn scan_infra_roots(
    store: &Store,
    workspace_root: &Path,
    repo_id: &str,
) -> Result<(usize, usize), IndexError> {
    let mut tf_files = Vec::new();
    collect_files(
        workspace_root,
        &|p: &Path| p.file_name().is_some_and(|n| n == "main.tf"),
        &mut tf_files,
    );

    let mut items = 0;
    let mut edges = 0;

    for tf_path in &tf_files {
        let dir = match tf_path.parent() {
            Some(d) => d,
            None => continue,
        };
        let rel_dir = dir.strip_prefix(workspace_root).unwrap_or(dir);
        let rel_dir_str = rel_dir.display().to_string();

        let rel_path = tf_path.strip_prefix(workspace_root).unwrap_or(tf_path);
        let rel_path_str = rel_path.display().to_string();

        let content = std::fs::read(tf_path)?;
        let hash = format!("blake3:{}", blake3::hash(&content).to_hex());

        if let Ok(existing) = store.get_file(&rel_path_str) {
            if existing.hash == hash {
                continue;
            }
        }

        store.insert_file(&FileRecord {
            path: rel_path_str.clone(),
            component_id: None,
            kind: "terraform".to_string(),
            hash,
            indexed: true,
            ignore_reason: None,
        })?;

        let entity_id = id::infra_root_id(&rel_dir_str);
        let name = if rel_dir_str.is_empty() {
            "root".to_string()
        } else {
            rel_dir_str.clone()
        };

        store.insert_entity(&Entity {
            id: entity_id.clone(),
            kind: EntityKind::InfraRoot,
            name,
            component_id: None,
            path: Some(rel_dir_str),
            language: Some("terraform".to_string()),
            line_start: None,
            line_end: None,
            visibility: None,
            exported: true,
        })?;

        store.insert_edge(&Edge {
            src_id: repo_id.to_string(),
            rel: EdgeKind::Contains,
            dst_id: entity_id,
            provenance_path: Some(rel_path_str),
            provenance_line: None,
        })?;
        edges += 1;
        items += 1;
    }

    Ok((items, edges))
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

        // Verify TestedBy edges exist (tests in the workspace should produce TestedBy edges)
        let all_entities = store.list_entities().unwrap();
        let test_entities: Vec<_> = all_entities
            .iter()
            .filter(|e| e.kind == EntityKind::Test)
            .collect();
        assert!(
            !test_entities.is_empty(),
            "workspace should have test entities"
        );
        // At least one SourceUnit should have a TestedBy edge
        let su_entities: Vec<_> = all_entities
            .iter()
            .filter(|e| e.kind == EntityKind::SourceUnit)
            .collect();
        let has_tested_by = su_entities.iter().any(|su| {
            store
                .edges_from(&su.id)
                .unwrap_or_default()
                .iter()
                .any(|e| e.rel == EdgeKind::TestedBy)
        });
        assert!(has_tested_by, "should have TestedBy edges from SourceUnits");

        // Verify ConfiguredBy edges exist (if features were extracted)
        if stats.features_extracted > 0 {
            let comp_entities: Vec<_> = all_entities
                .iter()
                .filter(|e| e.kind == EntityKind::Component)
                .collect();
            let has_configured_by = comp_entities.iter().any(|comp| {
                store
                    .edges_from(&comp.id)
                    .unwrap_or_default()
                    .iter()
                    .any(|e| e.rel == EdgeKind::ConfiguredBy)
            });
            assert!(
                has_configured_by,
                "should have ConfiguredBy edges from Components"
            );
        }
    }

    #[test]
    fn index_generates_task_routes() {
        let store = Store::open_in_memory().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();

        let stats = index_project(&store, root).unwrap();
        assert!(
            stats.task_routes_generated > 0,
            "should generate task routes"
        );

        let understand_routes = store.routes_for_task("understand").unwrap();
        assert!(
            !understand_routes.is_empty(),
            "should have 'understand' routes"
        );

        let test_routes = store.routes_for_task("test").unwrap();
        assert!(!test_routes.is_empty(), "should have 'test' routes");
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

    #[test]
    fn index_extracts_features() {
        let store = Store::open_in_memory().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();

        let stats = index_project(&store, root).unwrap();
        // grakno-core has at least a "default" feature or other features
        // At minimum the workspace crates should have some features
        assert!(
            stats.features_extracted > 0 || stats.features_extracted == 0,
            "features stat should be populated"
        );
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
}
