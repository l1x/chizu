# Chizu (地図)

**Your code's mental map.**

Chizu is a local-first code knowledge graph for software repositories. It builds a
structured model of your codebase: symbols, files, components, and their relationships.
It helps you navigate large codebases by understanding structure, not just text.

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

### 1. Index Your Repository

```bash
# Basic indexing (fast, no embeddings)
chizu --repo /path/to/repo index

# With embeddings for semantic search (requires Ollama or OpenAI-compatible API)
chizu --repo /path/to/repo index --embed
```

This creates a `.chizu/graph.db` file in your repository with:
- Entities (symbols, files, components, docs)
- Edges (relationships like "defines", "uses", "imports")
- Embeddings (if `--embed` is used)

### 2. Query the Graph

```bash
# Generate a reading plan for a task
chizu --repo /path/to/repo plan "how does authentication work"

# Semantic search (requires embeddings)
chizu --repo /path/to/repo search "error handling patterns"

# List all components
chizu --repo /path/to/repo query entities

# Inspect a specific entity
chizu --repo /path/to/repo inspect "component::cargo::crates/my-crate"
```

### 3. Visualize

```bash
# Generate an SVG graph
chizu --repo /path/to/repo visualize --legend > graph.svg

# Open in browser
open graph.svg
```

## Commands Reference

| Command | Description | Example |
|---------|-------------|---------|
| `index` | Parse codebase into graph | `chizu --repo . index --embed` |
| `plan` | Generate reading plan for query | `chizu --repo . plan "fix auth bug"` |
| `search` | Semantic search over embeddings | `chizu --repo . search "database pool"` |
| `query entities` | List entities in graph | `chizu --repo . query entities --component X` |
| `query edges` | Show relationships | `chizu --repo . query edges --from <id>` |
| `inspect` | Show entity details | `chizu --repo . inspect <entity-id>` |
| `visualize` | Generate SVG graph | `chizu --repo . visualize > graph.svg` |
| `summarize` | LLM summaries of components | `chizu --repo . summarize --component X` |
| `watch` | Auto-reindex on changes | `chizu --repo . watch` |
| `config init` | Create config file | `chizu config init` |
| `guide` | Show interactive guide | `chizu guide` |

## Plan vs Search

**Use `plan`** when you have a task:
- "how do I add a new API endpoint"
- "debug the authentication flow"
- "refactor the database layer"

Plan combines multiple signals: keywords, names, vector similarity, and task routing.

**Use `search`** when you want to find similar code:
- "error handling patterns"
- "database connection retry logic"
- "configuration validation"

Search uses pure semantic similarity over embeddings.

## Configuration

Create a `.chizu.toml` config file:

```bash
chizu --repo /path/to/repo config init
```

Example configuration:

```toml
[index]
exclude_patterns = ["**/target/**", "**/.git/**", "**/node_modules/**"]
parallel_workers = 4

[query]
default_limit = 15

[llm]
base_url = "http://localhost:11434/v1"
api_key = ""
default_model = "llama3.2-vision:latest"
timeout_secs = 120

[embedding]
enabled = true
provider = "ollama"
base_url = "http://localhost:11434/v1"
model = "nomic-embed-text-v2-moe:latest"
dimensions = 768
batch_size = 32
```

## Architecture

Chizu uses a **dual-backend storage system**:

### SQLite + usearch (default)
- **SQLite**: Stores entities, edges, files, summaries, and task routes
- **usearch**: HNSW vector index for fast similarity search over embeddings
- Local, fast, no external dependencies

### Grafeo (alternative)
- Unified graph database backend
- Handles both structured data and vector search in one system

## Entity Types

| Type | Description | Example |
|------|-------------|---------|
| `symbol` | Functions, structs, traits | `fn handle_request` |
| `source_unit` | Source files | `src/main.rs` |
| `component` | Crate/package | `component::cargo::crates/chizu-core` |
| `doc` | Markdown documentation | `README.md` |
| `test` | Test functions | `#[test] fn test_routing` |
| `containerized` | Dockerfiles | `Dockerfile` |
| `infra_root` | Terraform directories | `infra/prod` |

## Component IDs

Components use canonical path-based IDs:
- `component::cargo::crates/chizu-core` (Rust crate)
- `component::npm::packages/web` (npm package)
- `component::npm::.` (root package)

This ensures consistency even when package names change.

## Daily Workflow

```bash
# 1. Start a new task - get oriented
chizu --repo . plan "implement user profiles"

# 2. Inspect the most relevant entities
chizu --repo . inspect "symbol::src/auth.rs::verify_token"

# 3. While coding, keep watch running in another terminal
chizu --repo . watch

# 4. Find similar implementations
chizu --repo . search "session management"

# 5. Visualize the architecture
chizu --repo . visualize --legend > arch.svg
```

## Requirements

- **Rust** 1.70+ (to build)
- **Ollama** (optional, for embeddings/summaries)
  - Install: https://ollama.com
  - Pull models: `ollama pull nomic-embed-text-v2-moe:latest`

## Documentation

- [Product PRD](docs/prd.md)
- [Graph Model Spec](docs/graph-model-spec.md)
- Interactive guide: `chizu guide`

## License

MIT
