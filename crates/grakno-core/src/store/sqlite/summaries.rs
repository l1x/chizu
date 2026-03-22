use super::SqliteStore;
use crate::error::{GraknoError, Result};
use crate::model::Summary;

impl SqliteStore {
    pub fn upsert_summary(&self, summary: &Summary) -> Result<()> {
        let keywords_json = serde_json::to_string(&summary.keywords)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO summaries
             (entity_id, short_summary, detailed_summary, keywords_json, updated_at, source_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                summary.entity_id,
                summary.short_summary,
                summary.detailed_summary,
                keywords_json,
                summary.updated_at,
                summary.source_hash,
            ],
        )?;
        Ok(())
    }

    pub fn get_summary(&self, entity_id: &str) -> Result<Summary> {
        self.conn
            .query_row(
                "SELECT entity_id, short_summary, detailed_summary, keywords_json, updated_at, source_hash
                 FROM summaries WHERE entity_id = ?1",
                [entity_id],
                |row| {
                    let kw_json: Option<String> = row.get(3)?;
                    let keywords: Vec<String> = kw_json
                        .as_deref()
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or_default();
                    Ok(Summary {
                        entity_id: row.get(0)?,
                        short_summary: row.get(1)?,
                        detailed_summary: row.get(2)?,
                        keywords,
                        updated_at: row.get(4)?,
                        source_hash: row.get(5)?,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    GraknoError::NotFound(format!("summary: {entity_id}"))
                }
                other => GraknoError::Sqlite(other),
            })
    }

    pub fn delete_summary(&self, entity_id: &str) -> Result<bool> {
        let count = self
            .conn
            .execute("DELETE FROM summaries WHERE entity_id = ?1", [entity_id])?;
        Ok(count > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_summary(entity_id: &str) -> Summary {
        Summary {
            entity_id: entity_id.to_string(),
            short_summary: "A test component".to_string(),
            detailed_summary: Some("Detailed description here".to_string()),
            keywords: vec!["test".to_string(), "component".to_string()],
            updated_at: "2026-03-21T00:00:00Z".to_string(),
            source_hash: None,
        }
    }

    #[test]
    fn upsert_get_summary() {
        let store = SqliteStore::open_in_memory().unwrap();
        let s = test_summary("comp::a");
        store.upsert_summary(&s).unwrap();
        let got = store.get_summary("comp::a").unwrap();
        assert_eq!(s, got);
    }

    #[test]
    fn upsert_replaces_summary() {
        let store = SqliteStore::open_in_memory().unwrap();
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
        let store = SqliteStore::open_in_memory().unwrap();
        let s = test_summary("comp::b");
        store.upsert_summary(&s).unwrap();
        let got = store.get_summary("comp::b").unwrap();
        assert_eq!(got.keywords, vec!["test", "component"]);
    }

    #[test]
    fn delete_summary() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.upsert_summary(&test_summary("comp::x")).unwrap();
        assert!(store.delete_summary("comp::x").unwrap());
        assert!(!store.delete_summary("comp::x").unwrap());
    }

    #[test]
    fn get_missing_summary() {
        let store = SqliteStore::open_in_memory().unwrap();
        let err = store.get_summary("nope").unwrap_err();
        assert!(matches!(err, GraknoError::NotFound(_)));
    }
}
