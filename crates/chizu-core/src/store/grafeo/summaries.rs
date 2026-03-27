use std::collections::HashMap;

use grafeo::Value;

use super::entities::{val_to_opt_string, val_to_string};
use super::GrafeoStore;
use crate::error::{ChizuError, Result};
use crate::model::Summary;

impl GrafeoStore {
    pub fn upsert_summary(&self, summary: &Summary) -> Result<()> {
        let sess = self.session();
        let keywords_json = serde_json::to_string(&summary.keywords)?;

        // Delete existing (upsert)
        let mut params = HashMap::new();
        params.insert(
            "entity_id".to_string(),
            Value::from(summary.entity_id.as_str()),
        );
        sess.execute_with_params(
            "MATCH (n:summary) WHERE n.entity_id = $entity_id DETACH DELETE n",
            params,
        )
        .map_err(|e| ChizuError::Other(format!("grafeo: {e}")))?;

        let mut props: Vec<(&str, Value)> = vec![
            ("entity_id", Value::from(summary.entity_id.as_str())),
            ("short_summary", Value::from(summary.short_summary.as_str())),
            ("keywords_json", Value::from(keywords_json.as_str())),
            ("updated_at", Value::from(summary.updated_at.as_str())),
        ];
        if let Some(ref v) = summary.detailed_summary {
            props.push(("detailed_summary", Value::from(v.as_str())));
        }
        if let Some(ref v) = summary.source_hash {
            props.push(("source_hash", Value::from(v.as_str())));
        }

        sess.create_node_with_props(&["summary"], props);
        Ok(())
    }

    pub fn get_summary(&self, entity_id: &str) -> Result<Summary> {
        let sess = self.session();
        let mut params = HashMap::new();
        params.insert("entity_id".to_string(), Value::from(entity_id));
        let result = sess
            .execute_with_params(
                "MATCH (n:summary) WHERE n.entity_id = $entity_id RETURN n.entity_id, n.short_summary, n.detailed_summary, n.keywords_json, n.updated_at, n.source_hash",
                params,
            )
            .map_err(|e| ChizuError::Other(format!("grafeo: {e}")))?;

        let rows: Vec<Vec<Value>> = result.rows;
        if rows.is_empty() {
            return Err(ChizuError::NotFound(format!("summary: {entity_id}")));
        }
        row_to_summary(&rows[0])
    }

    pub fn delete_summary(&self, entity_id: &str) -> Result<bool> {
        let sess = self.session();
        let mut params = HashMap::new();
        params.insert("entity_id".to_string(), Value::from(entity_id));

        let result = sess
            .execute_with_params(
                "MATCH (n:summary) WHERE n.entity_id = $entity_id RETURN n.entity_id",
                params.clone(),
            )
            .map_err(|e| ChizuError::Other(format!("grafeo: {e}")))?;
        let rows: Vec<Vec<Value>> = result.rows;
        if rows.is_empty() {
            return Ok(false);
        }

        sess.execute_with_params(
            "MATCH (n:summary) WHERE n.entity_id = $entity_id DETACH DELETE n",
            params,
        )
        .map_err(|e| ChizuError::Other(format!("grafeo: {e}")))?;
        Ok(true)
    }
}

fn row_to_summary(row: &[Value]) -> Result<Summary> {
    let kw_json = val_to_opt_string(&row[3]);
    let keywords: Vec<String> = kw_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    Ok(Summary {
        entity_id: val_to_string(&row[0]),
        short_summary: val_to_string(&row[1]),
        detailed_summary: val_to_opt_string(&row[2]),
        keywords,
        updated_at: val_to_string(&row[4]),
        source_hash: val_to_opt_string(&row[5]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::grafeo::GrafeoStore;

    fn test_summary(entity_id: &str) -> Summary {
        Summary {
            entity_id: entity_id.to_string(),
            short_summary: "A test summary".to_string(),
            detailed_summary: Some("Detailed description here".to_string()),
            keywords: vec!["test".to_string(), "example".to_string()],
            updated_at: "2026-03-21T00:00:00Z".to_string(),
            source_hash: Some("hash123".to_string()),
        }
    }

    #[test]
    fn upsert_get_summary() {
        let store = GrafeoStore::open_in_memory().unwrap();
        let s = test_summary("comp::a");
        store.upsert_summary(&s).unwrap();
        let got = store.get_summary("comp::a").unwrap();
        assert_eq!(s, got);
    }

    #[test]
    fn upsert_replace_summary() {
        let store = GrafeoStore::open_in_memory().unwrap();
        store.upsert_summary(&test_summary("comp::a")).unwrap();
        let updated = Summary {
            short_summary: "Updated summary".to_string(),
            ..test_summary("comp::a")
        };
        store.upsert_summary(&updated).unwrap();
        let got = store.get_summary("comp::a").unwrap();
        assert_eq!(got.short_summary, "Updated summary");
    }

    #[test]
    fn keywords_round_trip() {
        let store = GrafeoStore::open_in_memory().unwrap();
        let s = Summary {
            keywords: vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()],
            ..test_summary("comp::kw")
        };
        store.upsert_summary(&s).unwrap();
        let got = store.get_summary("comp::kw").unwrap();
        assert_eq!(got.keywords, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn delete_summary() {
        let store = GrafeoStore::open_in_memory().unwrap();
        store.upsert_summary(&test_summary("comp::x")).unwrap();
        assert!(store.delete_summary("comp::x").unwrap());
        assert!(!store.delete_summary("comp::x").unwrap());
    }

    #[test]
    fn get_missing_summary() {
        let store = GrafeoStore::open_in_memory().unwrap();
        let err = store.get_summary("nope").unwrap_err();
        assert!(matches!(err, ChizuError::NotFound(_)));
    }
}
