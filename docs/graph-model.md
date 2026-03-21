# Graph Model

**Status:** Draft
**Date:** 2026-03-21

## Purpose

This document describes the initial graph model for Grakno.

The model is designed to be:

- local
- explainable
- SQLite-backed
- useful without embeddings
- broad enough for both Harrow and Panzerotti

## Three Layers

Grakno separates three concerns:

- graph = structure
- summaries = fast descriptions of nodes
- embeddings = optional fuzzy lookup over summaries

These should not be collapsed into one mechanism.

## Storage Layout

```text
.agent/
  index.sqlite
  build.json
  config.toml
  vectors.usearch
```

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

## SQLite Schema

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
  updated_at TEXT NOT NULL
);

CREATE TABLE task_routes (
  task_name TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  priority INTEGER NOT NULL,
  PRIMARY KEY (task_name, entity_id)
);

CREATE TABLE embeddings (
  entity_id TEXT PRIMARY KEY,
  model TEXT NOT NULL,
  dimensions INTEGER NOT NULL,
  vector_ref TEXT,
  updated_at TEXT NOT NULL
);
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
2. prefilter with SQL using component, task, and exact-match signals
3. run optional vector lookup over summaries
4. expand one hop in the graph
5. rerank using graph-aware signals
6. return a reading plan

## Design Constraint

The first version should prefer correct ownership and task routing over deep
semantic extraction.
