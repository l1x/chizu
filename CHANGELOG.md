# Changelog

All notable changes to Chizu should be documented in this file.

The format is based on Keep a Changelog, and this project follows Semantic
Versioning.

## [Unreleased]

### Added

- Release metadata for all publishable crates in the workspace.
- A tag-driven GitHub release workflow and a documented release checklist.

### Changed

- Installation docs now include both source and registry install paths.

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

