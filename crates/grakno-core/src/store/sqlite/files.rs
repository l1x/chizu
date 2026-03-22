use super::SqliteStore;
use crate::error::{GraknoError, Result};
use crate::model::FileRecord;

impl SqliteStore {
    pub fn insert_file(&self, file: &FileRecord) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO files (path, component_id, kind, hash, indexed, ignore_reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                file.path,
                file.component_id,
                file.kind,
                file.hash,
                file.indexed as i64,
                file.ignore_reason,
            ],
        )?;
        Ok(())
    }

    pub fn get_file(&self, path: &str) -> Result<FileRecord> {
        self.conn
            .query_row(
                "SELECT path, component_id, kind, hash, indexed, ignore_reason
                 FROM files WHERE path = ?1",
                [path],
                |row| {
                    Ok(FileRecord {
                        path: row.get(0)?,
                        component_id: row.get(1)?,
                        kind: row.get(2)?,
                        hash: row.get(3)?,
                        indexed: row.get::<_, i64>(4)? != 0,
                        ignore_reason: row.get(5)?,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    GraknoError::NotFound(format!("file: {path}"))
                }
                other => GraknoError::Sqlite(other),
            })
    }

    pub fn list_files(&self, component_id: Option<&str>) -> Result<Vec<FileRecord>> {
        let mut results = Vec::new();
        match component_id {
            Some(cid) => {
                let mut stmt = self.conn.prepare(
                    "SELECT path, component_id, kind, hash, indexed, ignore_reason
                     FROM files WHERE component_id = ?1 ORDER BY path",
                )?;
                let rows = stmt.query_map([cid], |row| {
                    Ok(FileRecord {
                        path: row.get(0)?,
                        component_id: row.get(1)?,
                        kind: row.get(2)?,
                        hash: row.get(3)?,
                        indexed: row.get::<_, i64>(4)? != 0,
                        ignore_reason: row.get(5)?,
                    })
                })?;
                for row in rows {
                    results.push(row?);
                }
            }
            None => {
                let mut stmt = self.conn.prepare(
                    "SELECT path, component_id, kind, hash, indexed, ignore_reason
                     FROM files ORDER BY path",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok(FileRecord {
                        path: row.get(0)?,
                        component_id: row.get(1)?,
                        kind: row.get(2)?,
                        hash: row.get(3)?,
                        indexed: row.get::<_, i64>(4)? != 0,
                        ignore_reason: row.get(5)?,
                    })
                })?;
                for row in rows {
                    results.push(row?);
                }
            }
        }
        Ok(results)
    }

    pub fn delete_file(&self, path: &str) -> Result<bool> {
        let count = self
            .conn
            .execute("DELETE FROM files WHERE path = ?1", [path])?;
        Ok(count > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_file(path: &str) -> FileRecord {
        FileRecord {
            path: path.to_string(),
            component_id: Some("comp-a".to_string()),
            kind: "rust".to_string(),
            hash: "abc123".to_string(),
            indexed: true,
            ignore_reason: None,
        }
    }

    #[test]
    fn insert_get_file() {
        let store = SqliteStore::open_in_memory().unwrap();
        let f = test_file("src/main.rs");
        store.insert_file(&f).unwrap();
        let got = store.get_file("src/main.rs").unwrap();
        assert_eq!(f, got);
    }

    #[test]
    fn get_missing_file_returns_not_found() {
        let store = SqliteStore::open_in_memory().unwrap();
        let err = store.get_file("nope").unwrap_err();
        assert!(matches!(err, GraknoError::NotFound(_)));
    }

    #[test]
    fn list_files_by_component() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.insert_file(&test_file("a.rs")).unwrap();
        store
            .insert_file(&FileRecord {
                component_id: Some("comp-b".to_string()),
                ..test_file("b.rs")
            })
            .unwrap();

        let all = store.list_files(None).unwrap();
        assert_eq!(all.len(), 2);

        let comp_a = store.list_files(Some("comp-a")).unwrap();
        assert_eq!(comp_a.len(), 1);
        assert_eq!(comp_a[0].path, "a.rs");
    }

    #[test]
    fn delete_file() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.insert_file(&test_file("x.rs")).unwrap();
        assert!(store.delete_file("x.rs").unwrap());
        assert!(!store.delete_file("x.rs").unwrap());
    }

    #[test]
    fn insert_replaces_on_conflict() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.insert_file(&test_file("lib.rs")).unwrap();
        let updated = FileRecord {
            hash: "new_hash".to_string(),
            ..test_file("lib.rs")
        };
        store.insert_file(&updated).unwrap();
        let got = store.get_file("lib.rs").unwrap();
        assert_eq!(got.hash, "new_hash");
    }
}
