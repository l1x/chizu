use ::usearch::{Index, IndexOptions, MetricKind, ScalarKind};

use super::SqliteStore;
use crate::error::Result;
use crate::model::VectorSearchResult;
use crate::store::sqlite::embeddings::entity_id_to_key;

impl SqliteStore {
    /// Ensure the in-memory usearch index is loaded, creating it from the
    /// persisted `.usearch` file if available. `dimensions` is needed when
    /// creating a fresh index (e.g. during upsert); pass 0 when only loading
    /// a persisted index (the file carries its own dimension metadata).
    fn ensure_usearch_index(&self, dimensions: usize) -> Result<()> {
        let mut idx_ref = self.vector_index.borrow_mut();
        if idx_ref.is_some() {
            return Ok(());
        }

        let dims = if dimensions > 0 {
            dimensions
        } else {
            // Infer from the first embedding row in SQLite
            let maybe: rusqlite::Result<i64> =
                self.conn
                    .query_row("SELECT dimensions FROM embeddings LIMIT 1", [], |r| {
                        r.get(0)
                    });
            match maybe {
                Ok(d) if d > 0 => d as usize,
                _ => return Ok(()), // nothing to load
            }
        };

        let opts = IndexOptions {
            dimensions: dims,
            metric: MetricKind::Cos,
            quantization: ScalarKind::F32,
            ..Default::default()
        };
        let index = Index::new(&opts)
            .map_err(|e| crate::error::ChizuError::Other(format!("usearch: {e}")))?;
        index
            .reserve(1024)
            .map_err(|e| crate::error::ChizuError::Other(format!("usearch: {e}")))?;

        // Try loading persisted index
        if let Some(ref path) = self.db_path {
            let idx_path = format!("{path}.usearch");
            if std::path::Path::new(&idx_path).exists() {
                index
                    .load(&idx_path)
                    .map_err(|e| crate::error::ChizuError::Other(format!("usearch: {e}")))?;
            }
        }
        *idx_ref = Some(index);
        Ok(())
    }

    pub fn usearch_upsert(&self, entity_id: &str, vector: &[f32]) -> Result<()> {
        let key = entity_id_to_key(entity_id) as u64;
        self.ensure_usearch_index(vector.len())?;

        let idx_ref = self.vector_index.borrow();
        let index = idx_ref.as_ref().unwrap();

        // Ensure capacity - reserve more if needed
        let current_size = index.size();
        let capacity = index.capacity();
        if current_size >= capacity {
            let new_capacity = (capacity * 2).max(1024);
            index
                .reserve(new_capacity)
                .map_err(|e| crate::error::ChizuError::Other(format!("usearch: {e}")))?;
        }

        // Idempotent upsert: remove old key first (ignore errors if not found)
        let _ = index.remove(key);
        index
            .add(key, vector)
            .map_err(|e| crate::error::ChizuError::Other(format!("usearch: {e}")))?;

        // Persist if file-backed
        if let Some(ref path) = self.db_path {
            let idx_path = format!("{path}.usearch");
            index
                .save(&idx_path)
                .map_err(|e| crate::error::ChizuError::Other(format!("usearch: {e}")))?;
        }

        Ok(())
    }

    pub fn usearch_remove(&self, entity_id: &str) -> Result<()> {
        let key = entity_id_to_key(entity_id) as u64;
        let idx_ref = self.vector_index.borrow();
        if let Some(ref index) = *idx_ref {
            let _ = index.remove(key);
            // Persist if file-backed
            if let Some(ref path) = self.db_path {
                let idx_path = format!("{path}.usearch");
                index
                    .save(&idx_path)
                    .map_err(|e| crate::error::ChizuError::Other(format!("usearch: {e}")))?;
            }
        }
        Ok(())
    }

    pub fn vector_search(&self, query: &[f32], k: usize) -> Result<Vec<VectorSearchResult>> {
        // Ensure the persisted index is loaded (dimensions=0 → infer from DB)
        self.ensure_usearch_index(0)?;

        let idx_ref = self.vector_index.borrow();
        let index = match *idx_ref {
            Some(ref i) if i.size() > 0 => i,
            _ => return Ok(Vec::new()),
        };

        let results = index
            .search(query, k)
            .map_err(|e| crate::error::ChizuError::Other(format!("usearch: {e}")))?;

        let mut out = Vec::with_capacity(results.keys.len());
        for (key, dist) in results.keys.iter().zip(results.distances.iter()) {
            match self.entity_id_for_usearch_key(*key as i64) {
                Ok(entity_id) => out.push(VectorSearchResult {
                    entity_id,
                    distance: *dist,
                }),
                Err(_) => continue,
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EmbeddingRecord;

    fn make_store_with_embedding(entity_id: &str, vector: Vec<f32>) -> SqliteStore {
        let store = SqliteStore::open_in_memory().unwrap();
        let emb = EmbeddingRecord {
            entity_id: entity_id.to_string(),
            model: "test".to_string(),
            dimensions: vector.len() as i64,
            vector,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        store.upsert_embedding(&emb).unwrap();
        store
    }

    #[test]
    fn upsert_and_search() {
        let store = make_store_with_embedding("a", vec![1.0, 0.0, 0.0]);
        // Insert a second
        let emb_b = EmbeddingRecord {
            entity_id: "b".to_string(),
            model: "test".to_string(),
            dimensions: 3,
            vector: vec![0.0, 1.0, 0.0],
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        store.upsert_embedding(&emb_b).unwrap();

        let results = store.vector_search(&[1.0, 0.0, 0.0], 2).unwrap();
        assert_eq!(results.len(), 2);
        // The closest should be "a" (identical vector)
        assert_eq!(results[0].entity_id, "a");
    }

    #[test]
    fn upsert_replaces() {
        let store = make_store_with_embedding("a", vec![1.0, 0.0, 0.0]);
        // Replace with different vector
        let emb = EmbeddingRecord {
            entity_id: "a".to_string(),
            model: "test".to_string(),
            dimensions: 3,
            vector: vec![0.0, 0.0, 1.0],
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        store.upsert_embedding(&emb).unwrap();

        let results = store.vector_search(&[0.0, 0.0, 1.0], 1).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entity_id, "a");
        // Should be very close to 0 distance (identical vectors)
        assert!(results[0].distance < 0.01);
    }

    #[test]
    fn remove_then_search_empty() {
        let store = make_store_with_embedding("a", vec![1.0, 0.0, 0.0]);
        store.delete_embedding("a").unwrap();
        let results = store.vector_search(&[1.0, 0.0, 0.0], 5).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_empty_index() {
        let store = SqliteStore::open_in_memory().unwrap();
        let results = store.vector_search(&[1.0, 0.0, 0.0], 5).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_loads_persisted_index() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db_str = db_path.to_str().unwrap();

        // Store 1: embed two vectors and persist
        {
            let store = SqliteStore::open(db_str).unwrap();
            let emb_a = EmbeddingRecord {
                entity_id: "a".to_string(),
                model: "test".to_string(),
                dimensions: 3,
                vector: vec![1.0, 0.0, 0.0],
                updated_at: "2026-01-01T00:00:00Z".to_string(),
            };
            store.upsert_embedding(&emb_a).unwrap();
            let emb_b = EmbeddingRecord {
                entity_id: "b".to_string(),
                model: "test".to_string(),
                dimensions: 3,
                vector: vec![0.0, 1.0, 0.0],
                updated_at: "2026-01-01T00:00:00Z".to_string(),
            };
            store.upsert_embedding(&emb_b).unwrap();
        }

        // Store 2: fresh open, no upsert — vector_search must load from disk
        {
            let store = SqliteStore::open(db_str).unwrap();
            let results = store.vector_search(&[1.0, 0.0, 0.0], 2).unwrap();
            assert_eq!(results.len(), 2);
            assert_eq!(results[0].entity_id, "a");
        }
    }

    #[test]
    fn search_no_embeddings_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("empty.db");
        let db_str = db_path.to_str().unwrap();

        let store = SqliteStore::open(db_str).unwrap();
        let results = store.vector_search(&[1.0, 0.0, 0.0], 5).unwrap();
        assert!(results.is_empty());
    }
}
