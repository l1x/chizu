# Chizu PRD

**Date:** 2026-03-29

## Problem

Coding agents repeatedly pay a repo-discovery tax. They spend too much time
listing files, rediscovering component boundaries, reopening the same
entrypoint files, and re-deriving relationships between code, docs, tests,
build tasks, deployables, and infrastructure.

The missing piece is not just semantic search. The missing piece is a local,
explicit, queryable repository fact model that can route a subject to the most
relevant files and components.

## Thesis

Chizu is a local repository understanding engine for subject-to-file routing.

It combines:

1. deterministic extraction of structural repository facts
2. graph materialization for human visualization and graph-based navigation
3. lexical, route-based, and semantic retrieval over those facts
4. ranked reading plans that tell an agent what to read next

The source of truth is the extracted repository facts: components, files,
symbols, docs, infra units, and their relationships. Chizu materializes those
facts as a graph for visualization and expansion, and as retrieval features for
subject-to-file routing.

## Goals

- Reduce agentic file reading by routing subjects to the most relevant files and
  components quickly.
- Maintain deterministic, reproducible structural facts for a given repo
  snapshot and configuration.
- Combine structural, lexical, and semantic signals for high-quality retrieval.
- Support mixed-language repos through adapters and configuration.

## Non-Goals

- Replacing normal file reads entirely
- Becoming a general-purpose graph database
- Full whole-program call graph precision
- Making graph traversal the primary end-user workflow for agents

## Product Invariants

These rules define correctness at the product level.

- Chizu must assign exactly one canonical component identity to each discovered
  component root.
- Manifest names and package names are labels and lookup aliases, not alternate
  component identities.
- Files and file-backed entities inside a component must inherit the canonical
  `component_id` of their enclosing component root.
- Structural fact extraction must be deterministic for the same repo snapshot
  and configuration. The same inputs must produce the same ownership, entities,
  and edges.
- Incremental re-indexing must converge. Renames, moves, and deletions must not
  leave stale ownership, stale graph edges, or stale summaries/embeddings behind.
- Query and rerank signals must be backed by data actually produced during
  indexing. Chizu should not rely on theoretical signals that are defined in the
  model but not materialized in extracted facts or derived indexes.
- Graph edges, summaries, task routes, and embeddings are derived projections
  over extracted facts. They can improve navigation and retrieval, but they must
  not redefine canonical ownership or file-to-component identity.
- Query classification, summarization, embeddings, and reranking may be
  heuristic or probabilistic, but the entities they point to must come from
  deterministic extracted facts.

## Architecture

```text
source inputs
  -> adapters
  -> deterministic fact extraction
  -> fact store (sqlite)
  -> derived projections
       - graph relationships / traversal
       - summaries
       - task routes
       - vector index (usearch HNSW)
  -> query / expansion / rerank
  -> reading plan
```

The store backend is **sqlite+usearch**. SQLite stores canonical repo facts and
their derived metadata (entities, edges, files, summaries, task routes,
embedding metadata). usearch provides HNSW-based approximate nearest neighbor
search over embedding vectors. Vectors live only in usearch, not duplicated in
SQLite.

### Storage Layout

```text
.chizu/
  graph.db             # graph, summaries, embedding metadata, task routes
  graph.db.usearch     # usearch HNSW index (vectors only)
```

## Core Model

Ownership correctness and deterministic fact extraction matter more than
representational breadth. The system should prefer a smaller model with stable
component identity over a broader model with ambiguous ownership. The graph is
one projection over those facts, not the product endpoint by itself.

### Component Identity

The graph uses a strict ownership model. Component identity is the key used by
indexing, summarization, retrieval, and cleanup.

- Every discovered component root gets exactly one canonical `component_id`.
- The canonical id is derived from the repo-relative component root path, not
  from the manifest display name.
- The id is namespaced by ecosystem:
  - `component::cargo::crates/chizu-core`
  - `component::npm::packages/web`
  - `component::npm::.` (root package)
- Directory basename and manifest `name` are not safe identifiers on their own.
  They collide too easily and change independently of the root.
- The component entity keeps a human-facing `name` from the manifest.
- Manifest names are treated as aliases for local dependency resolution. Local
  dependency resolution maps aliases back to the canonical path-rooted
  `component_id`.
- External dependencies use a separate namespace (`external::npm::{name}`) and
  are never conflated with local component IDs.

### Ownership Propagation

- Component discovery happens before file/entity extraction (two-phase
  indexing).
- After roots are discovered, every file under a component root inherits that
  component's canonical `component_id`.
- Every file-backed entity derived from that file inherits the same
  `component_id`.

### Entity Kinds

| Kind           | Description                                              |
| -------------- | -------------------------------------------------------- |
| `repo`         | Top-level repository root                                |
| `directory`    | Filesystem directory in the project tree                 |
| `component`    | Build-defined unit: Cargo crate, npm package             |
| `source_unit`  | Individual source file                                   |
| `symbol`       | Function, struct, enum, trait, impl, const, macro, class, interface |
| `doc`          | Documentation file (README, PRD, design doc, changelog)  |
| `test`         | Test function (`#[test]`, test files)                    |
| `bench`        | Benchmark function (`#[bench]`)                          |
| `task`         | Build/dev task from mise.toml or similar                 |
| `feature`      | Cargo feature flag                                       |
| `containerized`| Dockerfile or docker-compose definition                  |
| `infra_root`   | Terraform root (directory containing `main.tf`)          |
| `command`      | Ansible playbook or similar automation command           |
| `content_page` | Markdown with frontmatter in content directories         |
| `template`     | HTML/Astro template file                                 |
| `site`         | Site root (Astro, Hugo, etc.)                            |
| `migration`    | SQL migration file                                       |
| `spec`         | TLA+ specification                                       |
| `workflow`     | CI/CD workflow definition                                |
| `agent_config` | Agent configuration file (CLAUDE.md, AGENTS.md)          |

### Edge Kinds

| Kind               | Typical src -> dst                                       |
| ------------------ | -------------------------------------------------------- |
| `contains`         | Component -> SourceUnit, Repo -> Component, Site -> ContentPage |
| `defines`          | SourceUnit -> Symbol                                     |
| `depends_on`       | Component -> Component                                   |
| `reexports`        | SourceUnit -> Symbol                                     |
| `documented_by`    | Component -> Doc, Site -> Doc                            |
| `tested_by`        | SourceUnit -> Test                                       |
| `benchmarked_by`   | SourceUnit -> Bench                                      |
| `related_to`       | Any -> Any                                               |
| `configured_by`    | Component -> Feature, Repo -> AgentConfig                |
| `builds`           | Task -> Containerized                                    |
| `deploys`          | Site -> InfraRoot                                        |
| `implements`       | Impl -> Trait                                            |
| `owns_task`        | Repo -> Task                                             |
| `declares_feature` | Component -> Feature                                     |
| `feature_enables`  | Feature -> Feature                                       |
| `migrates`         | Repo -> Migration                                        |
| `specifies`        | Repo -> Spec                                             |
| `renders`          | Template -> ContentPage, Template -> Site                |

### SQLite Schema (v4)

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

## Adapters

| Adapter              | Entities produced                        | Source                                                |
| -------------------- | ---------------------------------------- | ----------------------------------------------------- |
| Rust AST             | `source_unit`, `symbol`, `test`, `bench` | tree-sitter parse of `.rs` files                      |
| Cargo metadata       | `repo`, `component`, `feature`           | Cargo.toml / `cargo_metadata` crate                   |
| package.json         | `component`                              | package.json name, workspaces, dependencies            |
| Docs                 | `doc`                                    | `.md` files in root, crate dirs, `docs/`              |
| mise.toml            | `task`                                   | TOML task definitions                                 |
| Terraform scanner    | `infra_root`                             | Directories containing `main.tf`                      |
| Dockerfile scanner   | `containerized`                          | `Dockerfile*`, `docker-compose*.yml`                  |
| Ansible scanner      | `command`                                | `**/playbooks/*.yml`                                  |
| Frontmatter parser   | `content_page`                           | Markdown with TOML/YAML frontmatter in content dirs   |
| Template scanner     | `template`                               | `templates/**/*.html`, `layouts/**/*.html`, `*.astro` |
| Site detector        | `site`                                   | `site.toml`, `astro.config.*`, Hugo `config.toml`     |
| Migration scanner    | `migration`                              | `**/migrations/*.sql`                                 |
| Spec scanner         | `spec`                                   | `**/*.tla`                                            |
| Workflow scanner     | `workflow`                               | `**/workflows/*.{toml,yml,yaml}`                      |
| Agent config scanner | `agent_config`                           | `CLAUDE.md`, `AGENTS.md`, `SKILL.md`                  |

## Indexing

### Two-Phase Approach

1. **Discovery phase**: Walk the directory tree and identify all component roots
   (Cargo.toml, package.json). Build a `ComponentRegistry` mapping root paths to
   canonical component IDs and manifest display names to canonical IDs
   (ecosystem-scoped).
2. **Indexing phase**: Walk the tree again. For each file, look up the enclosing
   component in the registry and propagate the canonical `component_id` to the
   file record and all entities derived from that file.

### Incremental Re-indexing

Each source file is hashed with BLAKE3. On re-index, unchanged files are
skipped. If a file's content is unchanged but its component assignment changed
(e.g., a new manifest appeared in a parent directory), the file is reindexed.

### Cleanup

- **Deleted files**: remove file records, file-backed entities, edges,
  summaries, embeddings, and task routes.
- **Orphaned components**: remove component entities that no longer correspond
  to discovered roots.
- **Renamed/moved components**: when a component root path changes (directory
  rename or move), the old component and all its associated data -- entities,
  edges, summaries, embeddings, task routes, and file records -- are removed
  entirely. The component at the new path is treated as a new component and
  fully re-indexed from scratch. This avoids partial-state bugs from attempting
  to migrate identity across paths.

## Query Model

The query pipeline is a five-stage process:

1. classify the task
2. prefilter with SQL and task routes
3. run lexical and semantic retrieval over indexed representations
4. expand neighbors in the graph
5. rerank and return a reading plan

The primary output is a ranked reading plan telling an agent which files or
components to read first and why. The graph supports expansion and human
navigation, but it is not the end product.

### Task Categories

| Category   | Route names              | Preferred entity kinds                                       |
| ---------- | ------------------------ | ------------------------------------------------------------ |
| understand | understand, architecture | Component, SourceUnit, Doc, Symbol, ContentPage, AgentConfig |
| debug      | debug, fix               | SourceUnit, Symbol, Test, Spec                               |
| build      | build, implement         | Component, SourceUnit, Symbol, Feature, Template, Migration  |
| test       | test, bench              | Test, Bench, SourceUnit, Spec                                |
| deploy     | deploy, release          | Containerized, InfraRoot, Task, Command, Site                |
| configure  | configure, setup         | Component, Feature, InfraRoot, AgentConfig, Workflow         |
| general    | (none)                   | Component, SourceUnit, Symbol                                |

### Task Route Heuristics

Entities are assigned to task routes with priority scores during indexing:

| Entity Kind                  | Routes (task -> priority)                                                      |
| ---------------------------- | ------------------------------------------------------------------------------ |
| `Component`                  | understand(80), architecture(80), build(70), implement(70)                     |
| `SourceUnit (mod.rs/lib.rs)` | understand(60), architecture(60), debug(50), fix(50), build(40), implement(40) |
| `SourceUnit (other)`         | understand(30), architecture(30), debug(50), fix(50), build(40), implement(40) |
| `Symbol`                     | build(50), implement(50)                                                       |
| `Test`                       | test(80), bench(40), debug(60), fix(60)                                        |
| `Doc`                        | understand(70), architecture(70)                                               |
| `ContentPage`                | understand(60), build(40)                                                      |
| `Template`                   | build(60), understand(40)                                                      |
| `Site`                       | understand(70), deploy(70), build(60)                                          |
| `Migration`                  | build(60), debug(50)                                                           |
| `Spec`                       | understand(70), test(60), debug(50)                                            |
| `Workflow`                   | configure(60), build(40)                                                       |
| `AgentConfig`                | configure(70), understand(60)                                                  |
| `Feature`                    | configure(70), setup(70)                                                       |
| `Task (deploy/release/ci)`   | deploy(80), release(80)                                                        |
| `Task (test)`                | test(70), bench(40)                                                            |
| `Task (build)`               | build(70), implement(40)                                                       |

Cross-cutting: entities with "config" in name or path also get configure(60), setup(60).

### Rerank Weights

Multi-signal weighted scoring. Default weights (configurable via `.chizu.toml`):

| Signal          | Weight | Notes                                                   |
| --------------- | ------ | ------------------------------------------------------- |
| task_route      | 0.00   | Zeroed until task route generation is fully implemented  |
| keyword         | 0.25   |                                                         |
| name_match      | 0.20   |                                                         |
| vector          | 0.25   | Semantic similarity over entity embeddings              |
| kind_preference | 0.10   |                                                         |
| exported        | 0.10   |                                                         |
| path_match      | 0.10   |                                                         |

Weights must sum to 1.0. Neighbor/context entities receive a 50% discount.

## CLI Surface

Chizu exposes a flat 9-command CLI:

- `index` always runs the full indexing pipeline: parse, summarize, and embed.
- `search` is the single natural-language entry point and returns a ranked
  reading plan.
- `entity`, `entities`, `routes`, and `edges` are top-level commands. `query`
  is no longer a parent command.
- `plan`, `inspect`, `watch`, `embed`, and `summarize` are removed from the
  public CLI surface.

| Command     | Description                         | Key flags |
| ----------- | ----------------------------------- | --------- |
| `index`     | Parse graph + summarize + embed     | `--force` |
| `search`    | Full query pipeline -> reading plan | `--limit`, `--category`, `--format`, positional query |
| `entity`    | Look up a single entity by id       | positional id |
| `entities`  | List entities                       | `--component`, `--kind` |
| `routes`    | List task routes                    | `--task`, `--entity` |
| `edges`     | List edges                          | `--from`, `--to`, `--rel` |
| `visualize` | Generate SVG or interactive HTML    | `--entity-id`, `--depth`, `--kind`, `--exclude`, `--interactive`, `--max-nodes`, `--output` |
| `config`    | Initialize or validate config       | subcommands: `init`, `validate` |
| `guide`     | Interactive usage guide             | none |

`visualize` defaults to static SVG output. `visualize --interactive` emits a
self-contained HTML tree explorer over the same focused graph slice.

### Command Migration

| Old command | New command | Notes |
| ----------- | ----------- | ----- |
| `query entity <id>` | `entity <id>` | `entity` absorbs detailed inspection output |
| `query entities` | `entities` | Flattened |
| `query routes` | `routes` | Flattened |
| `query edges` | `edges` | Flattened |
| `plan` | `search` | `search` now owns the reading-plan pipeline |
| `search` | `search` | Semantic search is folded into the full pipeline |
| `inspect` | `entity` | Removed as a separate command |
| `embed` | `index` | Embeddings are generated during indexing |
| `summarize` | `index` | Summaries are generated during indexing |
| `watch` | removed | No direct replacement |
| `query` | removed | No longer a parent command |

## Search Command

The `search` command produces a ranked reading plan telling an agent which
files and entities to read first.

```
chizu --repo . search "how does the store work" [--limit 15] [--category understand|debug|build|test|deploy|configure|general] [--format text|json]
```

`search` keeps the task-oriented pipeline from the old `plan` command and drops
per-invocation provider settings such as `--base-url`, `--api-key`, and
`--model`. Model and provider selection come from configuration.

### Pipeline Stages

1. **Classify** -- Heuristic keyword matching assigns a `TaskCategory`.
2. **Retrieve** -- Three retrieval sources merged by entity id: task route
   prefilter, keyword/name/path matching, vector search when available.
3. **Expand** -- 1-hop graph traversal from each seed candidate. Capped at 5
   neighbors per seed, deduped.
4. **Rerank** -- Multi-signal weighted scoring (see Rerank Weights above).
5. **Reading Plan** -- Scored entries assembled into a serializable
   `ReadingPlan`.

### Determinism Boundary

Determinism applies to structural fact extraction, not to the full retrieval
stack.

- Deterministic: component discovery, canonical IDs, file ownership, entity
  extraction, edge extraction, cleanup, and the mapping from facts to retrievable
  units.
- Heuristic or probabilistic: query classification, summaries, embeddings,
  vector recall, and reranking.

### Embeddings and Summaries

Embeddings are a first-class retrieval signal, not the definition of repository
truth. `index` should generate summaries and embeddings as part of normal
indexing, and `search` should use them when available to improve recall and
ranking. There is no standalone `embed` or `summarize` command in the
simplified CLI.

Graph structure and vector retrieval are not treated as fully orthogonal
subsystems. Both can be derived from the same extracted facts, and both can
contribute to ranking the final reading plan.

If summary or embedding generation fails for some entities, `index` must report
that degradation clearly. `search` must still be able to run against the
deterministic fact index using available structural and lexical signals, while
indicating reduced retrieval quality.

## Configuration

All runtime configuration lives in `.chizu.toml` at the repository root.
Missing file means all defaults apply. Missing sections or keys fall back to
defaults individually.

```toml
[index]
exclude_patterns = [
    "**/target/**",
    "**/.git/**",
    "**/node_modules/**",
    "**/.venv/**",
    "**/fuzz/**",
    "**/*.lock",
]

[search]
default_limit = 15

[search.rerank_weights]
task_route = 0.00
keyword = 0.25
name_match = 0.20
vector = 0.25
kind_preference = 0.10
exported = 0.10
path_match = 0.10

[providers.ollama]
base_url = "http://localhost:11434/v1"
timeout_secs = 120
retry_attempts = 3

[summary]
provider = "ollama"
model = "llama3:8b"
max_tokens = 512
temperature = 0.2
batch_size = 4
concurrency = 1
exported_only = true

[embedding]
provider = "ollama"
model = "nomic-embed-text-v2-moe:latest"
dimensions = 768
batch_size = 32

[visualize]
# Optional: enable editor links in interactive HTML output
# editor_link = "vscode://file/{abs_path}:{line}:{column}"
```

### Configuration Design Rules

- Provider connection config (`base_url`, `timeout_secs`, `retry_attempts`) is
  defined once per provider under `[providers.<name>]`. The `[summary]` and
  `[embedding]` sections reference a provider by name.
- `api_key` defaults to empty string (local providers like Ollama need no key).
  Only specify when using a remote provider.
- `summary.exported_only` defaults to true so symbol summaries focus on
  exported Rust items unless explicitly overridden.
- `summary` and `embedding` currently must reference the same provider.
- Rerank weights must sum to 1.0. `task_route` stays at 0.00 until task route
  generation is fully implemented.
- `config validate` checks that weights sum to 1.0, referenced providers
  exist, the summary and embedding providers match, and required fields are
  present.

## Open Questions

- **Adapter priority and conflict resolution.** Multiple adapters can match the
  same file. A `.rs` file in a crate gets touched by both the Rust AST adapter
  and potentially the Cargo metadata adapter (if it's `lib.rs` or `main.rs`
  referenced in `[lib]`/`[[bin]]`). A `README.md` in a crate root could match
  both the Docs adapter and the Frontmatter parser (if it has frontmatter). The
  current design does not specify what happens when two adapters produce
  entities from the same file -- last-writer-wins, union, or adapter
  precedence. This needs a resolution before the adapter set grows further.
