use grafeo::Value;

use super::GrafeoStore;
use crate::error::{GraknoError, Result};
use crate::store::stats::GraphStats;

impl GrafeoStore {
    pub fn stats(&self) -> Result<GraphStats> {
        let sess = self.session();

        let count_label = |label: &str| -> Result<u64> {
            let query = format!("MATCH (n:{label}) RETURN count(n)");
            let result = sess
                .execute(&query)
                .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;
            let rows: Vec<Vec<Value>> = result.rows;
            Ok(rows
                .first()
                .and_then(|r| r.first())
                .and_then(|v| v.as_int64())
                .unwrap_or(0) as u64)
        };

        // Count edges via relationship pattern
        let edge_result = sess
            .execute("MATCH (:entity)-[r]->(:entity) RETURN count(r)")
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;
        let edge_rows: Vec<Vec<Value>> = edge_result.rows;
        let edges = edge_rows
            .first()
            .and_then(|r| r.first())
            .and_then(|v| v.as_int64())
            .unwrap_or(0) as u64;

        Ok(GraphStats {
            entities: count_label("entity")?,
            edges,
            files: count_label("file")?,
            summaries: count_label("summary")?,
            task_routes: count_label("task_route")?,
            embeddings: count_label("embedding")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::model::*;
    use crate::store::grafeo::GrafeoStore;
    use crate::store::stats::GraphStats;

    #[test]
    fn empty_store_stats() {
        let store = GrafeoStore::open_in_memory().unwrap();
        let stats = store.stats().unwrap();
        assert_eq!(
            stats,
            GraphStats {
                entities: 0,
                edges: 0,
                files: 0,
                summaries: 0,
                task_routes: 0,
                embeddings: 0,
            }
        );
    }

    #[test]
    fn stats_after_inserts() {
        let store = GrafeoStore::open_in_memory().unwrap();

        // Insert 2 entities
        for id in &["a", "b"] {
            store
                .insert_entity(&Entity {
                    id: id.to_string(),
                    kind: EntityKind::Component,
                    name: id.to_string(),
                    component_id: None,
                    path: None,
                    language: None,
                    line_start: None,
                    line_end: None,
                    visibility: None,
                    exported: false,
                })
                .unwrap();
        }

        // Insert 1 edge
        store
            .insert_edge(&Edge {
                src_id: "a".to_string(),
                rel: EdgeKind::Contains,
                dst_id: "b".to_string(),
                provenance_path: None,
                provenance_line: None,
            })
            .unwrap();

        // Insert 1 file
        store
            .insert_file(&FileRecord {
                path: "src/lib.rs".to_string(),
                component_id: None,
                kind: "rust".to_string(),
                hash: "abc".to_string(),
                indexed: true,
                ignore_reason: None,
            })
            .unwrap();

        let stats = store.stats().unwrap();
        assert_eq!(stats.entities, 2);
        assert_eq!(stats.edges, 1);
        assert_eq!(stats.files, 1);
    }
}
