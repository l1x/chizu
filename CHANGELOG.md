# Changelog

All notable changes to Chizu should be documented in this file.

The format is based on Keep a Changelog, and this project follows Semantic
Versioning.

## [Unreleased]

## [0.3.0] - 2026-04-05

### Added

- `graph_traversal` function in `chizu-core` for reusable BFS graph traversal.
- `ChizuStore::open_test` helper behind `test-support` feature flag.
- SQL column list constants (`ENTITY_COLUMNS`, `EDGE_COLUMNS`, etc.) in SQLite store.
- Bulk store methods: `get_all_edges`, `get_all_embedding_metas`,
  `search_entities_by_name_or_path`, `search_summaries_by_text`,
  `delete_edges_for_entity_ids`.
- `PartialOrd`/`Ord` derives on `EdgeKind`.
- MiniJinja template for the interactive HTML tree explorer.
- Tree-sitter line numbers on all Rust entities (functions, structs, enums, traits, tests, benches).
- Release metadata for all publishable crates in the workspace.
- A tag-driven GitHub release workflow and a documented release checklist.

### Changed

- Extracted ~1460 lines of inline HTML/CSS/JS from `visualize.rs` into `explorer.html.j2`.
- Replaced stringly-typed `VisualEdge.rel` and edge sets with `EdgeKind` enum throughout.
- Replaced 10-param `assign_tree_positions` with `TreeLayoutContext` struct.
- Replaced 9-param `extract_items` with `ParseContext` struct.
- Pushed LIKE filtering into SQL for query retrieval (was full-table scan).
- `TraversalOptions.kind_filter` now takes `&[EntityKind]` instead of `&[String]`.
- Installation docs now include both source and registry install paths.

### Fixed

- Eliminated N+1 query patterns in `cmd_visualize`, summarizer, embedder, and cleanup.
- `graph_traversal` now queries on demand (scales with reachable subgraph, not total repo size).
- `resolve_edge_target_names` uses targeted lookups instead of loading all entities.
- UTF-8 truncation panic in `main.rs` (was byte-slicing at arbitrary boundary).
- Property test for `ComponentId` ecosystem extraction with colon edge case.
- CI: skip search integration test that requires Ollama.

## [0.2.0] - 2026-04-04

### Added

- Initial Chizu workspace with `chizu-core`, `chizu-index`, `chizu-query`, and
  the `chizu` CLI binary.
- Deterministic repository fact extraction for components, files, symbols,
  docs, tasks, infra units, and related entities.
- Ranked reading-plan search with task classification, retrieval, graph
  expansion, and reranking.
- Incremental indexing with SQLite storage and usearch vector retrieval.
- Static SVG graph visualization and an interactive HTML tree explorer.

