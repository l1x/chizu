use crate::config::Config;
use crate::model::{
    ComponentId, Edge, EdgeKind, EmbeddingMeta, Entity, EntityKind, FileRecord, Summary, TaskRoute,
};
use std::path::Path;

pub mod sqlite;
pub mod usearch;

pub use sqlite::SqliteStore;
pub use usearch::UsearchIndex;

/// Store error types
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("usearch error: {0}")]
    Usearch(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("vector key collision: key {key} already exists for a different entity")]
    VectorKeyCollision { key: i64 },
    #[error("{0}")]
    Other(String),
}

/// Result type for store operations
pub type Result<T> = std::result::Result<T, StoreError>;

/// The main store trait abstracting over the combined SQLite + vector storage.
///
/// All relational operations (entities, edges, files, summaries, task routes,
/// embedding metadata) are backed by SQLite. Vector operations (add, search,
/// remove embeddings) are backed by usearch. Implementations must provide both.
pub trait Store {
    // ── Entity operations ───────────────────────────────────────────────

    /// Insert or replace an entity.
    fn insert_entity(&self, entity: &Entity) -> Result<()>;

    /// Get an entity by ID.
    fn get_entity(&self, id: &str) -> Result<Option<Entity>>;

    /// Query entities by kind.
    fn get_entities_by_kind(&self, kind: EntityKind) -> Result<Vec<Entity>>;

    /// Query entities by component ID.
    fn get_entities_by_component(&self, component_id: &ComponentId) -> Result<Vec<Entity>>;

    /// Delete an entity by ID.
    fn delete_entity(&self, id: &str) -> Result<()>;

    /// Delete all entities for a component.
    fn delete_entities_by_component(&self, component_id: &ComponentId) -> Result<usize>;

    // ── Edge operations ─────────────────────────────────────────────────

    /// Insert or replace an edge.
    fn insert_edge(&self, edge: &Edge) -> Result<()>;

    /// Get edges by source ID.
    fn get_edges_from(&self, src_id: &str) -> Result<Vec<Edge>>;

    /// Get edges by destination ID.
    fn get_edges_to(&self, dst_id: &str) -> Result<Vec<Edge>>;

    /// Get edges by relationship kind.
    fn get_edges_by_rel(&self, rel: EdgeKind) -> Result<Vec<Edge>>;

    /// Delete an edge.
    fn delete_edge(&self, src_id: &str, rel: EdgeKind, dst_id: &str) -> Result<()>;

    /// Delete all edges for a component.
    fn delete_edges_by_component(&self, component_id: &ComponentId) -> Result<usize>;

    // ── File operations ─────────────────────────────────────────────────

    /// Insert or replace a file record.
    fn insert_file(&self, file: &FileRecord) -> Result<()>;

    /// Get a file record by path.
    fn get_file(&self, path: &str) -> Result<Option<FileRecord>>;

    /// Get all indexed files.
    fn get_all_files(&self) -> Result<Vec<FileRecord>>;

    /// Delete a file record.
    fn delete_file(&self, path: &str) -> Result<()>;

    // ── Summary operations ──────────────────────────────────────────────

    /// Insert or replace a summary.
    fn insert_summary(&self, summary: &Summary) -> Result<()>;

    /// Get a summary by entity ID.
    fn get_summary(&self, entity_id: &str) -> Result<Option<Summary>>;

    /// Delete a summary.
    fn delete_summary(&self, entity_id: &str) -> Result<()>;

    // ── Task route operations ───────────────────────────────────────────

    /// Insert or replace a task route.
    fn insert_task_route(&self, route: &TaskRoute) -> Result<()>;

    /// Get task routes by task name.
    fn get_task_routes(&self, task_name: &str) -> Result<Vec<TaskRoute>>;

    /// Get task routes by entity ID.
    fn get_entity_task_routes(&self, entity_id: &str) -> Result<Vec<TaskRoute>>;

    /// Delete task routes for an entity.
    fn delete_entity_task_routes(&self, entity_id: &str) -> Result<()>;

    // ── Embedding metadata operations ───────────────────────────────────

    /// Insert or replace embedding metadata.
    fn insert_embedding_meta(&self, meta: &EmbeddingMeta) -> Result<()>;

    /// Get embedding metadata by entity ID.
    fn get_embedding_meta(&self, entity_id: &str) -> Result<Option<EmbeddingMeta>>;

    /// Delete embedding metadata.
    fn delete_embedding_meta(&self, entity_id: &str) -> Result<()>;

    // ── Vector operations ───────────────────────────────────────────────

    /// Add a vector to the index.
    ///
    /// If the key already exists (same entity re-indexed), the old vector is
    /// replaced. Returns `VectorKeyCollision` if the key maps to a *different*
    /// entity (hash collision — astronomically unlikely with 64-bit keys at
    /// realistic repo sizes, but checked for safety).
    fn add_vector(&self, entity_id: &str, key: i64, vector: &[f32]) -> Result<()>;

    /// Search for nearest-neighbor vectors.
    /// Returns `(key, distance)` pairs sorted by ascending distance.
    fn search_vectors(&self, query: &[f32], k: usize) -> Result<Vec<(i64, f32)>>;

    /// Remove a vector by key.
    fn remove_vector(&self, key: i64) -> Result<()>;

    /// Get a vector by key.
    fn get_vector(&self, key: i64) -> Result<Option<Vec<f32>>>;

    /// Check if a vector key exists.
    fn contains_vector(&self, key: i64) -> bool;

    /// Number of vectors in the index.
    fn vector_count(&self) -> usize;
}

/// A combined store that manages both SQLite and usearch.
///
/// Fields are private — all access goes through the [`Store`] trait or
/// the transaction helper.
pub struct ChizuStore {
    sqlite: SqliteStore,
    usearch: UsearchIndex,
}

impl ChizuStore {
    /// Open or create a store at the given directory.
    pub fn open(dir: &Path, config: &Config) -> Result<Self> {
        std::fs::create_dir_all(dir)?;

        let sqlite_path = dir.join("graph.db");
        let usearch_path = dir.join("graph.db.usearch");

        let sqlite = SqliteStore::open(&sqlite_path)?;

        let dimensions = config.embedding.dimensions.unwrap_or(768) as usize;
        let usearch = UsearchIndex::open_or_create(&usearch_path, dimensions)?;

        Ok(Self { sqlite, usearch })
    }

    /// Close the store and flush any pending writes.
    pub fn close(self) -> Result<()> {
        self.usearch.close()?;
        // SQLite connection is closed when dropped
        Ok(())
    }

    /// Execute a closure inside a SQLite transaction.
    ///
    /// If the closure returns `Ok`, the transaction is committed.
    /// If the closure returns `Err` or panics, the transaction is rolled back.
    pub fn in_transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Self) -> Result<T>,
    {
        self.sqlite.begin_transaction()?;
        match f(self) {
            Ok(val) => {
                self.sqlite.commit_transaction()?;
                Ok(val)
            }
            Err(e) => {
                let _ = self.sqlite.rollback_transaction();
                Err(e)
            }
        }
    }

    /// Dimensions of the vector index.
    pub fn vector_dimensions(&self) -> usize {
        self.usearch.dimensions()
    }
}

impl Store for ChizuStore {
    // ── Entity (delegate to sqlite) ─────────────────────────────────────

    fn insert_entity(&self, entity: &Entity) -> Result<()> {
        self.sqlite.insert_entity(entity)
    }

    fn get_entity(&self, id: &str) -> Result<Option<Entity>> {
        self.sqlite.get_entity(id)
    }

    fn get_entities_by_kind(&self, kind: EntityKind) -> Result<Vec<Entity>> {
        self.sqlite.get_entities_by_kind(kind)
    }

    fn get_entities_by_component(&self, component_id: &ComponentId) -> Result<Vec<Entity>> {
        self.sqlite.get_entities_by_component(component_id)
    }

    fn delete_entity(&self, id: &str) -> Result<()> {
        self.sqlite.delete_entity(id)
    }

    fn delete_entities_by_component(&self, component_id: &ComponentId) -> Result<usize> {
        self.sqlite.delete_entities_by_component(component_id)
    }

    // ── Edge (delegate to sqlite) ───────────────────────────────────────

    fn insert_edge(&self, edge: &Edge) -> Result<()> {
        self.sqlite.insert_edge(edge)
    }

    fn get_edges_from(&self, src_id: &str) -> Result<Vec<Edge>> {
        self.sqlite.get_edges_from(src_id)
    }

    fn get_edges_to(&self, dst_id: &str) -> Result<Vec<Edge>> {
        self.sqlite.get_edges_to(dst_id)
    }

    fn get_edges_by_rel(&self, rel: EdgeKind) -> Result<Vec<Edge>> {
        self.sqlite.get_edges_by_rel(rel)
    }

    fn delete_edge(&self, src_id: &str, rel: EdgeKind, dst_id: &str) -> Result<()> {
        self.sqlite.delete_edge(src_id, rel, dst_id)
    }

    fn delete_edges_by_component(&self, component_id: &ComponentId) -> Result<usize> {
        self.sqlite.delete_edges_by_component(component_id)
    }

    // ── File (delegate to sqlite) ───────────────────────────────────────

    fn insert_file(&self, file: &FileRecord) -> Result<()> {
        self.sqlite.insert_file(file)
    }

    fn get_file(&self, path: &str) -> Result<Option<FileRecord>> {
        self.sqlite.get_file(path)
    }

    fn get_all_files(&self) -> Result<Vec<FileRecord>> {
        self.sqlite.get_all_files()
    }

    fn delete_file(&self, path: &str) -> Result<()> {
        self.sqlite.delete_file(path)
    }

    // ── Summary (delegate to sqlite) ────────────────────────────────────

    fn insert_summary(&self, summary: &Summary) -> Result<()> {
        self.sqlite.insert_summary(summary)
    }

    fn get_summary(&self, entity_id: &str) -> Result<Option<Summary>> {
        self.sqlite.get_summary(entity_id)
    }

    fn delete_summary(&self, entity_id: &str) -> Result<()> {
        self.sqlite.delete_summary(entity_id)
    }

    // ── Task route (delegate to sqlite) ─────────────────────────────────

    fn insert_task_route(&self, route: &TaskRoute) -> Result<()> {
        self.sqlite.insert_task_route(route)
    }

    fn get_task_routes(&self, task_name: &str) -> Result<Vec<TaskRoute>> {
        self.sqlite.get_task_routes(task_name)
    }

    fn get_entity_task_routes(&self, entity_id: &str) -> Result<Vec<TaskRoute>> {
        self.sqlite.get_entity_task_routes(entity_id)
    }

    fn delete_entity_task_routes(&self, entity_id: &str) -> Result<()> {
        self.sqlite.delete_entity_task_routes(entity_id)
    }

    // ── Embedding metadata (delegate to sqlite) ─────────────────────────

    fn insert_embedding_meta(&self, meta: &EmbeddingMeta) -> Result<()> {
        self.sqlite.insert_embedding_meta(meta)
    }

    fn get_embedding_meta(&self, entity_id: &str) -> Result<Option<EmbeddingMeta>> {
        self.sqlite.get_embedding_meta(entity_id)
    }

    fn delete_embedding_meta(&self, entity_id: &str) -> Result<()> {
        self.sqlite.delete_embedding_meta(entity_id)
    }

    // ── Vector operations (delegate to usearch with collision check) ────

    fn add_vector(&self, entity_id: &str, key: i64, vector: &[f32]) -> Result<()> {
        // Collision check: if the key already exists, verify it belongs to
        // the same entity (re-index) rather than a different one (collision).
        if self.usearch.contains(key) {
            if let Some(existing) = self.sqlite.get_embedding_meta_by_usearch_key(key)? {
                if existing.entity_id != entity_id {
                    return Err(StoreError::VectorKeyCollision { key });
                }
            }
            // Same entity re-indexed — remove old vector first
            self.usearch.remove(key)?;
        }
        self.usearch.add(key, vector)
    }

    fn search_vectors(&self, query: &[f32], k: usize) -> Result<Vec<(i64, f32)>> {
        self.usearch.search(query, k)
    }

    fn remove_vector(&self, key: i64) -> Result<()> {
        self.usearch.remove(key)
    }

    fn get_vector(&self, key: i64) -> Result<Option<Vec<f32>>> {
        self.usearch.get(key)
    }

    fn contains_vector(&self, key: i64) -> bool {
        self.usearch.contains(key)
    }

    fn vector_count(&self) -> usize {
        self.usearch.len()
    }
}
