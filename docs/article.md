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

With Chizu, you ask: `chizu plan "what tests cover user authentication"` and get a ranked list of relevant test entities with explanations of why they matter.

## What Chizu Does

Chizu treats your codebase as a graph. It parses your source files, extracts meaningful entities (functions, structs, tests, documentation, infrastructure), and creates edges between them based on their relationships.

### Entity Types

| Type | Description | Example |
|------|-------------|---------|
| `symbol` | Functions, structs, traits, types | `fn handle_request` |
| `test` | Test functions | `#[test] fn test_routing` |
| `source_unit` | Source files | `src/main.rs` |
| `doc` | Markdown documentation | `README.md` |
| `infra_root` | Terraform directories | `infra/base-infra` |
| `containerized` | Dockerfiles | `Dockerfile` |

### Edge Types

| Edge | Meaning | Example |
|------|---------|---------|
| `defines` | File contains symbol | `main.rs --defines--> handle_request` |
| `tested_by` | File has associated tests | `router.rs --tested_by--> test_routing` |
| `mentions` | Doc references symbol | `README.md --mentions--> Config` |
| `deploys` | Infra deploys container | `base-infra --deploys--> Dockerfile` |

This graph structure enables queries that understand context, not just text matching.

## Architecture Overview

```
Input (Rust, TS, Astro, Terraform, Markdown)
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

3. **Language-agnostic**: The parser architecture supports any language with a tree-sitter grammar. Currently supports Rust, TypeScript, Astro, Terraform, and Markdown.

4. **Graph-native**: Relationships are first-class citizens, not afterthoughts.

5. **Query-flexible**: Access your data via CLI, direct SQL, or natural language.

## Using Chizu

### Basic Indexing

```bash
# Index a repository
chizu index /path/to/repo

# With embeddings (requires Ollama)
chizu index --embed /path/to/repo
```

The index is stored in `.chizu/` at the repository root:
- `graph.db` - SQLite database with entities and edges
- `vectors.usearch` - Vector index for semantic search
- `content_hashes.json` - Content addressing for incremental updates

### Natural Language Queries

```bash
chizu plan "how does routing work"
```

This uses an LLM to interpret your question, query the graph, and return relevant entities with explanations. The reranking system considers:

- Keyword matches in entity names
- Semantic similarity (if embeddings enabled)
- Entity type relevance
- Graph connectivity
- Path matching

### Direct Entity Queries

```bash
# List all symbols
chizu query entities --kind symbol

# Find specific entity
chizu inspect "symbol::src/main.rs::main"
```

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

Files are parsed using tree-sitter, a parser generator that produces concrete syntax trees for many languages. For each file:

- Rust: Extracts functions, structs, enums, traits, impl blocks, tests
- TypeScript: Extracts functions, classes, interfaces, types
- Astro: Extracts components, frontmatter
- Terraform: Extracts resources, modules, variables
- Markdown: Extracts headers, code blocks, symbol mentions

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

- A file "defines" all symbols it contains
- A symbol "uses" symbols it references
- A test file "tests" the source file it's named after
- Documentation "mentions" symbols referenced in backticks

### 5. Embedding Generation (Optional)

If embeddings are enabled, Chizu sends entity text to a local Ollama instance (or OpenAI) and stores the resulting vectors in usearch. This enables semantic search - finding entities related by meaning, not just keyword.

## Query Processing

When you run `chizu plan "how does error handling work"`, here's what happens:

1. **Entity Retrieval**: Fetch candidate entities from the graph
2. **Keyword Matching**: Score entities whose names contain query terms
3. **Vector Search** (if enabled): Find semantically similar entities
4. **Reranking**: Combine scores using weighted factors:
   - Task routing (detect query intent)
   - Keyword relevance
   - Name match quality
   - Vector similarity
   - Entity type preference
   - Whether symbol is exported
   - Path relevance
5. **LLM Synthesis**: Present top entities to an LLM with context, get structured answer

## Configuration

Create `.chizu.toml` in your repository root:

```toml
[index]
exclude_patterns = ["**/target/**", "**/node_modules/**"]
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

[embedding]
enabled = true
provider = "ollama"
base_url = "http://localhost:11434/v1"
model = "nomic-embed-text-v2-moe:latest"
dimensions = 768
```

## Use Cases

### Onboarding to a New Codebase

```bash
chizu plan "explain the architecture of the payment system"
```

Get a high-level overview without reading hundreds of files.

### Finding Relevant Tests

```bash
chizu plan "what tests cover the checkout flow"
```

Skip the grep-and-hope approach.

### Understanding Dependencies

```bash
sqlite3 .chizu/graph.db "SELECT dst_id FROM edges 
    WHERE src_id = 'symbol::src/order.rs::process_order' 
    AND rel = 'uses';"
```

See exactly what a function depends on.

### Documentation Gap Analysis

```bash
sqlite3 .chizu/graph.db "SELECT s.name FROM entities s 
    LEFT JOIN edges e ON s.id = e.dst_id AND e.rel = 'mentions'
    WHERE s.kind = 'symbol' AND e.dst_id IS NULL;"
```

Find exported symbols never mentioned in docs.

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

- **Language coverage**: Only Rust, TypeScript, Astro, Terraform, and Markdown. Python, Go, Java, and others need parsers.
- **Cross-file analysis**: Import resolution and cross-file type inference are limited.
- **Git integration**: No blame information or commit history in the graph yet.

Future directions:

- Language server protocol (LSP) integration for IDE support
- Web UI for graph visualization
- Code complexity metrics
- Import/dependency analysis
- Git blame integration
- Automated documentation generation

## Why Rust?

Chizu is written in Rust because parsing millions of lines of code needs to be fast. Tree-sitter parsing, content hashing, and database operations all benefit from Rust's zero-cost abstractions and memory safety. The incremental indexing process can handle large repositories in seconds, not minutes.

## Getting Started

```bash
# Clone and build
git clone https://github.com/yourusername/chizu
cd chizu
cargo build --release

# Index your project
./target/release/chizu index /path/to/your/project

# Start exploring
./target/release/chizu plan "how does this codebase work"
```

## Conclusion

Codebases are graphs. It's time our tools treated them that way.

Chizu is an experiment in bringing knowledge graph technology to local code exploration. It won't replace your IDE or your ability to read code, but it might just save you from the 30-minute grep spirals that we've all experienced.

If you're working with large codebases and frustrated with code discovery, give it a try. The project is open source and contributions - especially new language parsers - are welcome.

---

*Chizu (地図) means "map" in Japanese. Because every codebase needs a map.*
