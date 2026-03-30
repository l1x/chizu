# Chizu Guide for Agents

## What is Chizu?

Chizu is a **local code knowledge graph** that indexes a codebase into a
queryable graph database (SQLite + usearch). It helps agents understand code
relationships without opening files blindly.

```
Traditional workflow: grep -> open file -> read -> understand -> repeat
Chizu workflow:       ask question -> get ranked reading plan -> read what matters
```

## Quick Reference

```bash
# Index current directory
chizu index

# Natural language search
chizu search "how does authentication work"

# Structured output for programmatic use
chizu search "store layer" --format json --limit 5

# Inspect a specific entity
chizu entity "symbol::src/main.rs::validate_token"

# List entities in a component
chizu entities --component cargo::chizu-core

# List all test entities
chizu entities --kind test

# Explore relationships
chizu edges --from "component::cargo::chizu-core"

# Task-specific routes
chizu routes --task debug

# Generate SVG visualization
chizu visualize --entity-id "component::cargo::." --legend -o graph.svg
```

## Commands

| Command | Description | Key flags |
|---------|-------------|-----------|
| `index` | Parse repo, generate summaries + embeddings | `--force` |
| `search <query>` | Natural language search -> ranked reading plan | `--limit`, `--category`, `--format` (text/json) |
| `entity <id>` | Look up a single entity by ID | positional id |
| `entities` | List entities | `--component`, `--kind` |
| `routes` | List task route assignments | `--task`, `--entity` |
| `edges` | List edges | `--from`, `--to`, `--rel` |
| `visualize` | Generate SVG graph | `--entity-id`, `--depth`, `--kind`, `--exclude`, `--layout` (dot/neato/fdp), `--max-nodes`, `-o`, `--legend` |
| `config` | Initialize or validate config | subcommands: `init` (`-f`), `validate` |
| `guide` | Show usage guide | none |

All commands accept `--repo <path>` (defaults to current directory).

## Entity Types

| Kind | String | Description | Examples |
|------|--------|-------------|---------|
| Repo | `repo` | Repository root | |
| Directory | `directory` | Filesystem directory | |
| Component | `component` | Cargo crate, npm package | `component::cargo::chizu-core` |
| SourceUnit | `source_unit` | Individual source file | `source_unit::src/main.rs` |
| Symbol | `symbol` | Function, struct, enum, trait, impl | `symbol::src/lib.rs::Config` |
| Doc | `doc` | README, PRD, design doc | `doc::README.md` |
| Test | `test` | Test function | `test::src/lib.rs::test_roundtrip` |
| Bench | `bench` | Benchmark function | |
| Task | `task` | Build/dev task (mise.toml) | |
| Feature | `feature` | Cargo feature flag | |
| Containerized | `containerized` | Dockerfile, docker-compose | |
| InfraRoot | `infra_root` | Terraform directory | |
| Command | `command` | Automation command | |
| ContentPage | `content_page` | Markdown with frontmatter | |
| Template | `template` | HTML/Astro template | |
| Site | `site` | Site root (Astro, Hugo) | |
| Migration | `migration` | SQL migration file | |
| Spec | `spec` | TLA+ specification | |
| Workflow | `workflow` | CI/CD workflow definition | |
| AgentConfig | `agent_config` | CLAUDE.md, AGENTS.md | |

## Edge Types

| Kind | String | Typical usage |
|------|--------|---------------|
| Contains | `contains` | Component -> SourceUnit, Repo -> Component |
| Defines | `defines` | SourceUnit -> Symbol |
| DependsOn | `depends_on` | Component -> Component |
| Reexports | `reexports` | SourceUnit -> Symbol |
| DocumentedBy | `documented_by` | Component -> Doc |
| TestedBy | `tested_by` | SourceUnit -> Test |
| BenchmarkedBy | `benchmarked_by` | SourceUnit -> Bench |
| RelatedTo | `related_to` | Any -> Any |
| ConfiguredBy | `configured_by` | Component -> Feature, Repo -> AgentConfig |
| Builds | `builds` | Task -> Containerized |
| Deploys | `deploys` | Site -> InfraRoot |
| Implements | `implements` | Impl -> Trait |
| OwnsTask | `owns_task` | Repo -> Task |
| DeclaresFeature | `declares_feature` | Component -> Feature |
| FeatureEnables | `feature_enables` | Feature -> Feature |
| Migrates | `migrates` | Repo -> Migration |
| Specifies | `specifies` | Repo -> Spec |
| Renders | `renders` | Template -> ContentPage |

## Search Pipeline

Queries go through five stages:

1. **Classify** -- heuristic keyword match into a TaskCategory
   (`understand`, `debug`, `build`, `test`, `deploy`, `configure`, `general`)
2. **Retrieve** -- three parallel signals merged:
   - Task routes (priority 0-100 per entity per task)
   - Keyword search against summaries
   - Name/path substring match + vector nearest-neighbor search
3. **Score** -- weighted sum of signals (weights configurable in `.chizu.toml`)
4. **Expand** -- add 1-hop graph neighbors as context (discounted 0.5x)
5. **Rerank** -- final weighted scoring, output as a ranked ReadingPlan

Override the auto-classified category with `--category`:

```bash
chizu search "auth middleware" --category debug
```

## Configuration

All config lives in `.chizu.toml` at the repo root. Generate defaults with
`chizu config init`.

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

[embedding]
provider = "ollama"
model = "nomic-embed-text-v2-moe:latest"
dimensions = 768
batch_size = 32
```

Provider connection is defined once under `[providers.<name>]`. The `[summary]`
and `[embedding]` sections reference a provider by name. Rerank weights must
sum to 1.0.

## Storage

Indexing creates `.chizu/` in the repo root:

| File | Contents |
|------|----------|
| `graph.db` | SQLite database (entities, edges, files, summaries, task routes, embedding metadata) |
| `graph.db.usearch` | usearch HNSW vector index (embedding vectors only) |

Schema version: **4**. Tables: `files`, `entities`, `edges`, `summaries`,
`task_routes`, `embeddings`.

### Direct SQL access

```bash
sqlite3 .chizu/graph.db "SELECT id, kind, name FROM entities WHERE kind='symbol' LIMIT 10;"
sqlite3 .chizu/graph.db "SELECT rel, COUNT(*) FROM edges GROUP BY rel;"
sqlite3 .chizu/graph.db "SELECT * FROM entities WHERE name LIKE '%handler%';"
```

## Project Layout

```
chizu/
  chizu-cli/             CLI binary (command dispatch, output formatting, SVG generation)
  chizu-core/            Core types, storage, config, LLM provider, query classifier
  chizu-index/           Indexing pipeline and language adapters
  chizu-query/           Search pipeline (retrieval, expansion, reranking, reading plan)
  docs/                  Documentation (brief, PRD, article)
```

## Key Files

| File | Purpose |
|------|---------|
| `chizu-cli/src/main.rs` | CLI entry point and command dispatch |
| `chizu-core/src/config/mod.rs` | TOML configuration parsing and validation |
| `chizu-core/src/model/entity.rs` | Entity struct |
| `chizu-core/src/model/entity_kind.rs` | EntityKind enum (20 kinds) |
| `chizu-core/src/model/edge_kind.rs` | EdgeKind enum (18 kinds) |
| `chizu-core/src/model/id.rs` | Canonical ID construction |
| `chizu-core/src/provider/openai.rs` | OpenAI-compatible HTTP provider (completions + embeddings) |
| `chizu-core/src/query/classifier.rs` | TaskCategory classification heuristics |
| `chizu-core/src/store/sqlite.rs` | SQLite schema and queries |
| `chizu-core/src/store/usearch.rs` | usearch HNSW vector index wrapper |
| `chizu-index/src/indexer.rs` | Main IndexPipeline orchestration |
| `chizu-index/src/summarizer.rs` | LLM-based summary generation |
| `chizu-index/src/embedder.rs` | Vector embedding generation |
| `chizu-index/src/walk.rs` | File walker with exclusion patterns |
| `chizu-index/src/ownership.rs` | Component ownership assignment |
| `chizu-index/src/task_routes.rs` | Task route generation |
| `chizu-index/src/cleanup.rs` | Stale entity/edge cascade deletion |
| `chizu-index/src/adapter/rust.rs` | Tree-sitter Rust extraction (fn, struct, enum, trait, impl, test, bench) |
| `chizu-index/src/adapter/cargo.rs` | Cargo workspace and dependency parsing |
| `chizu-index/src/adapter/npm.rs` | npm workspace parsing |
| `chizu-index/src/adapter/doc.rs` | Documentation file extraction |
| `chizu-index/src/adapter/site.rs` | Static site extraction (Astro, Hugo) |
| `chizu-index/src/adapter/mise.rs` | mise.toml task extraction |
| `chizu-index/src/adapter/scanner.rs` | Generic fallback file classification |
| `chizu-index/src/adapter/frontmatter.rs` | Frontmatter parsing for markdown files |
| `chizu-query/src/pipeline.rs` | SearchPipeline (classify -> retrieve -> expand -> rerank -> plan) |
| `chizu-query/src/retrieval.rs` | Multi-signal candidate retrieval |
| `chizu-query/src/rerank.rs` | Weighted multi-signal scoring |
| `chizu-query/src/expansion.rs` | Graph neighbor expansion |
| `chizu-query/src/plan.rs` | ReadingPlan output formatting |

## Working with the Code

### Adding a new language adapter

1. Add the tree-sitter dependency in `chizu-index/Cargo.toml`:
```toml
tree-sitter-python = "0.23"
```

2. Create `chizu-index/src/adapter/python.rs` implementing extraction logic.

3. Register the adapter in `chizu-index/src/adapter/mod.rs`.

4. Wire it into the indexer in `chizu-index/src/indexer.rs`.

### Adding a new entity kind

1. Add the variant to `EntityKind` in `chizu-core/src/model/entity_kind.rs`
2. Add Display and FromStr arms in the same file
3. Emit entities of that kind from the relevant adapter

### Adding a new edge kind

1. Add the variant to `EdgeKind` in `chizu-core/src/model/edge_kind.rs`
2. Add Display and FromStr arms in the same file
3. Create edges of that kind from the relevant adapter

### Testing

```bash
cargo test                   # all tests
cargo test -p chizu-core     # core crate only
cargo test -p chizu-index    # index crate only
cargo test -p chizu-query    # query crate only
```

### Debug logging

```bash
RUST_LOG=debug chizu index
RUST_LOG=debug chizu search "how does auth work"
```

## Design Principles

1. **Deterministic facts, heuristic retrieval** -- structural extraction is
   reproducible; summaries, embeddings, and ranking are heuristic projections
   over the same canonical facts.
2. **Path-based component identity** -- canonical IDs derived from repo-relative
   paths (`component::cargo::chizu-core`), not manifest display names.
3. **Two-phase indexing** -- component discovery runs before file extraction so
   ownership is stable.
4. **Hash-based incremental updates** -- Blake3 content hashing detects changes;
   stale entities cascade-delete; summaries regenerate only when source changes.
5. **Local-first** -- SQLite + usearch, no cloud dependency.
6. **Graph + vectors from same facts** -- both are projections over the same
   entity/edge fact store.
