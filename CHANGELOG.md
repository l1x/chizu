# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-04-05

### Added
- `graph_traversal` function in `chizu-core` for reusable BFS graph traversal
- `ChizuStore::open_test` helper behind `test-support` feature flag
- SQL column list constants (`ENTITY_COLUMNS`, `EDGE_COLUMNS`, etc.) in SQLite store
- Bulk store methods: `get_all_edges`, `get_all_embedding_metas`,
  `search_entities_by_name_or_path`, `search_summaries_by_text`,
  `delete_edges_for_entity_ids`
- `PartialOrd`/`Ord` derives on `EdgeKind`
- MiniJinja template for the interactive HTML tree explorer
- Tree-sitter line numbers on all Rust entities
- Release metadata for all publishable crates
- Tag-driven GitHub release workflow and release checklist
- Comprehensive markdown reference replacing inline guide

### Changed
- Extracted inline HTML/CSS/JS from `visualize.rs` into `explorer.html.j2`
- Replaced stringly-typed edge sets with `EdgeKind` enum throughout
- Replaced multi-param functions with `TreeLayoutContext` and `ParseContext` structs
- Pushed LIKE filtering into SQL for query retrieval
- `TraversalOptions.kind_filter` takes `&[EntityKind]` instead of `&[String]`

### Fixed
- Eliminated N+1 query patterns in visualize, summarizer, embedder, and cleanup
- `graph_traversal` queries on demand (scales with reachable subgraph)
- UTF-8 truncation panic in `main.rs`
- Property test for `ComponentId` ecosystem extraction with colon edge case
- CI: skip search test that requires Ollama

## [0.2.0] - 2026-04-04

### Added
- Initial Chizu workspace with `chizu-core`, `chizu-index`, `chizu-query`, and `chizu-cli`
- Deterministic repository fact extraction for components, files, symbols,
  docs, tasks, infra units, and related entities
- Ranked reading-plan search with task classification, retrieval, graph
  expansion, and reranking
- Incremental indexing with SQLite storage and usearch vector retrieval
- Static SVG graph visualization and interactive HTML tree explorer
- Property-based testing and fuzz targets with bolero
- LLM provider, summarizer, and embedder integration
- TypeScript, Astro, Terraform/HCL, and markdown parsers
- Component-level visualization with pan/zoom
- Configuration system and observability with rolly

## [0.1.0] - 2026-03-21

### Added
- Initial project skeleton
- Dual-backend store (SQLite + usearch and Grafeo)
- LLM-based entity summarization
- Rust indexing with feature extraction, doc indexing, reexports, and trait impls

[0.3.0]: https://github.com/vectorian-rs/chizu/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/vectorian-rs/chizu/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/vectorian-rs/chizu/releases/tag/v0.1.0
