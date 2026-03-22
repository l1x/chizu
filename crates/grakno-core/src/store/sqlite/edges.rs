use super::SqliteStore;
use crate::error::Result;
use crate::model::{Edge, EdgeKind};

impl SqliteStore {
    pub fn insert_edge(&self, edge: &Edge) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO edges (src_id, rel, dst_id, provenance_path, provenance_line)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                edge.src_id,
                edge.rel.as_str(),
                edge.dst_id,
                edge.provenance_path,
                edge.provenance_line,
            ],
        )?;
        Ok(())
    }

    pub fn edges_from(&self, src_id: &str) -> Result<Vec<Edge>> {
        let mut stmt = self.conn.prepare(
            "SELECT src_id, rel, dst_id, provenance_path, provenance_line
             FROM edges WHERE src_id = ?1 ORDER BY rel, dst_id",
        )?;
        let rows = stmt.query_map([src_id], Self::row_to_edge)?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn edges_to(&self, dst_id: &str) -> Result<Vec<Edge>> {
        let mut stmt = self.conn.prepare(
            "SELECT src_id, rel, dst_id, provenance_path, provenance_line
             FROM edges WHERE dst_id = ?1 ORDER BY rel, src_id",
        )?;
        let rows = stmt.query_map([dst_id], Self::row_to_edge)?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn delete_edges_from(&self, src_id: &str) -> Result<usize> {
        let count = self
            .conn
            .execute("DELETE FROM edges WHERE src_id = ?1", [src_id])?;
        Ok(count)
    }

    pub fn delete_edges_to(&self, dst_id: &str) -> Result<usize> {
        let count = self
            .conn
            .execute("DELETE FROM edges WHERE dst_id = ?1", [dst_id])?;
        Ok(count)
    }

    pub fn delete_edge(&self, src_id: &str, rel: EdgeKind, dst_id: &str) -> Result<bool> {
        let count = self.conn.execute(
            "DELETE FROM edges WHERE src_id = ?1 AND rel = ?2 AND dst_id = ?3",
            rusqlite::params![src_id, rel.as_str(), dst_id],
        )?;
        Ok(count > 0)
    }

    fn row_to_edge(row: &rusqlite::Row<'_>) -> rusqlite::Result<Edge> {
        let rel_str: String = row.get(1)?;
        Ok(Edge {
            src_id: row.get(0)?,
            rel: EdgeKind::parse(&rel_str).unwrap_or(EdgeKind::RelatedTo),
            dst_id: row.get(2)?,
            provenance_path: row.get(3)?,
            provenance_line: row.get(4)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EdgeKind;

    fn test_edge(src: &str, dst: &str) -> Edge {
        Edge {
            src_id: src.to_string(),
            rel: EdgeKind::Contains,
            dst_id: dst.to_string(),
            provenance_path: Some("Cargo.toml".to_string()),
            provenance_line: Some(10),
        }
    }

    #[test]
    fn insert_and_query_edges() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.insert_edge(&test_edge("a", "b")).unwrap();
        store.insert_edge(&test_edge("a", "c")).unwrap();
        store
            .insert_edge(&Edge {
                rel: EdgeKind::DependsOn,
                ..test_edge("b", "a")
            })
            .unwrap();

        let from_a = store.edges_from("a").unwrap();
        assert_eq!(from_a.len(), 2);

        let to_a = store.edges_to("a").unwrap();
        assert_eq!(to_a.len(), 1);
        assert_eq!(to_a[0].src_id, "b");
    }

    #[test]
    fn delete_edge() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.insert_edge(&test_edge("x", "y")).unwrap();
        assert!(store.delete_edge("x", EdgeKind::Contains, "y").unwrap());
        assert!(!store.delete_edge("x", EdgeKind::Contains, "y").unwrap());
    }

    #[test]
    fn delete_edges_from() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.insert_edge(&test_edge("a", "b")).unwrap();
        store.insert_edge(&test_edge("a", "c")).unwrap();
        store.insert_edge(&test_edge("d", "a")).unwrap();

        let deleted = store.delete_edges_from("a").unwrap();
        assert_eq!(deleted, 2);
        assert!(store.edges_from("a").unwrap().is_empty());
        assert_eq!(store.edges_to("a").unwrap().len(), 1);
    }

    #[test]
    fn delete_edges_to() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.insert_edge(&test_edge("a", "b")).unwrap();
        store.insert_edge(&test_edge("c", "b")).unwrap();
        store.insert_edge(&test_edge("b", "d")).unwrap();

        let deleted = store.delete_edges_to("b").unwrap();
        assert_eq!(deleted, 2);
        assert!(store.edges_to("b").unwrap().is_empty());
        assert_eq!(store.edges_from("b").unwrap().len(), 1);
    }

    #[test]
    fn insert_replaces_edge() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.insert_edge(&test_edge("a", "b")).unwrap();
        let updated = Edge {
            provenance_line: Some(99),
            ..test_edge("a", "b")
        };
        store.insert_edge(&updated).unwrap();
        let edges = store.edges_from("a").unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].provenance_line, Some(99));
    }
}
