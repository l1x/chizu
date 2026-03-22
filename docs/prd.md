# Grakno PRD

**Status:** Draft
**Date:** 2026-03-21

## Problem

Coding agents repeatedly pay a repo-discovery tax. They spend too much time
listing files, rediscovering component boundaries, reopening the same
entrypoint files, and re-deriving relationships between code, docs, tests,
build tasks, deployables, and infrastructure.

This problem shows up differently across repositories:

- Harrow is a compact Rust workspace where crate ownership, feature wiring,
  re-exports, and tests/docs are the key routing signals.
- Panzerotti is a heterogeneous component monorepo where services, sites,
  packages, Terraform roots, docs, deployment tasks, and infrastructure are all
  first-class.

The missing piece is not just semantic search. The missing piece is a local,
explicit, queryable structural model of the repository.

## Thesis

Grakno should be a local component graph indexer for repositories.

It should combine:

1. a structured graph for ownership and relationships
2. short generated summaries for fast orientation
3. optional semantic retrieval over summaries

The graph is the source of truth. Semantic search is an entry mechanism, not
the model itself.

## Goals

- Reduce agentic file reading by producing a fast local graph of repo structure
  and task routing.
- Keep the system useful without embeddings.
- Support both Harrow and Panzerotti as first-class design targets.
- Model repos around components, not only around language package systems.
- Support mixed-language repos through adapters and configuration.

## Non-Goals

- Replacing normal file reads entirely
- Becoming a general-purpose graph database
- Requiring embeddings for the system to be useful
- Full whole-program call graph precision in v1

## Architecture

```text
source inputs
  -> adapters
  -> graph extraction
  -> store backend (sqlite+usearch | grafeo)
  -> summary generation
  -> vector search (usearch HNSW | grafeo HNSW)
  -> query / expansion / rerank
```

Two store backends are supported:

- **sqlite+usearch** — SQLite stores the structured graph (entities, edges, files,
  summaries, task routes, embedding metadata). usearch provides HNSW-based
  approximate nearest neighbor search over embedding vectors. Vectors live only
  in usearch, not duplicated in SQLite.
- **grafeo** — A graph-native backend using GrafeoDB (GQL query language). Stores
  all data as labeled property graph nodes and edges. Provides built-in HNSW
  vector indexing on embedding nodes.

Both backends expose the same `Store` API. The choice is a compile-time feature
flag (`grafeo` or `usearch`).

## Core Model

Core entity kinds:

- `repo`
- `component`
- `source_unit`
- `symbol`
- `doc`
- `test`
- `bench`
- `task`
- `deployable`
- `infra_root`
- `command`
- `feature`

Core edge kinds:

- `contains`
- `defines`
- `depends_on`
- `reexports`
- `documented_by`
- `tested_by`
- `benchmarked_by`
- `related_to`
- `configured_by`
- `builds`
- `deploys`
- `implements`
- `owns_task`
- `declares_feature`
- `feature_enables`
- `mentions`

## Initial Adapters

V0 should start with:

- Rust AST
- Cargo metadata
- docs
- `mise.toml`
- simple filesystem and component configuration

Future adapters can include:

- C#
- Astro
- TypeScript package graphs
- Terraform roots
- additional build systems

## Query Model

The intended query flow is:

1. classify the task
2. prefilter with SQL and task routes
3. optionally run semantic lookup over summaries
4. expand neighbors in the graph
5. rerank and return a reading plan

The output should tell an agent which files or components to read first and
why.
