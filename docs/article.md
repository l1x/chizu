# Chizu: A Local Knowledge Graph for Your Codebase

## Introduction

We've all been there. You join a new team, clone a repository with half a million lines of code, and your manager asks you to "fix a bug in the payment processing module." You grep for "payment" and get 3,000 results. You open a file that looks promising, trace through five imports, and twenty minutes later realize you're looking at the wrong payment system entirely.

This is the fundamental friction of software engineering at scale: **codebases grow faster than our ability to understand them**. We have powerful tools for writing code - IDEs with autocomplete, linters, formatters - but our tools for *reading* code haven't evolved much beyond text search.

Chizu is an attempt to fix this. It's a local code knowledge graph that indexes your entire codebase into a queryable database, letting you ask questions like "how does error handling work in the API layer" instead of grepping for "error" and hoping for the best.

## The Problem with Code Discovery

Modern software development involves navigating enormous graphs of interconnected concepts:

- Functions call other functions
- Types reference other types
- Tests validate implementations
- Documentation mentions APIs
- Infrastructure deploys services
- Configuration wires everything together

Yet our primary discovery tool is still `grep` - a line-oriented text search that treats code as a flat sequence of characters. This works fine for finding where a specific string appears, but falls apart when you need to understand *relationships*.

Consider a seemingly simple question: "What tests cover the user authentication flow?" To answer this with traditional tools, you might:

1. Find files that mention "auth" or "login"
2. Identify which functions handle authentication
3. Search for test files that import or reference those functions
4. Manually verify which tests actually test the flow vs. just mention it

With Chizu, you ask: `chizu --repo . search "what tests cover user authentication"` and get a ranked list of relevant entities with explanations of why they matter.

## What Chizu Does

Chizu treats your codebase as a graph. It parses and scans repository inputs,
extracts meaningful entities (components, source files, symbols, tests,
documentation, tasks, sites, and infrastructure), and creates edges between
them based on their relationships.

### Entity Types

| Type | Description | Example |
|------|-------------|---------|
| `component` | Cargo crate or npm package | `component::cargo::chizu-core` |
| `symbol` | Functions, structs, traits, types | `fn handle_request` |
| `test` | Test functions | `#[test] fn test_routing` |
| `source_unit` | Source files | `src/main.rs` |
| `doc` | Markdown documentation | `README.md` |
| `task` | Build or dev tasks from `mise.toml` | `task::build` |
| `site` | Detected Astro/Hugo/site root | `site::.` |
| `infra_root` | Terraform roots | `infra/base-infra/main.tf` |
| `containerized` | Dockerfiles | `Dockerfile` |

### Edge Types

| Edge | Meaning | Example |
|------|---------|---------|
| `contains` | Repo/component/site contains another entity | `repo::. --contains--> component::cargo::chizu-core` |
| `defines` | File contains symbol | `main.rs --defines--> handle_request` |
| `documented_by` | Component or repo is documented by a doc file | `component::cargo::chizu-core --documented_by--> doc::README.md` |
| `tested_by` | File has associated tests | `router.rs --tested_by--> test_routing` |
| `depends_on` | Component depends on another local component | `component::cargo::chizu-cli --depends_on--> component::cargo::chizu-core` |
| `deploys` | Site points to an infra root | `site::. --deploys--> infra_root::infra/main.tf` |

This graph structure enables queries that understand context, not just text matching.

## Architecture Overview

```
Input (Rust, Cargo/npm, Markdown, site files, infra/config)
                    |
                    v
        +-----------------------+
        |   Indexing Pipeline   |
        | - File discovery      |
        | - Tree-sitter parsing |
        | - Entity extraction   |
        | - Edge creation       |
        | - Embedding generation|
        +-----------------------+
                    |
                    v
        +-----------------------+
        |    Storage Layer      |
        | - SQLite (entities)   |
        | - usearch (vectors)   |
        | - Blake3 (hashes)     |
        +-----------------------+
                    |
                    v
        +-----------------------+
        |   Query Interface     |
        | - Natural language    |
        | - Entity inspection   |
        | - Graph traversal     |
        | - Vector search       |
        +-----------------------+
```

### Design Principles

1. **Local-first**: Everything runs on your machine. No code, no embeddings, no metadata ever leaves your system.

2. **Incremental**: Chizu only re-indexes files that have changed, using content hashing to detect modifications quickly.

3. **Adapter-based**: Deep AST extraction is currently strongest for Rust. The
   broader repository model also comes from Cargo and npm manifests, Markdown
   docs, frontmatter, site detection, and scanner rules for infrastructure,
   templates, workflows, and agent config files.

4. **Graph-native**: Relationships are first-class citizens, not afterthoughts.

5. **Query-flexible**: Access your data via CLI, direct SQL, or natural language.

## Using Chizu

### Basic Indexing

```bash
chizu --repo /path/to/repo config init
chizu --repo /path/to/repo index
```

The index is stored in `.chizu/` at the repository root:
- `graph.db` - SQLite database with entities, edges, files, summaries, task routes, and embedding metadata
- `graph.db.usearch` - usearch HNSW vector index for semantic retrieval

Indexing also generates summaries and embeddings. An embedding provider is required.

### Natural Language Queries

```bash
chizu --repo /path/to/repo search "how does routing work"
```

This runs Chizu's full query pipeline and returns a ranked reading plan. The reranking system considers:

- Keyword matches in entity names
- Semantic similarity over required embeddings
- Entity type relevance
- Graph connectivity
- Path matching

### Direct Entity Queries

```bash
# List entities
chizu --repo /path/to/repo entities

# Find specific entity
chizu --repo /path/to/repo entity "symbol::src/main.rs::main"
```

For lower-level inspection, `routes` and `edges` expose task-route and
relationship data as top-level commands.

### Visualization Outputs

```bash
# Static SVG snapshot
chizu --repo /path/to/repo visualize --entity-id "component::cargo::." --output graph.svg

# Interactive HTML tree explorer
chizu --repo /path/to/repo visualize --interactive --entity-id "component::cargo::." --output graph.html
```

The default SVG output is a static artifact you can attach to docs, screenshots,
or design notes. The `--interactive` variant writes a single HTML file with a
tree explorer, search box, breadcrumbs, inspector pane, theme toggle, and
optional `Open in editor` links when `[visualize].editor_link` is configured.

### SQL Access

Since the underlying storage is SQLite, you can query directly:

```bash
cd /path/to/repo/.chizu

# Count entities by type
sqlite3 graph.db "SELECT kind, COUNT(*) FROM entities GROUP BY kind;"

# Find all tests for a module
sqlite3 graph.db "SELECT e.name FROM entities e 
    JOIN edges ed ON e.id = ed.dst_id 
    WHERE ed.src_id LIKE '%router%' AND e.kind = 'test';"
```

## How It Works: The Indexing Pipeline

### 1. File Discovery

Chizu walks the directory tree, respecting `.gitignore` and configurable exclude patterns. It computes a Blake3 hash of each file's content to detect changes.

### 2. Parsing

Chizu combines AST parsing, manifest parsing, and scanner-based extraction. In
the current implementation:

- Rust source files: Extract functions, structs, enums, traits, impl blocks, tests, benches, and reexports
- Cargo and npm manifests: Discover components, local component dependencies, and Cargo features
- Markdown and frontmatter content: Extract docs and content pages
- `mise.toml`: Extract tasks
- Site markers and scanner rules: Detect sites, templates, infra roots, workflows, migrations, specs, container files, and agent config files

### 3. Entity Extraction

Parsed ASTs are traversed to extract entities. Each entity gets a unique ID based on its type, path, and name:

```rust
// Entity ID format: kind::path::name
symbol::src/auth.rs::validate_token
test::src/auth.rs::test_validate_token_expired
source_unit::src/auth.rs
doc::docs/auth.md
```

### 4. Edge Creation

As entities are extracted, relationships are recorded:

- A repo or component "contains" the entities underneath it
- A source unit "defines" symbols it contains
- A source unit can be "tested_by" tests or "benchmarked_by" benches
- A component or repo can be "documented_by" Markdown docs
- A site can "deploy" an infra root, and a template can "render" a site or content page

### 5. Summary and Embedding Generation

During indexing, Chizu sends entity text to a local Ollama instance (or OpenAI-compatible provider), stores summaries, and writes embedding vectors to usearch. This enables retrieval by meaning, not just keyword.

## Query Processing

When you run `chizu --repo . search "how does error handling work"`, here's what happens:

1. **Classify**: Assign a task category such as understand, debug, or build
2. **Retrieve**: Merge candidates from task routes, keyword/name/path matching, and vector search
3. **Expand**: Traverse graph neighbors from the strongest seeds
4. **Rerank**: Combine scores using weighted factors:
   - Task routing (detect query intent)
   - Keyword relevance
   - Name match quality
   - Vector similarity
   - Entity type preference
   - Whether symbol is exported
   - Path relevance
5. **Reading Plan**: Return the top-ranked entities and files to read first

## Configuration

Create `.chizu.toml` in your repository root:

```toml
[index]
exclude_patterns = ["**/target/**", "**/node_modules/**"]

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
# Optional: enable editor deep links in interactive HTML output
# editor_link = "vscode://file/{abs_path}:{line}:{column}"
```

## Use Cases

### Onboarding to a New Codebase

```bash
chizu --repo . search "explain the architecture of the payment system"
```

Get a high-level overview without reading hundreds of files.

### Finding Relevant Tests

```bash
chizu --repo . search "what tests cover the checkout flow"
```

Skip the grep-and-hope approach.

### Understanding Dependencies

```bash
sqlite3 .chizu/graph.db "SELECT dst_id FROM edges 
    WHERE src_id = 'component::cargo::chizu-cli' 
    AND rel = 'depends_on';"
```

See exactly which local components a crate depends on.

### Documentation Gap Analysis

```bash
sqlite3 .chizu/graph.db "SELECT e.id FROM entities e
    LEFT JOIN edges ed ON e.id = ed.src_id AND ed.rel = 'documented_by'
    WHERE e.kind = 'component' AND ed.dst_id IS NULL;"
```

Find components that do not yet have an attached doc entity.

## Comparison with Existing Tools

| Tool | Approach | Local | Graph | Natural Language |
|------|----------|-------|-------|------------------|
| grep | Text search | Yes | No | No |
| ctags | Symbol index | Yes | No | No |
| Sourcegraph | Code search | No | Partial | Yes |
| GitHub Copilot | AI completion | Partial | No | Limited |
| **Chizu** | Knowledge graph | Yes | Yes | Yes |

Chizu occupies a unique space: it provides AI-powered code understanding that runs entirely locally, using a structured graph representation rather than just text search.

## Current Limitations and Future Work

Chizu is early software. Current limitations:

- **Deep language extraction**: Symbol-level parsing is strongest for Rust today. Other repository inputs are covered mostly through manifests, scanners, docs, and site detection.
- **Cross-file analysis**: Import resolution and full call-graph or type-flow analysis are limited.
- **Release polish**: The CLI is usable from source now, but package/release metadata still needs tightening for public distribution.
- **Git integration**: No blame information or commit history in the graph yet.

Future directions:

- Language server protocol (LSP) integration for IDE support
- Richer browser UI and additional graph layouts beyond the current interactive HTML explorer
- Code complexity metrics
- Import/dependency analysis
- Git blame integration
- Automated documentation generation

## Why Rust?

Chizu is written in Rust because parsing millions of lines of code needs to be fast. Tree-sitter parsing, content hashing, and database operations all benefit from Rust's zero-cost abstractions and memory safety. The incremental indexing process can handle large repositories in seconds, not minutes.

## Getting Started

```bash
# Clone and build
git clone https://github.com/l1x/chizu.git
cd chizu
cargo build --release

# Index your project
./target/release/chizu --repo /path/to/your/project config init
./target/release/chizu --repo /path/to/your/project index

# Start exploring
./target/release/chizu --repo /path/to/your/project search "how does this codebase work"
```

## Conclusion

Codebases are graphs. It's time our tools treated them that way.

Chizu is an experiment in bringing knowledge graph technology to local code exploration. It won't replace your IDE or your ability to read code, but it might just save you from the 30-minute grep spirals that we've all experienced.

If you're working with large codebases and frustrated with code discovery, give it a try. The project is open source and contributions - especially new language parsers - are welcome.

---

*Chizu (地図) means "map" in Japanese. Because every codebase needs a map.*
