use std::collections::HashMap;
use std::sync::Arc;

use grafeo::Value;

use super::entities::val_to_string;
use super::GrafeoStore;
use crate::error::{GraknoError, Result};
use crate::model::{EmbeddingRecord, VectorSearchResult};

impl GrafeoStore {
    pub fn upsert_embedding(&self, emb: &EmbeddingRecord) -> Result<()> {
        let sess = self.session();

        // Delete existing (upsert)
        let mut params = HashMap::new();
        params.insert("entity_id".to_string(), Value::from(emb.entity_id.as_str()));
        sess.execute_with_params(
            "MATCH (n:embedding) WHERE n.entity_id = $entity_id DETACH DELETE n",
            params,
        )
        .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;

        let mut props: Vec<(&str, Value)> = vec![
            ("entity_id", Value::from(emb.entity_id.as_str())),
            ("model", Value::from(emb.model.as_str())),
            ("dimensions", Value::from(emb.dimensions)),
            ("updated_at", Value::from(emb.updated_at.as_str())),
        ];
        if !emb.vector.is_empty() {
            let arc: Arc<[f32]> = emb.vector.clone().into();
            props.push(("vector", Value::Vector(arc)));
        }

        sess.create_node_with_props(&["embedding"], props);
        Ok(())
    }

    pub fn get_embedding(&self, entity_id: &str) -> Result<EmbeddingRecord> {
        let sess = self.session();
        let mut params = HashMap::new();
        params.insert("entity_id".to_string(), Value::from(entity_id));
        let result = sess
            .execute_with_params(
                "MATCH (n:embedding) WHERE n.entity_id = $entity_id RETURN n.entity_id, n.model, n.dimensions, n.vector, n.updated_at",
                params,
            )
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;

        let rows: Vec<Vec<Value>> = result.rows;
        if rows.is_empty() {
            return Err(GraknoError::NotFound(format!("embedding: {entity_id}")));
        }

        let vector = rows[0][3]
            .as_vector()
            .map(|v| v.to_vec())
            .unwrap_or_default();

        Ok(EmbeddingRecord {
            entity_id: val_to_string(&rows[0][0]),
            model: val_to_string(&rows[0][1]),
            dimensions: rows[0][2].as_int64().unwrap_or(0),
            vector,
            updated_at: val_to_string(&rows[0][4]),
        })
    }

    pub fn list_embeddings(&self) -> Result<Vec<EmbeddingRecord>> {
        let sess = self.session();
        let result = sess
            .execute(
                "MATCH (n:embedding) RETURN n.entity_id, n.model, n.dimensions, n.vector, n.updated_at",
            )
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;

        let mut out = Vec::new();
        for row in &result.rows {
            let vector = row[3]
                .as_vector()
                .map(|v| v.to_vec())
                .unwrap_or_default();
            out.push(EmbeddingRecord {
                entity_id: val_to_string(&row[0]),
                model: val_to_string(&row[1]),
                dimensions: row[2].as_int64().unwrap_or(0),
                vector,
                updated_at: val_to_string(&row[4]),
            });
        }
        Ok(out)
    }

    pub fn delete_embedding(&self, entity_id: &str) -> Result<bool> {
        let sess = self.session();
        let mut params = HashMap::new();
        params.insert("entity_id".to_string(), Value::from(entity_id));

        let result = sess
            .execute_with_params(
                "MATCH (n:embedding) WHERE n.entity_id = $entity_id RETURN n.entity_id",
                params.clone(),
            )
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;
        let rows: Vec<Vec<Value>> = result.rows;
        if rows.is_empty() {
            return Ok(false);
        }

        sess.execute_with_params(
            "MATCH (n:embedding) WHERE n.entity_id = $entity_id DETACH DELETE n",
            params,
        )
        .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;
        Ok(true)
    }

    pub fn vector_search(&self, query: &[f32], k: usize) -> Result<Vec<VectorSearchResult>> {
        let sess = self.session();
        let mut params = HashMap::new();
        let arc: Arc<[f32]> = query.to_vec().into();
        params.insert("query".to_string(), Value::Vector(arc));

        let gql = format!(
            "MATCH (n:embedding) \
             RETURN n.entity_id, cosine_distance(n.vector, $query) AS dist \
             ORDER BY dist \
             LIMIT {k}"
        );
        let result = sess
            .execute_with_params(&gql, params)
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;

        let mut out = Vec::new();
        for row in &result.rows {
            let eid = val_to_string(&row[0]);
            let dist = row[1].as_float64().unwrap_or(1.0) as f32;
            out.push(VectorSearchResult {
                entity_id: eid,
                distance: dist,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::grafeo::GrafeoStore;

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
        let store = GrafeoStore::open_in_memory().unwrap();
        let e = test_embedding("comp::a");
        store.upsert_embedding(&e).unwrap();
        let got = store.get_embedding("comp::a").unwrap();
        assert_eq!(e, got);
    }

    #[test]
    fn upsert_replace_embedding() {
        let store = GrafeoStore::open_in_memory().unwrap();
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
        let store = GrafeoStore::open_in_memory().unwrap();
        store.upsert_embedding(&test_embedding("comp::x")).unwrap();
        assert!(store.delete_embedding("comp::x").unwrap());
        assert!(!store.delete_embedding("comp::x").unwrap());
    }

    #[test]
    fn get_missing_embedding() {
        let store = GrafeoStore::open_in_memory().unwrap();
        let err = store.get_embedding("nope").unwrap_err();
        assert!(matches!(err, GraknoError::NotFound(_)));
    }

    #[test]
    fn list_all_embeddings() {
        let store = GrafeoStore::open_in_memory().unwrap();
        store.upsert_embedding(&test_embedding("a")).unwrap();
        store.upsert_embedding(&test_embedding("b")).unwrap();
        let list = store.list_embeddings().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn vector_search_cosine() {
        let store = GrafeoStore::open_in_memory().unwrap();
        store
            .upsert_embedding(&EmbeddingRecord {
                entity_id: "a".to_string(),
                model: "test".to_string(),
                dimensions: 3,
                vector: vec![1.0, 0.0, 0.0],
                updated_at: "2026-01-01T00:00:00Z".to_string(),
            })
            .unwrap();
        store
            .upsert_embedding(&EmbeddingRecord {
                entity_id: "b".to_string(),
                model: "test".to_string(),
                dimensions: 3,
                vector: vec![0.0, 1.0, 0.0],
                updated_at: "2026-01-01T00:00:00Z".to_string(),
            })
            .unwrap();

        let results = store.vector_search(&[1.0, 0.0, 0.0], 2).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].entity_id, "a");
        assert!(results[0].distance < 0.01);
    }
}

