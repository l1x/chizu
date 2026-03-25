# Grakno Guide for Agents

## What is Grakno?

Grakno is a **local code knowledge graph** that indexes your codebase into a queryable graph database. It helps agents and developers understand code relationships without opening files blindly.

### Core Purpose

```
Traditional workflow: grep → open file → read → understand → repeat
Grakno workflow:    ask question → get relevant entities → understand context
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        Input                                │
│  (Rust, TypeScript, Astro, Terraform, Markdown, etc.)       │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                    Indexing Pipeline                        │
│  1. File discovery (walk directory tree)                    │
│  2. Parse files (tree-sitter parsers)                       │
│  3. Extract entities (symbols, tests, docs, infra)          │
│  4. Create edges (defines, uses, mentions, deploys)         │
│  5. Generate embeddings (Ollama/OpenAI) - optional          │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                    Storage Layer                            │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │   SQLite     │  │   usearch    │  │   Blake3     │      │
│  │  (entities,  │  │  (vectors)   │  │  (hashes)    │      │
│  │   edges)     │  │              │  │              │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                    Query Interface                          │
│  - Natural language queries                                 │
│  - Entity inspection                                        │
│  - Graph traversal                                          │
│  - Vector search (if embeddings enabled)                    │
└─────────────────────────────────────────────────────────────┘
```

## Entity Types

| Type | Description | Example |
|------|-------------|---------|
| `symbol` | Functions, structs, traits, types | `fn handle_request` |
| `test` | Test functions | `#[test] fn test_routing` |
| `source_unit` | Source files | `src/main.rs` |
| `doc` | Markdown documentation | `README.md` |
| `infra_root` | Terraform directories | `infra/base-infra` |
| `containerized` | Dockerfiles | `Dockerfile` |

## Edge Types

| Edge | Meaning | Example |
|------|---------|---------|
| `defines` | File → Symbol | `main.rs --defines--> handle_request` |
| `tested_by` | File → Test | `router.rs --tested_by--> test_routing` |
| `mentions` | Doc → Symbol | `README.md --mentions--> Config` |
| `deploys` | Infra → Container | `base-infra --deploys--> Dockerfile` |

## Quick Start

### 1. Index a Repository

```bash
# Basic indexing
grakno index /path/to/repo

# With embeddings (requires Ollama)
grakno index --embed /path/to/repo

# Check results
grakno inspect
```

### 2. Query the Graph

```bash
# Natural language query
grakno plan "how does routing work"

# List all entities
grakno query entities

# Inspect specific entity
grakno inspect "symbol::src/main.rs::main"
```

### 3. Direct SQL Access

```bash
# Query the SQLite database directly
cd /path/to/repo/.grakno

# List all symbols
sqlite3 graph.db "SELECT name FROM entities WHERE kind='symbol' LIMIT 10;"

# Count edges by type
sqlite3 graph.db "SELECT rel, COUNT(*) FROM edges GROUP BY rel;"

# Find entities matching pattern
sqlite3 graph.db "SELECT * FROM entities WHERE name LIKE '%handler%';"
```

## Configuration

Create `.grakno.toml` in your repo:

```toml
[index]
exclude_patterns = ["**/target/**", "**/.git/**", "**/node_modules/**"]
parallel_workers = 4

[query]
default_limit = 15

[query.rerank_weights]
task_route = 0.30
keyword = 0.20
name_match = 0.15
vector = 0.20
kind_preference = 0.05
exported = 0.05
path_match = 0.05

[llm]
default_model = "gpt-4o-mini"
timeout_secs = 60
retry_attempts = 3

[embedding]
enabled = true
provider = "ollama"
base_url = "http://localhost:11434/v1"
api_key = ""
model = "nomic-embed-text-v2-moe:latest"
dimensions = 768
batch_size = 32
timeout_secs = 120
```

## Working with the Code

### Adding a New Language Parser

1. Add tree-sitter dependency in `crates/grakno-index/Cargo.toml`:
```toml
tree-sitter-python = "0"
```

2. Create parser in `crates/grakno-index/src/parser_python.rs`:
```rust
use tree_sitter::Parser;

pub fn parse_python_file(source: &str) -> Result<ParseResult, IndexError> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_python::LANGUAGE.into())?;
    let tree = parser.parse(source, None)
        .ok_or(IndexError::Parse("failed to parse".into()))?;
    
    // Extract symbols...
}
```

3. Register in `indexer.rs`:
```rust
Some("py") => index_python_file(store, &path, project_root, stats, indexed_files)?,
```

### Adding a New Entity Type

1. Add to `crates/grakno-core/src/model/entity.rs`:
```rust
pub enum EntityKind {
    // ... existing kinds
    NewKind,
}
```

2. Add parsing/serialization in the same file

3. Use in indexer:
```rust
store.insert_entity(&Entity {
    id: id::new_kind_id(&name),
    kind: EntityKind::NewKind,
    name,
    // ...
})?;
```

### Adding a New Edge Type

1. Add to `crates/grakno-core/src/model/edge.rs`:
```rust
pub enum EdgeKind {
    // ... existing kinds
    NewRelation,
}
```

2. Create edges in indexer:
```rust
store.insert_edge(&Edge {
    src_id: entity_a,
    rel: EdgeKind::NewRelation,
    dst_id: entity_b,
    provenance_path: Some(path),
    provenance_line: Some(line),
})?;
```

## Key Files

| File | Purpose |
|------|---------|
| `crates/grakno-index/src/indexer.rs` | Main indexing pipeline |
| `crates/grakno-index/src/parser*.rs` | Language-specific parsers |
| `crates/grakno-index/src/markdown.rs` | Markdown mention extraction |
| `crates/grakno-core/src/model/*.rs` | Entity/edge/summary models |
| `crates/grakno-core/src/store/*.rs` | SQLite + usearch backends |
| `crates/grakno-query/src/*.rs` | Query pipeline & ranking |
| `crates/grakno/src/main.rs` | CLI & command dispatch |

## Common Tasks

### Debug Indexing Issues

```bash
# Run with debug logging
RUST_LOG=debug grakno index /path/to/repo 2>&1 | head -50

# Check specific file
sqlite3 .grakno/graph.db "SELECT * FROM files WHERE path LIKE '%problematic%';"

# Verify entities
sqlite3 .grakno/graph.db "SELECT COUNT(*) FROM entities WHERE kind='symbol';"
```

### Test Changes

```bash
# Run unit tests
cargo test

# Test specific crate
cargo test -p grakno-index

# Test on sample repo
rm -rf /tmp/test_repo/.grakno
cargo run -- index /tmp/test_repo
```

### Performance Profiling

```bash
# Time indexing
time grakno index /large/repo

# Check DB size
ls -lh .grakno/

# Query performance
time grakno plan "complex query"
```

## Design Principles

1. **Local-first**: Everything runs locally, no cloud required
2. **Incremental**: Only re-index changed files
3. **Language-agnostic**: Extensible parser architecture
4. **Graph-native**: Relationships are first-class
5. **Query-flexible**: SQL, natural language, or API

## Troubleshooting

| Issue | Solution |
|-------|----------|
| Unicode panic | Already fixed (pulldown-cmark) |
| Embeddings fail | Check Ollama running + model pulled |
| No results | Check `.grakno/graph.db` exists |
| Slow queries | Enable embeddings for vector search |
| Wrong entity IDs | Use `grakno query entities` to find correct format |

## Future Extensions

Potential improvements:
- Python/Go/Java parsers
- Git blame integration
- Code complexity metrics
- Import/dependency graph analysis
- IDE LSP integration
- Web UI for graph visualization
