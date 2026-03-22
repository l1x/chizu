pub mod schema;
mod sqlite;
pub mod stats;

#[cfg(feature = "grafeo")]
mod grafeo;

pub use sqlite::SqliteStore;

#[cfg(feature = "grafeo")]
pub use grafeo::GrafeoStore;

use crate::error::Result;
use crate::model::*;
use stats::GraphStats;

pub enum Store {
    Sqlite(SqliteStore),
    #[cfg(feature = "grafeo")]
    Grafeo(GrafeoStore),
}

impl Store {
    pub fn open(path: &str) -> Result<Self> {
        Ok(Self::Sqlite(SqliteStore::open(path)?))
    }

    pub fn open_in_memory() -> Result<Self> {
        Ok(Self::Sqlite(SqliteStore::open_in_memory()?))
    }

    #[cfg(feature = "grafeo")]
    pub fn open_grafeo(path: &str) -> Result<Self> {
        Ok(Self::Grafeo(GrafeoStore::open(path)?))
    }

    #[cfg(feature = "grafeo")]
    pub fn open_grafeo_in_memory() -> Result<Self> {
        Ok(Self::Grafeo(GrafeoStore::open_in_memory()?))
    }

    pub fn schema_version(&self) -> Result<Option<i64>> {
        match self {
            Self::Sqlite(s) => s.schema_version().map(Some),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(_) => Ok(None),
        }
    }

    // --- Entities ---

    pub fn insert_entity(&self, entity: &Entity) -> Result<()> {
        match self {
            Self::Sqlite(s) => s.insert_entity(entity),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.insert_entity(entity),
        }
    }

    pub fn get_entity(&self, id: &str) -> Result<Entity> {
        match self {
            Self::Sqlite(s) => s.get_entity(id),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.get_entity(id),
        }
    }

    pub fn list_entities(&self) -> Result<Vec<Entity>> {
        match self {
            Self::Sqlite(s) => s.list_entities(),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.list_entities(),
        }
    }

    pub fn list_entities_by_component(&self, component_id: &str) -> Result<Vec<Entity>> {
        match self {
            Self::Sqlite(s) => s.list_entities_by_component(component_id),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.list_entities_by_component(component_id),
        }
    }

    pub fn list_entities_by_path(&self, path: &str) -> Result<Vec<Entity>> {
        match self {
            Self::Sqlite(s) => s.list_entities_by_path(path),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.list_entities_by_path(path),
        }
    }

    pub fn delete_entity(&self, id: &str) -> Result<bool> {
        match self {
            Self::Sqlite(s) => s.delete_entity(id),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.delete_entity(id),
        }
    }

    // --- Edges ---

    pub fn insert_edge(&self, edge: &Edge) -> Result<()> {
        match self {
            Self::Sqlite(s) => s.insert_edge(edge),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.insert_edge(edge),
        }
    }

    pub fn edges_from(&self, src_id: &str) -> Result<Vec<Edge>> {
        match self {
            Self::Sqlite(s) => s.edges_from(src_id),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.edges_from(src_id),
        }
    }

    pub fn edges_to(&self, dst_id: &str) -> Result<Vec<Edge>> {
        match self {
            Self::Sqlite(s) => s.edges_to(dst_id),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.edges_to(dst_id),
        }
    }

    pub fn delete_edges_from(&self, src_id: &str) -> Result<usize> {
        match self {
            Self::Sqlite(s) => s.delete_edges_from(src_id),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.delete_edges_from(src_id),
        }
    }

    pub fn delete_edges_to(&self, dst_id: &str) -> Result<usize> {
        match self {
            Self::Sqlite(s) => s.delete_edges_to(dst_id),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.delete_edges_to(dst_id),
        }
    }

    pub fn delete_edge(&self, src_id: &str, rel: EdgeKind, dst_id: &str) -> Result<bool> {
        match self {
            Self::Sqlite(s) => s.delete_edge(src_id, rel, dst_id),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.delete_edge(src_id, rel, dst_id),
        }
    }

    // --- Files ---

    pub fn insert_file(&self, file: &FileRecord) -> Result<()> {
        match self {
            Self::Sqlite(s) => s.insert_file(file),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.insert_file(file),
        }
    }

    pub fn get_file(&self, path: &str) -> Result<FileRecord> {
        match self {
            Self::Sqlite(s) => s.get_file(path),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.get_file(path),
        }
    }

    pub fn list_files(&self, component_id: Option<&str>) -> Result<Vec<FileRecord>> {
        match self {
            Self::Sqlite(s) => s.list_files(component_id),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.list_files(component_id),
        }
    }

    pub fn delete_file(&self, path: &str) -> Result<bool> {
        match self {
            Self::Sqlite(s) => s.delete_file(path),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.delete_file(path),
        }
    }

    // --- Summaries ---

    pub fn upsert_summary(&self, summary: &Summary) -> Result<()> {
        match self {
            Self::Sqlite(s) => s.upsert_summary(summary),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.upsert_summary(summary),
        }
    }

    pub fn get_summary(&self, entity_id: &str) -> Result<Summary> {
        match self {
            Self::Sqlite(s) => s.get_summary(entity_id),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.get_summary(entity_id),
        }
    }

    pub fn delete_summary(&self, entity_id: &str) -> Result<bool> {
        match self {
            Self::Sqlite(s) => s.delete_summary(entity_id),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.delete_summary(entity_id),
        }
    }

    // --- Embeddings ---

    pub fn upsert_embedding(&self, emb: &EmbeddingRecord) -> Result<()> {
        match self {
            Self::Sqlite(s) => s.upsert_embedding(emb),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.upsert_embedding(emb),
        }
    }

    pub fn get_embedding(&self, entity_id: &str) -> Result<EmbeddingRecord> {
        match self {
            Self::Sqlite(s) => s.get_embedding(entity_id),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.get_embedding(entity_id),
        }
    }

    pub fn delete_embedding(&self, entity_id: &str) -> Result<bool> {
        match self {
            Self::Sqlite(s) => s.delete_embedding(entity_id),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.delete_embedding(entity_id),
        }
    }

    pub fn list_embeddings(&self) -> Result<Vec<EmbeddingRecord>> {
        match self {
            Self::Sqlite(s) => s.list_embeddings(),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.list_embeddings(),
        }
    }

    pub fn vector_search(
        &self,
        _query: &[f32],
        _k: usize,
    ) -> Result<Vec<VectorSearchResult>> {
        match self {
            #[cfg(feature = "usearch")]
            Self::Sqlite(s) => s.vector_search(_query, _k),
            #[cfg(not(feature = "usearch"))]
            Self::Sqlite(_) => Err(crate::error::GraknoError::Other(
                "vector search requires the 'usearch' feature".into(),
            )),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.vector_search(_query, _k),
        }
    }

    // --- Task Routes ---

    pub fn insert_task_route(&self, route: &TaskRoute) -> Result<()> {
        match self {
            Self::Sqlite(s) => s.insert_task_route(route),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.insert_task_route(route),
        }
    }

    pub fn routes_for_task(&self, task_name: &str) -> Result<Vec<TaskRoute>> {
        match self {
            Self::Sqlite(s) => s.routes_for_task(task_name),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.routes_for_task(task_name),
        }
    }

    pub fn routes_for_entity(&self, entity_id: &str) -> Result<Vec<TaskRoute>> {
        match self {
            Self::Sqlite(s) => s.routes_for_entity(entity_id),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.routes_for_entity(entity_id),
        }
    }

    pub fn delete_task_route(&self, task_name: &str, entity_id: &str) -> Result<bool> {
        match self {
            Self::Sqlite(s) => s.delete_task_route(task_name, entity_id),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.delete_task_route(task_name, entity_id),
        }
    }

    // --- Stats ---

    pub fn stats(&self) -> Result<GraphStats> {
        match self {
            Self::Sqlite(s) => s.stats(),
            #[cfg(feature = "grafeo")]
            Self::Grafeo(g) => g.stats(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_open_in_memory() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.schema_version().unwrap(), Some(4));
    }

    #[cfg(feature = "grafeo")]
    #[test]
    fn store_open_grafeo_in_memory() {
        let store = Store::open_grafeo_in_memory().unwrap();
        assert_eq!(store.schema_version().unwrap(), None);
    }
}
