//! Chizu Core - Types and storage for the Chizu code knowledge graph.
//!
//! This crate provides:
//! - Core data models (Entity, Edge, ComponentId, etc.)
//! - Configuration types and validation
//! - SQLite storage backend
//! - usearch vector index wrapper

mod aws;
pub mod config;
pub mod model;
pub mod provider;
pub mod query;
pub mod reranker;
pub mod store;

// Re-export commonly used types
pub use config::{
    Config, ConfigError, CutoffMode, EmbeddingConfig, IndexConfig, ProviderConfig,
    ProviderFlavor, RerankWeights, RerankerConfig, RerankerFlavor, SearchConfig, SummaryConfig,
};
pub use model::{
    ComponentId, Edge, EdgeKind, EmbeddingMeta, Entity, EntityKind, FileKind, FileRecord, Summary,
    TaskRoute, Visibility,
};
pub use model::{doc_id, entity_id, entity_id_to_usearch_key, source_unit_id, symbol_id, test_id};
pub use provider::{BedrockProvider, OpenAiProvider, Provider, ProviderError, with_retry};
pub use query::{TaskCategory, TraversalOptions, TraversalResult, classify_query, graph_traversal};
pub use reranker::{
    BedrockReranker, HttpReranker, RerankDocument, RerankScore, Reranker, RerankerError,
};
pub use store::{ChizuStore, Store, StoreError};
