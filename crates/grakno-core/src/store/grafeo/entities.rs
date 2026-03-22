use std::collections::HashMap;

use grafeo::Value;

use super::GrafeoStore;
use crate::error::{GraknoError, Result};
use crate::model::{Entity, EntityKind};

impl GrafeoStore {
    pub fn insert_entity(&self, entity: &Entity) -> Result<()> {
        let sess = self.session();
        // Delete existing node with same eid (upsert semantics)
        let mut params = HashMap::new();
        params.insert("eid".to_string(), Value::from(entity.id.as_str()));
        sess.execute_with_params(
            "MATCH (n:entity) WHERE n.eid = $eid DETACH DELETE n",
            params,
        )
        .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;

        // Insert new node
        let mut props: Vec<(&str, Value)> = vec![
            ("eid", Value::from(entity.id.as_str())),
            ("kind", Value::from(entity.kind.as_str())),
            ("name", Value::from(entity.name.as_str())),
            ("exported", Value::from(entity.exported)),
        ];
        if let Some(ref v) = entity.component_id {
            props.push(("component_id", Value::from(v.as_str())));
        }
        if let Some(ref v) = entity.path {
            props.push(("path", Value::from(v.as_str())));
        }
        if let Some(ref v) = entity.language {
            props.push(("language", Value::from(v.as_str())));
        }
        if let Some(v) = entity.line_start {
            props.push(("line_start", Value::from(v)));
        }
        if let Some(v) = entity.line_end {
            props.push(("line_end", Value::from(v)));
        }
        if let Some(ref v) = entity.visibility {
            props.push(("visibility", Value::from(v.as_str())));
        }

        sess.create_node_with_props(&["entity"], props);
        Ok(())
    }

    pub fn get_entity(&self, id: &str) -> Result<Entity> {
        let sess = self.session();
        let mut params = HashMap::new();
        params.insert("eid".to_string(), Value::from(id));
        let result = sess
            .execute_with_params(
                "MATCH (n:entity) WHERE n.eid = $eid RETURN n.eid, n.kind, n.name, n.component_id, n.path, n.language, n.line_start, n.line_end, n.visibility, n.exported",
                params,
            )
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;

        let rows: Vec<Vec<Value>> = result.rows;
        if rows.is_empty() {
            return Err(GraknoError::NotFound(format!("entity: {id}")));
        }
        row_to_entity(&rows[0])
    }

    pub fn list_entities(&self) -> Result<Vec<Entity>> {
        let sess = self.session();
        let result = sess
            .execute(
                "MATCH (n:entity) RETURN n.eid, n.kind, n.name, n.component_id, n.path, n.language, n.line_start, n.line_end, n.visibility, n.exported ORDER BY n.eid",
            )
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;

        let rows: Vec<Vec<Value>> = result.rows;
        rows.iter().map(|r| row_to_entity(r)).collect()
    }

    pub fn list_entities_by_component(&self, component_id: &str) -> Result<Vec<Entity>> {
        let sess = self.session();
        let mut params = HashMap::new();
        params.insert("cid".to_string(), Value::from(component_id));
        let result = sess
            .execute_with_params(
                "MATCH (n:entity) WHERE n.component_id = $cid RETURN n.eid, n.kind, n.name, n.component_id, n.path, n.language, n.line_start, n.line_end, n.visibility, n.exported ORDER BY n.eid",
                params,
            )
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;

        let rows: Vec<Vec<Value>> = result.rows;
        rows.iter().map(|r| row_to_entity(r)).collect()
    }

    pub fn list_entities_by_path(&self, path: &str) -> Result<Vec<Entity>> {
        let sess = self.session();
        let mut params = HashMap::new();
        params.insert("path".to_string(), Value::from(path));
        let result = sess
            .execute_with_params(
                "MATCH (n:entity) WHERE n.path = $path RETURN n.eid, n.kind, n.name, n.component_id, n.path, n.language, n.line_start, n.line_end, n.visibility, n.exported ORDER BY n.eid",
                params,
            )
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;

        let rows: Vec<Vec<Value>> = result.rows;
        rows.iter().map(|r| row_to_entity(r)).collect()
    }

    pub fn delete_entity(&self, id: &str) -> Result<bool> {
        let sess = self.session();
        // Check if exists first
        let mut params = HashMap::new();
        params.insert("eid".to_string(), Value::from(id));
        let result = sess
            .execute_with_params(
                "MATCH (n:entity) WHERE n.eid = $eid RETURN n.eid",
                params.clone(),
            )
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;

        let rows: Vec<Vec<Value>> = result.rows;
        if rows.is_empty() {
            return Ok(false);
        }

        sess.execute_with_params(
            "MATCH (n:entity) WHERE n.eid = $eid DETACH DELETE n",
            params,
        )
        .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;
        Ok(true)
    }
}

fn row_to_entity(row: &[Value]) -> Result<Entity> {
    Ok(Entity {
        id: val_to_string(&row[0]),
        kind: EntityKind::parse(&val_to_string(&row[1])).unwrap_or(EntityKind::Symbol),
        name: val_to_string(&row[2]),
        component_id: val_to_opt_string(&row[3]),
        path: val_to_opt_string(&row[4]),
        language: val_to_opt_string(&row[5]),
        line_start: val_to_opt_i64(&row[6]),
        line_end: val_to_opt_i64(&row[7]),
        visibility: val_to_opt_string(&row[8]),
        exported: row[9].as_bool().unwrap_or(false),
    })
}

pub(crate) fn val_to_string(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

pub(crate) fn val_to_opt_string(v: &Value) -> Option<String> {
    if v.is_null() {
        None
    } else {
        Some(v.as_str().unwrap_or("").to_string())
    }
}

pub(crate) fn val_to_opt_i64(v: &Value) -> Option<i64> {
    if v.is_null() {
        None
    } else {
        v.as_int64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::grafeo::GrafeoStore;

    fn test_entity(id: &str) -> Entity {
        Entity {
            id: id.to_string(),
            kind: EntityKind::Component,
            name: "test-comp".to_string(),
            component_id: Some("root".to_string()),
            path: Some("crates/test".to_string()),
            language: Some("rust".to_string()),
            line_start: None,
            line_end: None,
            visibility: Some("pub".to_string()),
            exported: true,
        }
    }

    #[test]
    fn insert_get_entity() {
        let store = GrafeoStore::open_in_memory().unwrap();
        let e = test_entity("component::test");
        store.insert_entity(&e).unwrap();
        let got = store.get_entity("component::test").unwrap();
        assert_eq!(e, got);
    }

    #[test]
    fn get_missing_entity() {
        let store = GrafeoStore::open_in_memory().unwrap();
        let err = store.get_entity("nope").unwrap_err();
        assert!(matches!(err, GraknoError::NotFound(_)));
    }

    #[test]
    fn list_by_component() {
        let store = GrafeoStore::open_in_memory().unwrap();
        store.insert_entity(&test_entity("a")).unwrap();
        store
            .insert_entity(&Entity {
                component_id: Some("other".to_string()),
                ..test_entity("b")
            })
            .unwrap();

        let root = store.list_entities_by_component("root").unwrap();
        assert_eq!(root.len(), 1);
        assert_eq!(root[0].id, "a");
    }

    #[test]
    fn list_by_path() {
        let store = GrafeoStore::open_in_memory().unwrap();
        store
            .insert_entity(&Entity {
                path: Some("src/lib.rs".to_string()),
                ..test_entity("a")
            })
            .unwrap();
        store
            .insert_entity(&Entity {
                path: Some("src/lib.rs".to_string()),
                ..test_entity("b")
            })
            .unwrap();

        let results = store.list_entities_by_path("src/lib.rs").unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn delete_entity() {
        let store = GrafeoStore::open_in_memory().unwrap();
        store.insert_entity(&test_entity("x")).unwrap();
        assert!(store.delete_entity("x").unwrap());
        assert!(!store.delete_entity("x").unwrap());
    }
}
