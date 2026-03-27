# Chizu (地図)

**Your code's mental map.**

Chizu is a local-first code knowledge graph for software repositories. It builds a
structured model of your codebase, extracts symbols and relationships, and
enables fast semantic search over your code.

The goal is simple: help agents and developers answer "what should I read
first?" before they start opening files blindly.

## Architecture

Chizu uses a **dual-backend storage system**:

### SQLite + usearch (default)
- **SQLite**: Stores entities, edges, files, summaries, and task routes
- **usearch**: HNSW vector index for fast similarity search over embeddings
- Local, fast, no external dependencies

### Grafeo (alternative)
- Unified graph database backend
- Handles both structured data and vector search in one system

## Features

- **Multi-language parsing**: Rust, TypeScript, Astro, Terraform, Markdown
- **Graph traversal**: Navigate relationships (defines, uses, implements, etc.)
- **Semantic search**: Vector similarity over LLM-generated summaries
- **Task routing**: Prioritized entity lists for common tasks (debug, build, deploy)
- **Local-first**: All data stays on your machine

## Docs

- [Product PRD](docs/prd.md)
- [Graph Model](docs/graph-model.md)
