use std::collections::HashMap;

use grafeo::Value;

use super::entities::{val_to_opt_string, val_to_string};
use super::GrafeoStore;
use crate::error::{ChizuError, Result};
use crate::model::FileRecord;

impl GrafeoStore {
    pub fn insert_file(&self, file: &FileRecord) -> Result<()> {
        let sess = self.session();
        // Delete existing (upsert)
        let mut params = HashMap::new();
        params.insert("path".to_string(), Value::from(file.path.as_str()));
        sess.execute_with_params(
            "MATCH (n:file) WHERE n.path = $path DETACH DELETE n",
            params,
        )
        .map_err(|e| ChizuError::Other(format!("grafeo: {e}")))?;

        let mut props: Vec<(&str, Value)> = vec![
            ("path", Value::from(file.path.as_str())),
            ("kind", Value::from(file.kind.as_str())),
            ("hash", Value::from(file.hash.as_str())),
            ("indexed", Value::from(file.indexed)),
        ];
        if let Some(ref v) = file.component_id {
            props.push(("component_id", Value::from(v.as_str())));
        }
        if let Some(ref v) = file.ignore_reason {
            props.push(("ignore_reason", Value::from(v.as_str())));
        }

        sess.create_node_with_props(&["file"], props);
        Ok(())
    }

    pub fn get_file(&self, path: &str) -> Result<FileRecord> {
        let sess = self.session();
        let mut params = HashMap::new();
        params.insert("path".to_string(), Value::from(path));
        let result = sess
            .execute_with_params(
                "MATCH (n:file) WHERE n.path = $path RETURN n.path, n.component_id, n.kind, n.hash, n.indexed, n.ignore_reason",
                params,
            )
            .map_err(|e| ChizuError::Other(format!("grafeo: {e}")))?;

        let rows: Vec<Vec<Value>> = result.rows;
        if rows.is_empty() {
            return Err(ChizuError::NotFound(format!("file: {path}")));
        }
        row_to_file(&rows[0])
    }

    pub fn list_files(&self, component_id: Option<&str>) -> Result<Vec<FileRecord>> {
        let sess = self.session();
        let result = match component_id {
            Some(cid) => {
                let mut params = HashMap::new();
                params.insert("cid".to_string(), Value::from(cid));
                sess.execute_with_params(
                    "MATCH (n:file) WHERE n.component_id = $cid RETURN n.path, n.component_id, n.kind, n.hash, n.indexed, n.ignore_reason ORDER BY n.path",
                    params,
                )
            }
            None => sess.execute(
                "MATCH (n:file) RETURN n.path, n.component_id, n.kind, n.hash, n.indexed, n.ignore_reason ORDER BY n.path",
            ),
        }
        .map_err(|e| ChizuError::Other(format!("grafeo: {e}")))?;

        let rows: Vec<Vec<Value>> = result.rows;
        rows.iter().map(|r| row_to_file(r)).collect()
    }

    pub fn delete_file(&self, path: &str) -> Result<bool> {
        let sess = self.session();
        let mut params = HashMap::new();
        params.insert("path".to_string(), Value::from(path));

        let result = sess
            .execute_with_params(
                "MATCH (n:file) WHERE n.path = $path RETURN n.path",
                params.clone(),
            )
            .map_err(|e| ChizuError::Other(format!("grafeo: {e}")))?;
        let rows: Vec<Vec<Value>> = result.rows;
        if rows.is_empty() {
            return Ok(false);
        }

        sess.execute_with_params(
            "MATCH (n:file) WHERE n.path = $path DETACH DELETE n",
            params,
        )
        .map_err(|e| ChizuError::Other(format!("grafeo: {e}")))?;
        Ok(true)
    }
}

fn row_to_file(row: &[Value]) -> Result<FileRecord> {
    Ok(FileRecord {
        path: val_to_string(&row[0]),
        component_id: val_to_opt_string(&row[1]),
        kind: val_to_string(&row[2]),
        hash: val_to_string(&row[3]),
        indexed: row[4].as_bool().unwrap_or(false),
        ignore_reason: val_to_opt_string(&row[5]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::grafeo::GrafeoStore;

    fn test_file(path: &str) -> FileRecord {
        FileRecord {
            path: path.to_string(),
            component_id: Some("comp::core".to_string()),
            kind: "rust".to_string(),
            hash: "sha256:abc123".to_string(),
            indexed: true,
            ignore_reason: None,
        }
    }

    #[test]
    fn insert_get_file() {
        let store = GrafeoStore::open_in_memory().unwrap();
        let f = test_file("src/lib.rs");
        store.insert_file(&f).unwrap();
        let got = store.get_file("src/lib.rs").unwrap();
        assert_eq!(f, got);
    }

    #[test]
    fn get_missing_file() {
        let store = GrafeoStore::open_in_memory().unwrap();
        let err = store.get_file("nope.rs").unwrap_err();
        assert!(matches!(err, ChizuError::NotFound(_)));
    }

    #[test]
    fn list_by_component() {
        let store = GrafeoStore::open_in_memory().unwrap();
        store.insert_file(&test_file("a.rs")).unwrap();
        store
            .insert_file(&FileRecord {
                component_id: Some("other".to_string()),
                ..test_file("b.rs")
            })
            .unwrap();

        let files = store.list_files(Some("comp::core")).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "a.rs");
    }

    #[test]
    fn delete_file() {
        let store = GrafeoStore::open_in_memory().unwrap();
        store.insert_file(&test_file("x.rs")).unwrap();
        assert!(store.delete_file("x.rs").unwrap());
        assert!(!store.delete_file("x.rs").unwrap());
    }

    #[test]
    fn insert_replaces_file() {
        let store = GrafeoStore::open_in_memory().unwrap();
        store.insert_file(&test_file("a.rs")).unwrap();
        let updated = FileRecord {
            hash: "sha256:updated".to_string(),
            ..test_file("a.rs")
        };
        store.insert_file(&updated).unwrap();
        let got = store.get_file("a.rs").unwrap();
        assert_eq!(got.hash, "sha256:updated");
    }
}
