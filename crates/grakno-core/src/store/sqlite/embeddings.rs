use super::SqliteStore;
use crate::error::{GraknoError, Result};
use crate::model::EmbeddingRecord;

pub fn entity_id_to_key(entity_id: &str) -> i64 {
    let hash = blake3::hash(entity_id.as_bytes());
    // Use the first 8 bytes as a signed i64 (SQLite INTEGER is signed)
    i64::from_le_bytes(hash.as_bytes()[..8].try_into().unwrap())
}

impl SqliteStore {
    pub fn upsert_embedding(&self, emb: &EmbeddingRecord) -> Result<()> {
        let key = entity_id_to_key(&emb.entity_id);
        self.conn.execute(
            "INSERT OR REPLACE INTO embeddings
             (entity_id, model, dimensions, updated_at, usearch_key)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                emb.entity_id,
                emb.model,
                emb.dimensions,
                emb.updated_at,
                key
            ],
        )?;

        #[cfg(feature = "usearch")]
        if !emb.vector.is_empty() {
            self.usearch_upsert(&emb.entity_id, &emb.vector)?;
        }

        Ok(())
    }

    pub fn get_embedding(&self, entity_id: &str) -> Result<EmbeddingRecord> {
        let row = self
            .conn
            .query_row(
                "SELECT entity_id, model, dimensions, updated_at, usearch_key
                 FROM embeddings WHERE entity_id = ?1",
                [entity_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                    ))
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    GraknoError::NotFound(format!("embedding: {entity_id}"))
                }
                other => GraknoError::Sqlite(other),
            })?;

        let vector = self.vector_for_key(row.4, row.2 as usize);

        Ok(EmbeddingRecord {
            entity_id: row.0,
            model: row.1,
            dimensions: row.2,
            vector,
            updated_at: row.3,
        })
    }

    pub fn list_embeddings(&self) -> Result<Vec<EmbeddingRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT entity_id, model, dimensions, updated_at, usearch_key FROM embeddings",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let row = row?;
            let vector = self.vector_for_key(row.4, row.2 as usize);
            out.push(EmbeddingRecord {
                entity_id: row.0,
                model: row.1,
                dimensions: row.2,
                vector,
                updated_at: row.3,
            });
        }
        Ok(out)
    }

    pub fn delete_embedding(&self, entity_id: &str) -> Result<bool> {
        #[cfg(feature = "usearch")]
        self.usearch_remove(entity_id)?;

        let count = self
            .conn
            .execute("DELETE FROM embeddings WHERE entity_id = ?1", [entity_id])?;
        Ok(count > 0)
    }

    pub fn entity_id_for_usearch_key(&self, key: i64) -> Result<String> {
        self.conn
            .query_row(
                "SELECT entity_id FROM embeddings WHERE usearch_key = ?1",
                [key],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    GraknoError::NotFound(format!("usearch_key: {key}"))
                }
                other => GraknoError::Sqlite(other),
            })
    }

    /// Extract vector from usearch index by key, or return empty vec if unavailable.
    fn vector_for_key(&self, usearch_key: Option<i64>, dimensions: usize) -> Vec<f32> {
        #[cfg(feature = "usearch")]
        {
            if let Some(key) = usearch_key {
                let idx_ref = self.vector_index.borrow();
                if let Some(ref index) = *idx_ref {
                    let mut buf = vec![0.0f32; dimensions];
                    if index.get(key as u64, &mut buf).is_ok() {
                        return buf;
                    }
                }
            }
        }
        #[cfg(not(feature = "usearch"))]
        let _ = (usearch_key, dimensions);
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_embedding(entity_id: &str) -> EmbeddingRecord {
        EmbeddingRecord {
            entity_id: entity_id.to_string(),
            model: "text-embedding-3-small".to_string(),
            dimensions: 3,
            vector: vec![0.1, 0.2, 0.3],
            updated_at: "2026-03-21T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn upsert_get_embedding() {
        let store = SqliteStore::open_in_memory().unwrap();
        let e = test_embedding("comp::a");
        store.upsert_embedding(&e).unwrap();
        let got = store.get_embedding("comp::a").unwrap();
        // Without usearch feature, vector comes back empty
        #[cfg(feature = "usearch")]
        assert_eq!(e, got);
        #[cfg(not(feature = "usearch"))]
        {
            assert_eq!(got.entity_id, e.entity_id);
            assert_eq!(got.model, e.model);
            assert_eq!(got.dimensions, e.dimensions);
            assert!(got.vector.is_empty());
        }
    }

    #[test]
    fn upsert_replaces_embedding() {
        let store = SqliteStore::open_in_memory().unwrap();
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
        let store = SqliteStore::open_in_memory().unwrap();
        store.upsert_embedding(&test_embedding("comp::x")).unwrap();
        assert!(store.delete_embedding("comp::x").unwrap());
        assert!(!store.delete_embedding("comp::x").unwrap());
    }

    #[test]
    fn get_missing_embedding() {
        let store = SqliteStore::open_in_memory().unwrap();
        let err = store.get_embedding("nope").unwrap_err();
        assert!(matches!(err, GraknoError::NotFound(_)));
    }

    #[test]
    fn list_embeddings_returns_all() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.upsert_embedding(&test_embedding("a")).unwrap();
        store.upsert_embedding(&test_embedding("b")).unwrap();
        let list = store.list_embeddings().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn usearch_key_stored_on_upsert() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.upsert_embedding(&test_embedding("comp::a")).unwrap();
        let key = entity_id_to_key("comp::a");
        let eid = store.entity_id_for_usearch_key(key).unwrap();
        assert_eq!(eid, "comp::a");
    }
}
