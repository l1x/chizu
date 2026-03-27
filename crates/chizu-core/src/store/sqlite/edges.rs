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

    // --- Traversal ---

    /// Walk forward from `start` following edges of kind `rel` up to `max_depth` hops.
    /// Returns entity IDs in BFS order (by depth).
    pub fn walk_forward(
        &self,
        start: &str,
        rel: EdgeKind,
        max_depth: usize,
    ) -> Result<Vec<String>> {
        if max_depth == 0 {
            return Ok(vec![]);
        }

        let mut stmt = self.conn.prepare(
            "WITH RECURSIVE chain(eid, depth) AS (
                SELECT dst_id, 1 FROM edges WHERE src_id = ?1 AND rel = ?2
                UNION ALL
                SELECT e.dst_id, c.depth + 1
                FROM edges e JOIN chain c ON e.src_id = c.eid
                WHERE e.rel = ?2 AND c.depth < ?3
            )
            SELECT eid FROM chain ORDER BY depth",
        )?;

        let rows = stmt.query_map(
            rusqlite::params![start, rel.as_str(), max_depth as i64],
            |row| row.get::<_, String>(0),
        )?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Walk backward from `start` following edges of kind `rel` up to `max_depth` hops.
    /// Returns entity IDs in BFS order (by depth).
    pub fn walk_backward(
        &self,
        start: &str,
        rel: EdgeKind,
        max_depth: usize,
    ) -> Result<Vec<String>> {
        if max_depth == 0 {
            return Ok(vec![]);
        }

        let mut stmt = self.conn.prepare(
            "WITH RECURSIVE chain(eid, depth) AS (
                SELECT src_id, 1 FROM edges WHERE dst_id = ?1 AND rel = ?2
                UNION ALL
                SELECT e.src_id, c.depth + 1
                FROM edges e JOIN chain c ON e.dst_id = c.eid
                WHERE e.rel = ?2 AND c.depth < ?3
            )
            SELECT eid FROM chain ORDER BY depth",
        )?;

        let rows = stmt.query_map(
            rusqlite::params![start, rel.as_str(), max_depth as i64],
            |row| row.get::<_, String>(0),
        )?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Find all entities reachable from `start` within `max_depth` hops (any edge kind).
    /// Uses UNION for deduplication and cycle prevention.
    pub fn reachable_entities(&self, start: &str, max_depth: usize) -> Result<Vec<String>> {
        if max_depth == 0 {
            return Ok(vec![]);
        }

        let mut stmt = self.conn.prepare(
            "WITH RECURSIVE reachable(eid, depth) AS (
                SELECT dst_id, 1 FROM edges WHERE src_id = ?1
                UNION
                SELECT e.dst_id, r.depth + 1
                FROM edges e JOIN reachable r ON e.src_id = r.eid
                WHERE r.depth < ?2
            )
            SELECT DISTINCT eid FROM reachable ORDER BY eid",
        )?;

        let rows = stmt.query_map(rusqlite::params![start, max_depth as i64], |row| {
            row.get::<_, String>(0)
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
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

    // --- Traversal tests ---

    fn dep_edge(src: &str, dst: &str) -> Edge {
        Edge {
            src_id: src.to_string(),
            rel: EdgeKind::DependsOn,
            dst_id: dst.to_string(),
            provenance_path: None,
            provenance_line: None,
        }
    }

    /// Sets up a chain: a -(DependsOn)-> b -(DependsOn)-> c -(DependsOn)-> d
    fn setup_chain(store: &SqliteStore) {
        store.insert_edge(&dep_edge("a", "b")).unwrap();
        store.insert_edge(&dep_edge("b", "c")).unwrap();
        store.insert_edge(&dep_edge("c", "d")).unwrap();
    }

    #[test]
    fn walk_forward_traversal() {
        let store = SqliteStore::open_in_memory().unwrap();
        setup_chain(&store);

        let result = store.walk_forward("a", EdgeKind::DependsOn, 10).unwrap();
        assert_eq!(result, vec!["b", "c", "d"]);
    }

    #[test]
    fn walk_forward_max_depth() {
        let store = SqliteStore::open_in_memory().unwrap();
        setup_chain(&store);

        let result = store.walk_forward("a", EdgeKind::DependsOn, 2).unwrap();
        assert_eq!(result, vec!["b", "c"]);

        let result = store.walk_forward("a", EdgeKind::DependsOn, 1).unwrap();
        assert_eq!(result, vec!["b"]);
    }

    #[test]
    fn walk_forward_max_depth_zero() {
        let store = SqliteStore::open_in_memory().unwrap();
        setup_chain(&store);

        let result = store.walk_forward("a", EdgeKind::DependsOn, 0).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn walk_backward_traversal() {
        let store = SqliteStore::open_in_memory().unwrap();
        setup_chain(&store);

        let result = store.walk_backward("d", EdgeKind::DependsOn, 10).unwrap();
        assert_eq!(result, vec!["c", "b", "a"]);
    }

    #[test]
    fn walk_backward_max_depth() {
        let store = SqliteStore::open_in_memory().unwrap();
        setup_chain(&store);

        let result = store.walk_backward("d", EdgeKind::DependsOn, 2).unwrap();
        assert_eq!(result, vec!["c", "b"]);
    }

    #[test]
    fn reachable_entities_all_kinds() {
        let store = SqliteStore::open_in_memory().unwrap();
        // Chain with mixed edge kinds: a -(DependsOn)-> b -(Contains)-> c -(DependsOn)-> d
        store.insert_edge(&dep_edge("a", "b")).unwrap();
        store
            .insert_edge(&Edge {
                src_id: "b".to_string(),
                rel: EdgeKind::Contains,
                dst_id: "c".to_string(),
                provenance_path: None,
                provenance_line: None,
            })
            .unwrap();
        store.insert_edge(&dep_edge("c", "d")).unwrap();

        let result = store.reachable_entities("a", 10).unwrap();
        assert_eq!(result, vec!["b", "c", "d"]);
    }

    #[test]
    fn reachable_entities_dedup_cycles() {
        let store = SqliteStore::open_in_memory().unwrap();
        // Create a cycle: a -> b -> c -> a
        store.insert_edge(&dep_edge("a", "b")).unwrap();
        store.insert_edge(&dep_edge("b", "c")).unwrap();
        store.insert_edge(&dep_edge("c", "a")).unwrap();

        // Should not infinite loop and should return unique entities
        // a is reachable via cycle (c -> a), so it is included
        let result = store.reachable_entities("a", 10).unwrap();
        assert_eq!(result.len(), 3); // a, b, c (all reachable in cycle)
        assert!(result.contains(&"a".to_string()));
        assert!(result.contains(&"b".to_string()));
        assert!(result.contains(&"c".to_string()));
    }

    #[test]
    fn reachable_entities_max_depth_zero() {
        let store = SqliteStore::open_in_memory().unwrap();
        setup_chain(&store);

        let result = store.reachable_entities("a", 0).unwrap();
        assert!(result.is_empty());
    }
}
