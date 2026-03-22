use std::collections::HashMap;

use grafeo::Value;

use super::entities::{val_to_opt_i64, val_to_opt_string, val_to_string};
use super::GrafeoStore;
use crate::error::{GraknoError, Result};
use crate::model::{Edge, EdgeKind};

impl GrafeoStore {
    pub fn insert_edge(&self, edge: &Edge) -> Result<()> {
        let sess = self.session();
        // Delete existing edge with same key (upsert semantics)
        let mut params = HashMap::new();
        params.insert("src".to_string(), Value::from(edge.src_id.as_str()));
        params.insert("rel".to_string(), Value::from(edge.rel.as_str()));
        params.insert("dst".to_string(), Value::from(edge.dst_id.as_str()));

        sess.execute_with_params(
            "MATCH (a:entity)-[r]->(b:entity) WHERE a.eid = $src AND type(r) = $rel AND b.eid = $dst DELETE r",
            params.clone(),
        )
        .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;

        // Build the CREATE query with edge properties
        if let Some(ref pp) = edge.provenance_path {
            params.insert("pp".to_string(), Value::from(pp.as_str()));
        }
        if let Some(pl) = edge.provenance_line {
            params.insert("pl".to_string(), Value::from(pl));
        }

        // Use dynamic edge type via GQL
        let edge_type = edge.rel.as_str();
        let query = format!(
            "MATCH (a:entity), (b:entity) WHERE a.eid = $src AND b.eid = $dst CREATE (a)-[:{edge_type} {{provenance_path: $pp, provenance_line: $pl}}]->(b)"
        );
        // Ensure params have null values for missing optional fields
        params.entry("pp".to_string()).or_insert(Value::Null);
        params.entry("pl".to_string()).or_insert(Value::Null);

        sess.execute_with_params(&query, params)
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;
        Ok(())
    }

    pub fn edges_from(&self, src_id: &str) -> Result<Vec<Edge>> {
        let sess = self.session();
        let mut params = HashMap::new();
        params.insert("src".to_string(), Value::from(src_id));
        let result = sess
            .execute_with_params(
                "MATCH (a:entity)-[r]->(b:entity) WHERE a.eid = $src RETURN a.eid, type(r), b.eid, r.provenance_path, r.provenance_line ORDER BY b.eid",
                params,
            )
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;

        let rows: Vec<Vec<Value>> = result.rows;
        rows.iter().map(|r| row_to_edge(r)).collect()
    }

    pub fn edges_to(&self, dst_id: &str) -> Result<Vec<Edge>> {
        let sess = self.session();
        let mut params = HashMap::new();
        params.insert("dst".to_string(), Value::from(dst_id));
        let result = sess
            .execute_with_params(
                "MATCH (a:entity)-[r]->(b:entity) WHERE b.eid = $dst RETURN a.eid, type(r), b.eid, r.provenance_path, r.provenance_line ORDER BY a.eid",
                params,
            )
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;

        let rows: Vec<Vec<Value>> = result.rows;
        rows.iter().map(|r| row_to_edge(r)).collect()
    }

    pub fn delete_edges_from(&self, src_id: &str) -> Result<usize> {
        let sess = self.session();
        let mut params = HashMap::new();
        params.insert("src".to_string(), Value::from(src_id));

        // Count first
        let result = sess
            .execute_with_params(
                "MATCH (a:entity)-[r]->(b:entity) WHERE a.eid = $src RETURN count(r)",
                params.clone(),
            )
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;
        let rows: Vec<Vec<Value>> = result.rows;
        let count = rows
            .first()
            .and_then(|r| r.first())
            .and_then(|v| v.as_int64())
            .unwrap_or(0) as usize;

        sess.execute_with_params(
            "MATCH (a:entity)-[r]->(b:entity) WHERE a.eid = $src DELETE r",
            params,
        )
        .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;
        Ok(count)
    }

    pub fn delete_edges_to(&self, dst_id: &str) -> Result<usize> {
        let sess = self.session();
        let mut params = HashMap::new();
        params.insert("dst".to_string(), Value::from(dst_id));

        let result = sess
            .execute_with_params(
                "MATCH (a:entity)-[r]->(b:entity) WHERE b.eid = $dst RETURN count(r)",
                params.clone(),
            )
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;
        let rows: Vec<Vec<Value>> = result.rows;
        let count = rows
            .first()
            .and_then(|r| r.first())
            .and_then(|v| v.as_int64())
            .unwrap_or(0) as usize;

        sess.execute_with_params(
            "MATCH (a:entity)-[r]->(b:entity) WHERE b.eid = $dst DELETE r",
            params,
        )
        .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;
        Ok(count)
    }

    pub fn delete_edge(&self, src_id: &str, rel: EdgeKind, dst_id: &str) -> Result<bool> {
        let sess = self.session();
        let mut params = HashMap::new();
        params.insert("src".to_string(), Value::from(src_id));
        params.insert("dst".to_string(), Value::from(dst_id));

        let edge_type = rel.as_str();

        // Check existence
        let query = format!(
            "MATCH (a:entity)-[r:{edge_type}]->(b:entity) WHERE a.eid = $src AND b.eid = $dst RETURN count(r)"
        );
        let result = sess
            .execute_with_params(&query, params.clone())
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;
        let rows: Vec<Vec<Value>> = result.rows;
        let count = rows
            .first()
            .and_then(|r| r.first())
            .and_then(|v| v.as_int64())
            .unwrap_or(0);

        if count == 0 {
            return Ok(false);
        }

        let del_query = format!(
            "MATCH (a:entity)-[r:{edge_type}]->(b:entity) WHERE a.eid = $src AND b.eid = $dst DELETE r"
        );
        sess.execute_with_params(&del_query, params)
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;
        Ok(true)
    }
}

fn row_to_edge(row: &[Value]) -> Result<Edge> {
    let rel_str = val_to_string(&row[1]);
    Ok(Edge {
        src_id: val_to_string(&row[0]),
        rel: EdgeKind::parse(&rel_str).unwrap_or(EdgeKind::RelatedTo),
        dst_id: val_to_string(&row[2]),
        provenance_path: val_to_opt_string(&row[3]),
        provenance_line: val_to_opt_i64(&row[4]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Entity, EntityKind};
    use crate::store::grafeo::GrafeoStore;

    fn setup_store_with_entities() -> GrafeoStore {
        let store = GrafeoStore::open_in_memory().unwrap();
        for id in &["a", "b", "c", "d"] {
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
        store
    }

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
        let store = setup_store_with_entities();
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
    fn delete_single_edge() {
        let store = setup_store_with_entities();
        store.insert_edge(&test_edge("a", "b")).unwrap();
        assert!(store.delete_edge("a", EdgeKind::Contains, "b").unwrap());
        assert!(!store.delete_edge("a", EdgeKind::Contains, "b").unwrap());
    }

    #[test]
    fn delete_edges_from() {
        let store = setup_store_with_entities();
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
        let store = setup_store_with_entities();
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
        let store = setup_store_with_entities();
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
