# Chizu Brief

Chizu is a local-first repository understanding system for software
repositories. It extracts deterministic structural facts about a codebase --
components, files, symbols, docs, infra units, and their relationships -- and
uses those facts to route a subject to the most relevant files and components.
It also materializes a graph for human visualization and graph-based
navigation. The CLI binary is `chizu`; in this workspace the installable Rust
package is `chizu-cli`.

## What It Does

1. **Indexes** a repository into a canonical fact store of entities and
   relationships, then derives graph views, summaries, task routes, and
   embeddings from those facts.
2. **Maintains** those facts incrementally. File hashes detect changes;
   two-phase indexing ensures stable, canonical component ownership even in
   monorepos and workspaces.
3. **Answers** natural-language subjects through a single `search` command that
   runs a five-stage pipeline: classify, retrieve, expand, rerank, and return a
   ranked reading plan.
4. **Visualizes** indexed structure as either a static SVG graph or a
   self-contained interactive HTML tree explorer.

## Key Design Decisions

- **Ownership-first**: Components get canonical path-based IDs
  (`component::cargo::crates/core`), not mutable manifest names. Every file and
  symbol inherits its enclosing component's ID.
- **Deterministic facts, heuristic retrieval**: Structural fact extraction must
  be reproducible. Classification, summaries, embeddings, and reranking can be
  heuristic as long as they point back to canonical extracted facts.
- **Graph + vectors from the same facts**: The graph is a supporting projection
  for visualization and navigation; vector search is a supporting projection
  for recall and ranking. Agents primarily care about subject -> relevant
  files.
- **Single backend**: sqlite+usearch. SQLite for canonical facts and derived
  metadata, usearch for HNSW vector search.

## Target Repositories

Mixed-language monorepos with infrastructure and documentation: Rust
workspaces, npm workspaces, Terraform roots, Docker deployments, Astro/Hugo
sites, and combinations thereof.

## Details

See [docs/prd.md](prd.md) for the full product requirements, graph model,
schema, adapter list, query pipeline, and rerank weights.
