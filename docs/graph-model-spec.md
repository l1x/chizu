# Graph Model Spec

**Status:** Draft
**Date:** 2026-03-21

## Purpose

This document describes the initial graph model for Chizu.

The model is designed to be:

- local
- explainable
- backed by sqlite+usearch or grafeo
- useful without embeddings
- broad enough for both Harrow and Panzerotti

## Three Layers

Chizu separates three concerns:

- graph = structure
- summaries = fast descriptions of nodes
- embeddings = optional fuzzy lookup over summaries

These should not be collapsed into one mechanism.

## Storage Layout

### sqlite+usearch backend

```text
.agent/
  index.sqlite        # graph, summaries, embedding metadata, task routes
  index.sqlite.usearch # usearch HNSW index (vectors only)
  build.json
  config.toml
```

SQLite stores all structured data. Vectors live exclusively in the usearch HNSW
index file — they are not duplicated in SQLite.

### grafeo backend

```text
.agent/
  index.grafeo/       # GrafeoDB persistent storage (graph + vectors + HNSW)
  build.json
  config.toml
```

Grafeo stores everything — nodes, edges, vectors, and HNSW index — in a single
graph database.

## Core Entity Kinds

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

## Core Edge Kinds

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

## SQLite Schema (v4)

```sql
CREATE TABLE files (
  path TEXT PRIMARY KEY,
  component_id TEXT,
  kind TEXT NOT NULL,
  hash TEXT NOT NULL,
  indexed INTEGER NOT NULL DEFAULT 1,
  ignore_reason TEXT
);

CREATE TABLE entities (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  component_id TEXT,
  path TEXT,
  language TEXT,
  line_start INTEGER,
  line_end INTEGER,
  visibility TEXT,
  exported INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE edges (
  src_id TEXT NOT NULL,
  rel TEXT NOT NULL,
  dst_id TEXT NOT NULL,
  provenance_path TEXT,
  provenance_line INTEGER,
  PRIMARY KEY (src_id, rel, dst_id)
);

CREATE TABLE summaries (
  entity_id TEXT PRIMARY KEY,
  short_summary TEXT NOT NULL,
  detailed_summary TEXT,
  keywords_json TEXT,
  updated_at TEXT NOT NULL,
  source_hash TEXT
);

CREATE TABLE task_routes (
  task_name TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  priority INTEGER NOT NULL,
  PRIMARY KEY (task_name, entity_id)
);

-- Vectors live in usearch, not in SQLite.
-- usearch_key is blake3(entity_id) truncated to i64,
-- used to map usearch search results back to entity_ids.
CREATE TABLE embeddings (
  entity_id TEXT PRIMARY KEY,
  model TEXT NOT NULL,
  dimensions INTEGER NOT NULL,
  updated_at TEXT NOT NULL,
  usearch_key INTEGER
);
```

## Grafeo Data Model

In the grafeo backend, the same logical model is stored as labeled property
graph nodes and edges using GQL:

- **`:entity`** nodes with properties: `eid`, `kind`, `name`, `component_id`,
  `path`, `language`, `line_start`, `line_end`, `visibility`, `exported`
- **`:file`** nodes with properties: `path`, `component_id`, `kind`, `hash`,
  `indexed`, `ignore_reason`
- **`:summary`** nodes with properties: `entity_id`, `short_summary`,
  `detailed_summary`, `keywords_json`, `updated_at`, `source_hash`
- **`:embedding`** nodes with properties: `entity_id`, `model`, `dimensions`,
  `vector` (native vector type), `updated_at`
- **`:task_route`** nodes with properties: `task_name`, `entity_id`, `priority`
- **Edges** between `:entity` nodes use dynamic relationship types matching
  the edge kinds (`contains`, `depends_on`, etc.) with optional `provenance_path`
  and `provenance_line` properties.

Vector search uses a native HNSW index:
```sql
CREATE VECTOR INDEX idx_embedding_vector ON :embedding(vector) METRIC 'cosine'
```

## What Drives The Graph

The graph model should be driven by the questions an agent needs to answer
without rereading the whole repo:

- what component owns this behavior?
- what file should I read first?
- what tests or docs support this module?
- what deployable or infra root is connected to this code?
- what is the next best neighboring node to inspect?

ASTs, manifests, docs, and task metadata are inputs. Agent navigation needs are
the design driver.

## Component Identity And Ownership

The graph needs a strict ownership model. Component identity is not a display
concern. It is the key used by indexing, summarization, retrieval, and cleanup.

### Canonical Component Identity

- Every discovered component root gets exactly one canonical `component_id`.
- The canonical id is derived from the repo-relative component root path, not
  from the manifest display name.
- The id should be namespaced by discovery source when needed, for example:
  - `component::cargo::crates/chizu-core`
  - `component::npm::packages/web`
- Directory basename and manifest `name` are not safe identifiers on their own.
  They are too easy to collide and too easy to change independently of the root.

### Names, Aliases, And Lookup

- The component entity should still keep a human-facing `name`.
- If a manifest provides a package name, that value should be stored as metadata
  and treated as an alias for local dependency resolution.
- Local dependency resolution should map manifest aliases back to the canonical
  path-rooted `component_id`.
- External dependencies should not be silently collapsed into local component
  ids just because a name matches.

### Ownership Propagation

- Component discovery should happen before file/entity extraction.
- After roots are discovered, every file under a component root inherits that
  component's canonical `component_id`.
- Every file-backed entity derived from that file inherits the same
  `component_id`.
- A file or entity should never be indexed under both a directory-derived
  component id and a manifest-derived component id.

### Incremental Convergence

- Re-indexing must converge to the same graph as a fresh index.
- If a file is changed, renamed, moved, or deleted, cleanup must remove:
  - file records
  - file-backed entities
  - incoming and outgoing edges for those entities
  - summaries
  - embeddings
  - task routes
- Cleanup must also remove orphaned directory and component nodes that no longer
  correspond to discovered roots or retained files.

### Query Signal Discipline

- Query and rerank logic may only depend on signals that indexing actually
  materializes.
- If task routes are a first-class ranking signal, they must be generated
  deterministically during indexing.
- If a signal is defined in the conceptual model but not emitted, it should not
  affect ranking.

## Initial Indexing Flow

1. discover repo roots and component boundaries
2. parse source-system metadata such as Cargo and `mise.toml`
3. extract local structure from language adapters
4. attach docs, tests, benches, commands, and infra links
5. generate task routes
6. generate summaries
7. optionally build vectors over summaries

## Initial Query Flow

1. classify the query
2. prefilter using component, task, and exact-match signals
   (SQL for sqlite+usearch, GQL for grafeo)
3. run optional vector lookup over summaries
   (usearch HNSW for sqlite+usearch, grafeo HNSW for grafeo)
4. expand one hop in the graph
5. rerank using graph-aware signals
6. return a reading plan

## Design Constraint

The first version should prefer correct ownership and task routing over deep
semantic extraction.
