# Chizu PRD

**Status:** v1 complete
**Date:** 2026-03-23

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

Chizu should be a local component graph indexer for repositories.

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

## Product Invariants

These rules define correctness at the product level. The exact encoding and
storage details belong in the graph-model spec.

- Chizu must assign exactly one canonical component identity to each discovered
  component root.
- Manifest names and package names are labels and lookup aliases, not alternate
  component identities.
- Files and file-backed entities inside a component must inherit the canonical
  `component_id` of their enclosing component root.
- Incremental re-indexing must converge. Renames, moves, and deletions must not
  leave stale ownership, stale graph edges, or stale summaries/embeddings behind.
- Query and rerank signals must be backed by data actually produced during
  indexing. Chizu should not rely on theoretical signals that are defined in the
  model but not materialized in the graph.

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

Ownership correctness matters more than representational breadth in v1. If the
system has to choose, it should prefer a smaller model with stable component
identity over a broader model with ambiguous ownership.

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
| `deployable` | Dockerfile, docker-compose service | Filesystem scanner | Implemented |
| `infra_root` | Terraform root module (directory with `main.tf`) | Terraform scanner | Implemented |
| `command` | Ansible playbook | Ansible scanner | Implemented |
| `content_page` | Frontmatter markdown content (blog post, article, course page) | Frontmatter parser | Implemented |
| `template` | Rendering template (HTML, Hugo layout, `.astro` component) | Filesystem scanner | Implemented |
| `site` | Website deployment unit (domain + SSG + infra + content) | Site detector | Implemented |
| `migration` | SQL schema migration file | Filesystem scanner | Implemented |
| `spec` | Formal specification (TLA+) | Filesystem scanner | Implemented |
| `workflow` | Agent workflow definition (`.toml`, `.yml`) | Filesystem scanner | Implemented |
| `agent_config` | Agent instruction file (AGENTS.md, CLAUDE.md, SKILL.md) | Filesystem scanner | Implemented |

### Edge Kinds

| Kind | Typical src → dst | Status |
|------|-------------------|--------|
| `contains` | Component → SourceUnit, Repo → Component, Site → ContentPage | Implemented |
| `defines` | SourceUnit → Symbol | Implemented |
| `depends_on` | Component → Component | Implemented |
| `reexports` | SourceUnit → Symbol | Implemented |
| `documented_by` | Component → Doc, Site → Doc | Implemented |
| `tested_by` | SourceUnit → Test | Implemented |
| `benchmarked_by` | SourceUnit → Bench | Implemented |
| `related_to` | Any → Any | Defined, not emitted |
| `configured_by` | Component → Feature, Repo → AgentConfig | Implemented |
| `builds` | Task → Deployable | Defined, not emitted |
| `deploys` | Site → InfraRoot | Implemented |
| `implements` | Impl → Trait | Implemented |
| `owns_task` | Repo → Task | Implemented |
| `declares_feature` | Component → Feature | Implemented |
| `feature_enables` | Feature → Feature | Implemented |
| `mentions` | Doc → Symbol, ContentPage → Symbol | Defined, not emitted |
| `migrates` | Repo → Migration | Defined, used for containment |
| `specifies` | Repo → Spec | Defined, used for containment |
| `renders` | Template → ContentPage, Template → Site | Defined, not emitted |

## Adapters

### Implemented

| Adapter | Entities produced | Source |
|---------|-------------------|--------|
| Rust AST | `source_unit`, `symbol`, `test`, `bench` | tree-sitter parse of `.rs` files |
| Cargo metadata | `repo`, `component`, `feature` | `cargo_metadata` crate |
| Docs | `doc` | `.md` files in root, crate dirs, `docs/` |
| mise.toml | `task` | TOML task definitions |
| Terraform scanner | `infra_root` | Directories containing `main.tf` |
| Dockerfile scanner | `deployable` | `Dockerfile*`, `docker-compose*.yml` |
| Ansible scanner | `command` | `**/playbooks/*.yml` |
| Frontmatter parser | `content_page` | Markdown with TOML/YAML frontmatter in content dirs |
| Template scanner | `template` | `templates/**/*.html`, `layouts/**/*.html`, `*.astro` |
| Site detector | `site` | `site.toml`, `astro.config.*`, Hugo `config.toml` |
| Migration scanner | `migration` | `**/migrations/*.sql` |
| Spec scanner | `spec` | `**/*.tla` |
| Workflow scanner | `workflow` | `**/workflows/*.{toml,yml,yaml}` |
| Agent config scanner | `agent_config` | `CLAUDE.md`, `AGENTS.md`, `SKILL.md` |

### Future

- Astro component graph (imports, props, slots)
- TypeScript package graphs
- C# project/solution files
- Additional build systems
- Non-Rust workspace discovery (index sites/repos without Cargo.toml)
- `mentions` edge emission (parse markdown for code references)
- `renders` edge emission (link templates to content pages)
- `builds` edge emission (link tasks to deployables)

## Query Model

The query pipeline is a five-stage process:

1. classify the task
2. prefilter with SQL and task routes
3. optionally run semantic lookup over summaries
4. expand neighbors in the graph
5. rerank and return a reading plan

The output tells an agent which files or components to read first and why.

### Task Categories

| Category | Route names | Preferred entity kinds |
|----------|-------------|----------------------|
| understand | understand, architecture | Component, SourceUnit, Doc, Symbol, ContentPage, AgentConfig |
| debug | debug, fix | SourceUnit, Symbol, Test, Spec |
| build | build, implement | Component, SourceUnit, Symbol, Feature, Template, Migration |
| test | test, bench | Test, Bench, SourceUnit, Spec |
| deploy | deploy, release | Deployable, InfraRoot, Task, Command, Site |
| configure | configure, setup | Component, Feature, InfraRoot, AgentConfig, Workflow |
| general | (none) | Component, SourceUnit, Symbol |

### Task Route Heuristics

Entities are assigned to task routes with priority scores during indexing:

| Entity Kind | Routes (task → priority) |
|-------------|--------------------------|
| Component | understand(80), architecture(80), build(70), implement(70) |
| SourceUnit (mod.rs/lib.rs) | understand(60), architecture(60), debug(50), fix(50), build(40), implement(40) |
| SourceUnit (other) | understand(30), architecture(30), debug(50), fix(50), build(40), implement(40) |
| Doc | understand(70), architecture(70) |
| Test | test(80), bench(40), debug(60), fix(60) |
| Bench | test(40), bench(80) |
| Symbol (pub) | build(50), implement(50) |
| Task (deploy/release/ci) | deploy(80), release(80) |
| Task (test) | test(70), bench(40) |
| Task (build) | build(70), implement(40) |
| Deployable | deploy(80), release(80) |
| Feature | configure(70), setup(70) |
| InfraRoot | deploy(80), release(80), configure(60) |
| Command | deploy(70), configure(60) |
| ContentPage | understand(60), build(40) |
| Template | build(60), understand(40) |
| Site | understand(70), deploy(70), build(60) |
| Migration | build(60), debug(50) |
| Spec | understand(70), test(60), debug(50) |
| Workflow | configure(60), build(40) |
| AgentConfig | configure(70), understand(60) |

Cross-cutting: entities with "config" in name or path also get configure(60),
setup(60).

## Watch Mode

Chizu supports incremental re-indexing and filesystem watching to keep the
graph up to date as the codebase evolves.

### Incremental Re-indexing

`index_project` computes a BLAKE3 hash for each source file and compares it
against the stored hash. Unchanged files are skipped entirely, making
re-indexing fast even on large workspaces.

### Filesystem Watching

The `watch` command monitors the workspace and automatically re-indexes when
files change:

```
chizu watch [path] [--debounce-ms 500]
```

Behavior:

- Runs an initial full index on startup
- Uses OS-native filesystem notifications (via `notify`) to detect changes
- Debounces rapid saves — coalesces events within a configurable window
  (default 500 ms) before triggering a re-index
- Filters to relevant file types: `.rs`, `.toml`, `.md`, `.tf`, `.tla`,
  `.astro`, `.sql`, `.yml`, `.yaml`, `.html`, `Dockerfile`
- Ignores `target/`, `.git/`, `node_modules/`, and database files (`*.db`)
- Re-runs `index_project` on each trigger (unchanged files are skipped via
  hash comparison)
- Prints index stats after each cycle
- Runs until interrupted with Ctrl+C

## Reading Plan Generation

The `plan` command implements a multi-stage query pipeline that produces a
ranked reading plan telling an agent which files and entities to read first.

```
chizu plan "how does the store work" [--limit 15] [--format text|json]
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
   benchmarked_by, documented_by, reexports, configured_by, related_to,
   migrates, specifies, renders, deploys, builds). Capped at 5 neighbors per
   seed, deduped against seeds and other neighbors.

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
