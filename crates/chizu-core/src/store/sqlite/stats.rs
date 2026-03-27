use super::SqliteStore;
use crate::error::Result;
use crate::store::stats::GraphStats;

impl SqliteStore {
    pub fn stats(&self) -> Result<GraphStats> {
        let count = |table: &str| -> Result<u64> {
            let sql = format!("SELECT COUNT(*) FROM {table}");
            let n: i64 = self.conn().query_row(&sql, [], |row| row.get(0))?;
            Ok(n as u64)
        };

        Ok(GraphStats {
            entities: count("entities")?,
            edges: count("edges")?,
            files: count("files")?,
            summaries: count("summaries")?,
            task_routes: count("task_routes")?,
            embeddings: count("embeddings")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    #[test]
    fn stats_empty_store() {
        let store = SqliteStore::open_in_memory().unwrap();
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
        let store = SqliteStore::open_in_memory().unwrap();

        store
            .insert_entity(&Entity {
                id: "comp::a".to_string(),
                kind: EntityKind::Component,
                name: "a".to_string(),
                component_id: None,
                path: None,
                language: None,
                line_start: None,
                line_end: None,
                visibility: None,
                exported: false,
            })
            .unwrap();

        store
            .insert_edge(&Edge {
                src_id: "comp::a".to_string(),
                rel: EdgeKind::Contains,
                dst_id: "comp::b".to_string(),
                provenance_path: None,
                provenance_line: None,
            })
            .unwrap();

        let stats = store.stats().unwrap();
        assert_eq!(stats.entities, 1);
        assert_eq!(stats.edges, 1);
        assert_eq!(stats.files, 0);
    }
}
