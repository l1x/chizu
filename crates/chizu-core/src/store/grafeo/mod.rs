pub mod edges;
pub mod embeddings;
pub mod entities;
pub mod files;
pub mod stats;
pub mod summaries;
pub mod task_routes;

use grafeo::{Config, GrafeoDB};

use crate::error::Result;

pub struct GrafeoStore {
    db: GrafeoDB,
}

impl GrafeoStore {
    pub fn open(path: &str) -> Result<Self> {
        let config = Config::persistent(path);
        let db = GrafeoDB::with_config(config)
            .map_err(|e| crate::error::ChizuError::Other(format!("grafeo: {e}")))?;
        let store = Self { db };
        store.init_indexes()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let db = GrafeoDB::new_in_memory();
        let store = Self { db };
        store.init_indexes()?;
        Ok(store)
    }

    pub(crate) fn session(&self) -> grafeo::Session {
        self.db.session()
    }

    fn init_indexes(&self) -> Result<()> {
        let sess = self.session();
        // Create property indexes for fast lookups by domain ID.
        // Ignore "already exists" errors since these run on every open.
        let index_queries = [
            "CREATE INDEX idx_entity_eid FOR (n:entity) ON (n.eid)",
            "CREATE INDEX idx_file_path FOR (n:file) ON (n.path)",
            "CREATE INDEX idx_summary_eid FOR (n:summary) ON (n.entity_id)",
            "CREATE INDEX idx_embedding_eid FOR (n:embedding) ON (n.entity_id)",
            "CREATE INDEX idx_task_route_task FOR (n:task_route) ON (n.task_name)",
        ];
        for q in index_queries {
            let _ = sess.execute(q); // ignore if already exists
        }
        // HNSW vector index for ANN search on embedding vectors
        let _ = sess.execute(
            "CREATE VECTOR INDEX idx_embedding_vector ON :embedding(vector) METRIC 'cosine'",
        );
        Ok(())
    }

    pub fn begin_transaction(&self) -> Result<()> {
        Ok(())
    }

    pub fn commit_transaction(&self) -> Result<()> {
        Ok(())
    }

    pub fn rollback_transaction(&self) -> Result<()> {
        Ok(())
    }
}
