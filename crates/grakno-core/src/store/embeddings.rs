use crate::error::{GraknoError, Result};
use crate::model::EmbeddingRecord;
use crate::store::Store;

impl Store {
    pub fn upsert_embedding(&self, emb: &EmbeddingRecord) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO embeddings
             (entity_id, model, dimensions, vector_ref, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                emb.entity_id,
                emb.model,
                emb.dimensions,
                emb.vector_ref,
                emb.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_embedding(&self, entity_id: &str) -> Result<EmbeddingRecord> {
        self.conn
            .query_row(
                "SELECT entity_id, model, dimensions, vector_ref, updated_at
                 FROM embeddings WHERE entity_id = ?1",
                [entity_id],
                |row| {
                    Ok(EmbeddingRecord {
                        entity_id: row.get(0)?,
                        model: row.get(1)?,
                        dimensions: row.get(2)?,
                        vector_ref: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    GraknoError::NotFound(format!("embedding: {entity_id}"))
                }
                other => GraknoError::Sqlite(other),
            })
    }

    pub fn delete_embedding(&self, entity_id: &str) -> Result<bool> {
        let count = self
            .conn
            .execute("DELETE FROM embeddings WHERE entity_id = ?1", [entity_id])?;
        Ok(count > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_embedding(entity_id: &str) -> EmbeddingRecord {
        EmbeddingRecord {
            entity_id: entity_id.to_string(),
            model: "text-embedding-3-small".to_string(),
            dimensions: 1536,
            vector_ref: Some("vec_001".to_string()),
            updated_at: "2026-03-21T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn upsert_get_embedding() {
        let store = Store::open_in_memory().unwrap();
        let e = test_embedding("comp::a");
        store.upsert_embedding(&e).unwrap();
        let got = store.get_embedding("comp::a").unwrap();
        assert_eq!(e, got);
    }

    #[test]
    fn upsert_replaces_embedding() {
        let store = Store::open_in_memory().unwrap();
        store.upsert_embedding(&test_embedding("comp::a")).unwrap();
        let updated = EmbeddingRecord {
            dimensions: 768,
            ..test_embedding("comp::a")
        };
        store.upsert_embedding(&updated).unwrap();
        let got = store.get_embedding("comp::a").unwrap();
        assert_eq!(got.dimensions, 768);
    }

    #[test]
    fn delete_embedding() {
        let store = Store::open_in_memory().unwrap();
        store.upsert_embedding(&test_embedding("comp::x")).unwrap();
        assert!(store.delete_embedding("comp::x").unwrap());
        assert!(!store.delete_embedding("comp::x").unwrap());
    }

    #[test]
    fn get_missing_embedding() {
        let store = Store::open_in_memory().unwrap();
        let err = store.get_embedding("nope").unwrap_err();
        assert!(matches!(err, GraknoError::NotFound(_)));
    }
}
