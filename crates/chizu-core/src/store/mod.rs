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
    #[error("{0}")]
    Other(String),
}

/// Result type for store operations
pub type Result<T> = std::result::Result<T, StoreError>;

/// The main store trait abstracting over storage backends.
pub trait Store {
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

    /// Insert or replace a file record.
    fn insert_file(&self, file: &FileRecord) -> Result<()>;

    /// Get a file record by path.
    fn get_file(&self, path: &str) -> Result<Option<FileRecord>>;

    /// Get all indexed files.
    fn get_all_files(&self) -> Result<Vec<FileRecord>>;

    /// Delete a file record.
    fn delete_file(&self, path: &str) -> Result<()>;

    /// Insert or replace a summary.
    fn insert_summary(&self, summary: &Summary) -> Result<()>;

    /// Get a summary by entity ID.
    fn get_summary(&self, entity_id: &str) -> Result<Option<Summary>>;

    /// Delete a summary.
    fn delete_summary(&self, entity_id: &str) -> Result<()>;

    /// Insert or replace a task route.
    fn insert_task_route(&self, route: &TaskRoute) -> Result<()>;

    /// Get task routes by task name.
    fn get_task_routes(&self, task_name: &str) -> Result<Vec<TaskRoute>>;

    /// Get task routes by entity ID.
    fn get_entity_task_routes(&self, entity_id: &str) -> Result<Vec<TaskRoute>>;

    /// Delete task routes for an entity.
    fn delete_entity_task_routes(&self, entity_id: &str) -> Result<()>;

    /// Insert or replace embedding metadata.
    fn insert_embedding_meta(&self, meta: &EmbeddingMeta) -> Result<()>;

    /// Get embedding metadata by entity ID.
    fn get_embedding_meta(&self, entity_id: &str) -> Result<Option<EmbeddingMeta>>;

    /// Delete embedding metadata.
    fn delete_embedding_meta(&self, entity_id: &str) -> Result<()>;
}

/// A combined store that manages both SQLite and usearch.
pub struct ChizuStore {
    pub sqlite: SqliteStore,
    pub usearch: UsearchIndex,
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
}

impl Store for ChizuStore {
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

    fn insert_summary(&self, summary: &Summary) -> Result<()> {
        self.sqlite.insert_summary(summary)
    }

    fn get_summary(&self, entity_id: &str) -> Result<Option<Summary>> {
        self.sqlite.get_summary(entity_id)
    }

    fn delete_summary(&self, entity_id: &str) -> Result<()> {
        self.sqlite.delete_summary(entity_id)
    }

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

    fn insert_embedding_meta(&self, meta: &EmbeddingMeta) -> Result<()> {
        self.sqlite.insert_embedding_meta(meta)
    }

    fn get_embedding_meta(&self, entity_id: &str) -> Result<Option<EmbeddingMeta>> {
        self.sqlite.get_embedding_meta(entity_id)
    }

    fn delete_embedding_meta(&self, entity_id: &str) -> Result<()> {
        self.sqlite.delete_embedding_meta(entity_id)
    }
}
