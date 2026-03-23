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

### Entity Kinds

| Kind | Description | Adapter | Status |
|------|-------------|---------|--------|
| `repo` | Top-level repository root | Cargo metadata | Implemented |
| `component` | Cargo crate, site subproject, or infra module | Cargo metadata | Implemented |
| `source_unit` | Single source file (`.rs`, `.astro`, `.ts`, `.html` template) | Rust AST, filesystem | Implemented (Rust only) |
| `symbol` | Function, struct, enum, trait, impl, const, macro | Rust AST | Implemented |
| `doc` | Documentation file (README, PRD, design doc, changelog) | Filesystem | Implemented |
| `test` | Test function (`#[test]`, test files) | Rust AST | Implemented |
| `bench` | Benchmark function or file | Rust AST | Implemented |
| `task` | Build/dev task from mise.toml or similar | mise.toml | Implemented |
| `feature` | Cargo feature flag | Cargo metadata | Implemented |
| `deployable` | Dockerfile, docker-compose service | Dockerfile parser | Not implemented |
| `infra_root` | Terraform root module (directory with `main.tf`) | Terraform scanner | Not implemented |
| `command` | Ansible playbook | Ansible scanner | Not implemented |
| `content_page` | Frontmatter markdown content (blog post, article, course page) | Frontmatter parser | Not implemented |
| `template` | Rendering template (HTML, Hugo layout, `.astro` component) | Filesystem | Not implemented |
| `site` | Website deployment unit (domain + SSG + infra + content) | site.toml / astro.config / config.toml | Not implemented |
| `migration` | SQL schema migration file | Filesystem | Not implemented |
| `spec` | Formal specification (TLA+) | Filesystem | Not implemented |
| `workflow` | Agent workflow definition (`.toml`) | Filesystem | Not implemented |
| `agent_config` | Agent instruction file (AGENTS.md, CLAUDE.md, SKILL.md) | Filesystem | Not implemented |

### Edge Kinds

| Kind | Typical src → dst | Status |
|------|-------------------|--------|
| `contains` | Component → SourceUnit, Repo → Component, Site → ContentPage | Implemented |
| `defines` | SourceUnit → Symbol | Implemented |
| `depends_on` | Component → Component | Implemented |
| `reexports` | SourceUnit → Symbol | Implemented |
| `documented_by` | Component → Doc, Site → Doc | Implemented |
| `tested_by` | SourceUnit → Test | Implemented |
| `benchmarked_by` | SourceUnit → Bench | Defined, not emitted |
| `related_to` | Any → Any | Defined, not emitted |
| `configured_by` | Component → Feature | Implemented |
| `builds` | Task → Deployable | Defined, not emitted |
| `deploys` | Deployable → InfraRoot, InfraRoot → Site | Defined, not emitted |
| `implements` | Impl → Trait | Implemented |
| `owns_task` | Repo → Task | Implemented |
| `declares_feature` | Component → Feature | Implemented |
| `feature_enables` | Feature → Feature | Implemented |
| `mentions` | Doc → Symbol, ContentPage → Symbol | Defined, not emitted |
| `migrates` | Migration → Component | Not defined |
| `specifies` | Spec → Component | Not defined |
| `renders` | Template → ContentPage, Template → Site | Not defined |

## Adapters

### Implemented (v0)

| Adapter | Entities produced | Source |
|---------|-------------------|--------|
| Rust AST | `source_unit`, `symbol`, `test`, `bench` | tree-sitter parse of `.rs` files |
| Cargo metadata | `repo`, `component`, `feature` | `cargo_metadata` crate |
| Docs | `doc` | `.md` files in root, crate dirs, `docs/` |
| mise.toml | `task` | TOML task definitions |

### Planned (v1)

| Adapter | Entities produced | Source |
|---------|-------------------|--------|
| Terraform scanner | `infra_root` | Directories containing `main.tf` |
| Dockerfile parser | `deployable` | `Dockerfile`, `docker-compose.yml` |
| Ansible scanner | `command` | Playbook `.yml` files |
| Frontmatter parser | `content_page` | Markdown with TOML/YAML frontmatter |
| Template scanner | `template` | `.html` templates, Hugo layouts, `.astro` components |
| Site detector | `site` | `site.toml`, `astro.config.*`, Hugo `config.toml` |
| Migration scanner | `migration` | `migrations/` directories with `.sql` files |
| Spec scanner | `spec` | `.tla` files |
| Workflow scanner | `workflow` | `.toml` workflow definitions |
| Agent config scanner | `agent_config` | `AGENTS.md`, `CLAUDE.md`, `SKILL.md` |

### Future

- Astro component graph (imports, props, slots)
- TypeScript package graphs
- C# project/solution files
- Additional build systems

## Query Model

The intended query flow is:

1. classify the task
2. prefilter with SQL and task routes
3. optionally run semantic lookup over summaries
4. expand neighbors in the graph
5. rerank and return a reading plan

The output should tell an agent which files or components to read first and
why.

## Watch Mode

Grakno supports incremental re-indexing and filesystem watching to keep the
graph up to date as the codebase evolves.

### Incremental Re-indexing

`index_project` computes a BLAKE3 hash for each source file and compares it
against the stored hash. Unchanged files are skipped entirely, making
re-indexing fast even on large workspaces.

### Filesystem Watching

The `watch` command monitors the workspace and automatically re-indexes when
files change:

```
grakno watch [path] [--debounce-ms 500]
```

Behavior:

- Runs an initial full index on startup
- Uses OS-native filesystem notifications (via `notify`) to detect changes
- Debounces rapid saves — coalesces events within a configurable window
  (default 500 ms) before triggering a re-index
- Filters to relevant file types: `.rs`, `.toml`, `.md`
- Ignores `target/`, `.git/`, and database files (`*.db`)
- Re-runs `index_project` on each trigger (unchanged files are skipped via
  hash comparison)
- Prints index stats after each cycle
- Runs until interrupted with Ctrl+C

## Reading Plan Generation

The `plan` command implements a multi-stage query pipeline that produces a
ranked reading plan telling an agent which files and entities to read first.

```
grakno plan "how does the store work" [--limit 15] [--format text|json]
    [--base-url URL --api-key KEY --model MODEL]
```

### Pipeline Stages

1. **Classify** — Heuristic keyword matching on the query text assigns a
   `TaskCategory` (understand, debug, build, test, deploy, configure, general).
   Each category maps to task route names for prefiltering and preferred entity
   kinds for rerank boosting.

2. **Retrieve** — Three retrieval sources are merged by entity id:
   - Task route prefilter from the classified category
   - Keyword, name, and path matching against tokenized query terms
   - Optional vector search when embedding options are provided

3. **Expand** — 1-hop graph traversal from each seed candidate, following
   useful edge kinds (contains, defines, depends_on, implements, tested_by,
   documented_by, reexports, configured_by, related_to). Capped at 5
   neighbors per seed, deduped against seeds and other neighbors.

4. **Rerank** — Multi-signal weighted scoring combining task route priority
   (0.30), keyword match (0.20), vector similarity (0.20), name match (0.15),
   kind preference (0.05), exported bonus (0.05), and path match (0.05).
   Neighbor/context entities receive a 50% discount.

5. **Reading Plan** — The scored entries are assembled into a `ReadingPlan`
   with ranked `ReadingPlanItem`s. The plan is serializable as JSON for agent
   consumption or displayed as human-readable text.

### Usage Without Embeddings

The pipeline works without any embedding/LLM configuration. When `--base-url`,
`--api-key`, and `--model` are all omitted, retrieval relies entirely on task
routes, keyword matching, and graph expansion. This keeps the system useful
without external API dependencies.
