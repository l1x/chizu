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
cargo install --path chizu-cli
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
chizu --repo /path/to/repo visualize -o graph.svg
open graph.svg
```

## Commands

| Command     | Description                         | Key flags |
| ----------- | ----------------------------------- | --------- |
| `index`     | Extract facts + summarize + embed   | `--force` |
| `search`    | Full query pipeline -> reading plan | `--limit`, `--category`, `--format`, positional query |
| `entity`    | Look up a single entity by id       | positional id |
| `entities`  | List entities                       | `--component`, `--kind` |
| `routes`    | List task routes                    | `--task`, `--entity` |
| `edges`     | List edges                          | `--from`, `--to`, `--rel` |
| `visualize` | Generate SVG graph                  | `--entity-id`, `--depth`, `--kind`, `--exclude`, `--max-nodes`, `--output` |
| `config`    | Initialize or validate config       | subcommands: `init`, `validate` |
| `guide`     | Interactive usage guide             | none |

## Onboarding

### Prerequisites

1. **Rust toolchain** (1.85+): https://rustup.rs
2. **Ollama** running locally: https://ollama.com
3. Pull the required models:

```bash
ollama pull llama3:8b
ollama pull nomic-embed-text-v2-moe:latest
```

Verify ollama is running:

```bash
curl -s http://localhost:11434/v1/models | head -1
```

### Step 1: Install chizu

```bash
git clone https://github.com/l1x/chizu.git
cd chizu
cargo install --path chizu-cli
```

### Step 2: Configure

From your target repository root:

```bash
cd /path/to/your/repo
chizu config init
```

This creates `.chizu.toml` with sensible defaults pointing to a local ollama
instance. Edit it to customize exclude patterns, models, or rerank weights:

```toml
[index]
exclude_patterns = [
    "**/target/**",
    "**/.git/**",
    "**/node_modules/**",
    "**/.venv/**",
    "**/*.lock",
]

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

Validate your config:

```bash
chizu config validate
```

### Step 3: Index the repository

```bash
chizu index
```

This walks the repo, extracts entities and edges, generates LLM summaries, and
builds the embedding index. On a mid-size repo (~60 files, ~650 entities) with
local ollama, expect 5-10 minutes for the first run. Re-runs are incremental
and skip unchanged files.

Output:

```
Indexed 64 files (64 walked)
Discovered 4 components
Inserted 656 entities and 649 edges
Summaries: 345 generated, 0 skipped, 0 failed
Embeddings: 345 generated, 0 skipped, 0 failed
```

### Step 4: Search

```bash
chizu search "how does authentication work"
chizu search "deploy to prod" --category deploy
chizu search "fix the login bug" --format json --limit 5
```

### Step 5: Onboard an agent

To give a coding agent (Claude Code, Cursor, Aider, etc.) access to chizu's
knowledge graph, add a section to your `CLAUDE.md` or equivalent agent config:

```markdown
## Repository map

This repo is indexed with chizu. Before exploring code, use chizu to find
relevant files:

\`\`\`bash
# Find entities related to a topic
chizu search "your question here"

# Get details on a specific entity
chizu entity "symbol::src/auth.rs::validate_token"

# List entities in a component
chizu entities --component cargo::crates/core

# Explore edges from an entity
chizu edges --from "component::cargo::crates/core"

# Get task-specific routes
chizu routes --task debug
\`\`\`

Use `chizu search` with `--format json` for structured output that can be
parsed programmatically. The search pipeline classifies the query into a task
category (understand, debug, build, test, deploy, configure), retrieves
candidates from multiple signals, expands graph neighbors, and returns a
ranked reading plan.
```

The agent can then run `chizu search` to orient itself before reading files,
reducing the number of files it needs to explore and improving context
relevance.

For non-interactive agent pipelines, use JSON output:

```bash
chizu search "how does the store layer work" --format json | jq '.entries[:3]'
```

This returns structured data the agent can parse to extract file paths, entity
IDs, and relevance scores.

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

## Documentation

- [Brief](docs/brief.md)
- [Product Requirements](docs/prd.md)
- Interactive guide: `chizu guide`

## License

MIT
