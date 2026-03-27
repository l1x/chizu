use super::SqliteStore;
use crate::error::{ChizuError, Result};
use crate::model::{Entity, EntityKind};

impl SqliteStore {
    pub fn insert_entity(&self, entity: &Entity) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO entities
             (id, kind, name, component_id, path, language, line_start, line_end, visibility, exported)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                entity.id,
                entity.kind.as_str(),
                entity.name,
                entity.component_id,
                entity.path,
                entity.language,
                entity.line_start,
                entity.line_end,
                entity.visibility,
                entity.exported as i64,
            ],
        )?;
        Ok(())
    }

    pub fn get_entity(&self, id: &str) -> Result<Entity> {
        self.conn
            .query_row(
                "SELECT id, kind, name, component_id, path, language,
                        line_start, line_end, visibility, exported
                 FROM entities WHERE id = ?1",
                [id],
                |row| {
                    let kind_str: String = row.get(1)?;
                    Ok(Entity {
                        id: row.get(0)?,
                        kind: EntityKind::parse(&kind_str).unwrap_or(EntityKind::Symbol),
                        name: row.get(2)?,
                        component_id: row.get(3)?,
                        path: row.get(4)?,
                        language: row.get(5)?,
                        line_start: row.get(6)?,
                        line_end: row.get(7)?,
                        visibility: row.get(8)?,
                        exported: row.get::<_, i64>(9)? != 0,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    ChizuError::NotFound(format!("entity: {id}"))
                }
                other => ChizuError::Sqlite(other),
            })
    }

    pub fn list_entities(&self) -> Result<Vec<Entity>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, name, component_id, path, language,
                    line_start, line_end, visibility, exported
             FROM entities ORDER BY id",
        )?;
        let rows = stmt.query_map([], Self::row_to_entity)?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn list_entities_by_component(&self, component_id: &str) -> Result<Vec<Entity>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, name, component_id, path, language,
                    line_start, line_end, visibility, exported
             FROM entities WHERE component_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([component_id], Self::row_to_entity)?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn list_entities_by_path(&self, path: &str) -> Result<Vec<Entity>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, name, component_id, path, language,
                    line_start, line_end, visibility, exported
             FROM entities WHERE path = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([path], Self::row_to_entity)?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn delete_entity(&self, id: &str) -> Result<bool> {
        let count = self
            .conn
            .execute("DELETE FROM entities WHERE id = ?1", [id])?;
        Ok(count > 0)
    }

    fn row_to_entity(row: &rusqlite::Row<'_>) -> rusqlite::Result<Entity> {
        let kind_str: String = row.get(1)?;
        Ok(Entity {
            id: row.get(0)?,
            kind: EntityKind::parse(&kind_str).unwrap_or(EntityKind::Symbol),
            name: row.get(2)?,
            component_id: row.get(3)?,
            path: row.get(4)?,
            language: row.get(5)?,
            line_start: row.get(6)?,
            line_end: row.get(7)?,
            visibility: row.get(8)?,
            exported: row.get::<_, i64>(9)? != 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntityKind;

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
        let store = SqliteStore::open_in_memory().unwrap();
        let e = test_entity("component::test");
        store.insert_entity(&e).unwrap();
        let got = store.get_entity("component::test").unwrap();
        assert_eq!(e, got);
    }

    #[test]
    fn get_missing_entity() {
        let store = SqliteStore::open_in_memory().unwrap();
        let err = store.get_entity("nope").unwrap_err();
        assert!(matches!(err, ChizuError::NotFound(_)));
    }

    #[test]
    fn list_by_component() {
        let store = SqliteStore::open_in_memory().unwrap();
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
        let store = SqliteStore::open_in_memory().unwrap();
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
        store
            .insert_entity(&Entity {
                path: Some("src/main.rs".to_string()),
                ..test_entity("c")
            })
            .unwrap();

        let results = store.list_entities_by_path("src/lib.rs").unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "a");
        assert_eq!(results[1].id, "b");
    }

    #[test]
    fn delete_entity() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.insert_entity(&test_entity("x")).unwrap();
        assert!(store.delete_entity("x").unwrap());
        assert!(!store.delete_entity("x").unwrap());
    }
}
