//! Chizu Core - Types and storage for the Chizu code knowledge graph.
//!
//! This crate provides:
//! - Core data models (Entity, Edge, ComponentId, etc.)
//! - Configuration types and validation
//! - SQLite storage backend
//! - usearch vector index wrapper

pub mod config;
pub mod model;
pub mod store;

// Re-export commonly used types
pub use config::{
    Config, ConfigError, EmbeddingConfig, IndexConfig, ProviderConfig, RerankWeights, SearchConfig,
    SummaryConfig,
};
pub use model::{
    ComponentId, Edge, EdgeKind, EmbeddingMeta, Entity, EntityKind, FileKind, FileRecord, Summary,
    TaskRoute, Visibility,
};
pub use model::{doc_id, entity_id_to_usearch_key, source_unit_id, symbol_id, test_id};
pub use store::{ChizuStore, Store, StoreError};
