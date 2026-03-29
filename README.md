# Chizu (地図)

**Subject-to-file routing for coding agents.**

Chizu is a local repository understanding engine. It extracts deterministic
structural facts about a codebase -- components, files, symbols, docs, infra
units, and their relationships -- and uses those facts to route a subject to the
most relevant files and components. It also materializes a graph for human
visualization and navigation.

![Chizu Knowledge Graph](docs/knowledge-graph.svg)

## Quick Start

### Installation

```bash
cargo install --path crates/chizu
```

Or from crates.io (when published):
```bash
cargo install chizu
```

### 1. Configure and Index

```bash
chizu --repo /path/to/repo config init
chizu --repo /path/to/repo index
```

This creates `.chizu/graph.db` and `.chizu/graph.db.usearch` in your repository
with entities, edges, summaries, and embeddings. Requires a configured
LLM and embedding provider (e.g. Ollama) to be running.

### 2. Search

```bash
chizu --repo /path/to/repo search "how does authentication work"
```

Returns a ranked reading plan: which files and entities to read first and why.
The pipeline classifies the query, retrieves candidates via task routes,
keyword/name/path matching, and vector search, expands graph neighbors, then
reranks with weighted multi-signal scoring.

### 3. Inspect

```bash
chizu --repo /path/to/repo entities
chizu --repo /path/to/repo entity "component::cargo::crates/my-crate"
chizu --repo /path/to/repo edges --from "component::cargo::crates/my-crate"
chizu --repo /path/to/repo routes --task deploy
```

### 4. Visualize

```bash
chizu --repo /path/to/repo visualize --legend > graph.svg
open graph.svg
```

## Commands

| Command     | Description                         | Key flags |
| ----------- | ----------------------------------- | --------- |
| `index`     | Extract facts + summarize + embed   | none |
| `search`    | Full query pipeline -> reading plan | `--limit`, `--category`, `--format`, positional query |
| `entity`    | Look up a single entity by id       | positional id |
| `entities`  | List entities                       | `--component` |
| `routes`    | List task routes                    | `--task`, `--entity` |
| `edges`     | List edges                          | `--from`, `--to`, `--rel` |
| `visualize` | Generate SVG graph                  | `--entity-id`, `--depth`, `--kind`, `--exclude`, `--layout`, `--max-nodes`, `--output`, `--legend` |
| `config`    | Initialize or validate config       | subcommands: `init`, `validate` |
| `guide`     | Interactive usage guide             | none |

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
derived metadata (entities, edges, files, summaries, task routes, embedding
metadata). usearch provides HNSW-based approximate nearest neighbor search over
embedding vectors. Vectors live only in usearch, not duplicated in SQLite.

## Component Identity

Components use canonical path-based IDs derived from the repo-relative component
root path, not from manifest display names:

- `component::cargo::crates/chizu-core` (Rust crate)
- `component::npm::packages/web` (npm package)
- `component::npm::.` (root package)

Every file under a component root inherits that component's canonical
`component_id`. Every entity derived from that file inherits the same ID.
Component discovery happens before file extraction (two-phase indexing).

## Configuration

All runtime configuration lives in `.chizu.toml` at the repository root.
Missing file means all defaults apply. Generate one with `chizu config init`.

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

Provider connection config is defined once per provider under
`[providers.<name>]`. The `[summary]` and `[embedding]` sections reference a
provider by name. See [docs/prd.md](docs/prd.md) for configuration design rules.

## Target Repositories

Mixed-language monorepos with infrastructure and documentation: Rust workspaces,
TypeScript/npm workspaces, Terraform roots, Docker deployments, Astro/Hugo
sites, and combinations thereof.

## Requirements

- **Rust** 1.70+ (to build)
- **Ollama** or another OpenAI-compatible provider (required for summaries and
  embeddings during indexing)
  - Install: https://ollama.com
  - Pull models: `ollama pull llama3:8b && ollama pull nomic-embed-text-v2-moe:latest`

## Documentation

- [Brief](docs/brief.md)
- [Product Requirements](docs/prd.md)
- Interactive guide: `chizu guide`

## License

MIT
