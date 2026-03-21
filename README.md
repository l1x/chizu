# Grakno

Graph Repository Knowledge Navigator.

Grakno is a local-first component graph and navigation index for code
repositories. It builds a structured repo model in SQLite, attaches short
summaries to graph nodes, and can optionally use local vector search over those
summaries for fuzzy entry.

The goal is simple: help agents and developers answer "what should I read
first?" before they start opening files blindly.

## Initial Design Targets

- Harrow: multi-crate Rust workspace with strong feature wiring
- Panzerotti: heterogeneous component monorepo with infra, docs, and mixed
  source systems

## Early Shape

- SQLite as the structured source of truth
- component graph core
- language and source adapters
- generated summaries for speed
- optional vector search over summaries

## Docs

- [Product PRD](docs/prd.md)
- [Graph Model](docs/graph-model.md)
