use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

use super::{Result, Store};
use crate::model::{
    ComponentId, Edge, EdgeKind, EmbeddingMeta, Entity, EntityKind, FileRecord, Summary, TaskRoute,
};

/// SQLite-backed store implementation.
pub struct SqliteStore {
    conn: Connection,
}

/// Current schema version
const SCHEMA_VERSION: i32 = 4;

/// Schema v4 SQL
const SCHEMA_V4: &str = r#"
CREATE TABLE IF NOT EXISTS files (
  path TEXT PRIMARY KEY,
  component_id TEXT,
  kind TEXT NOT NULL,
  hash TEXT NOT NULL,
  indexed INTEGER NOT NULL DEFAULT 1,
  ignore_reason TEXT
);

CREATE TABLE IF NOT EXISTS entities (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  component_id TEXT,
  path TEXT,
  language TEXT,
  line_start INTEGER,
  line_end INTEGER,
  visibility TEXT,
  exported INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS edges (
  src_id TEXT NOT NULL,
  rel TEXT NOT NULL,
  dst_id TEXT NOT NULL,
  provenance_path TEXT,
  provenance_line INTEGER,
  PRIMARY KEY (src_id, rel, dst_id)
);

CREATE TABLE IF NOT EXISTS summaries (
  entity_id TEXT PRIMARY KEY,
  short_summary TEXT NOT NULL,
  detailed_summary TEXT,
  keywords_json TEXT,
  updated_at TEXT NOT NULL,
  source_hash TEXT
);

CREATE TABLE IF NOT EXISTS task_routes (
  task_name TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  priority INTEGER NOT NULL,
  PRIMARY KEY (task_name, entity_id)
);

CREATE TABLE IF NOT EXISTS embeddings (
  entity_id TEXT PRIMARY KEY,
  model TEXT NOT NULL,
  dimensions INTEGER NOT NULL,
  updated_at TEXT NOT NULL,
  usearch_key INTEGER
);

CREATE INDEX IF NOT EXISTS idx_entities_kind ON entities(kind);
CREATE INDEX IF NOT EXISTS idx_entities_component ON entities(component_id);
CREATE INDEX IF NOT EXISTS idx_edges_src ON edges(src_id);
CREATE INDEX IF NOT EXISTS idx_edges_dst ON edges(dst_id);
CREATE INDEX IF NOT EXISTS idx_edges_rel ON edges(rel);
CREATE INDEX IF NOT EXISTS idx_task_routes_task ON task_routes(task_name);
CREATE INDEX IF NOT EXISTS idx_task_routes_entity ON task_routes(entity_id);
"#;

impl SqliteStore {
    /// Open or create a SQLite store at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;

        // Enable WAL mode for better concurrency
        conn.execute_batch("PRAGMA journal_mode = WAL;")?;

        let store = Self { conn };
        store.migrate()?;

        Ok(store)
    }

    /// Run database migrations.
    fn migrate(&self) -> Result<()> {
        // Get or set user version
        let version: i32 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;

        if version < SCHEMA_VERSION {
            self.conn.execute_batch(SCHEMA_V4)?;
            self.conn
                .execute(&format!("PRAGMA user_version = {}", SCHEMA_VERSION), [])?;
        }

        Ok(())
    }

    /// Get the underlying connection (for advanced use).
    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

impl Store for SqliteStore {
    fn insert_entity(&self, entity: &Entity) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO entities 
             (id, kind, name, component_id, path, language, line_start, line_end, visibility, exported)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                entity.id,
                entity.kind.to_string(),
                entity.name,
                entity.component_id.as_ref().map(|c| c.to_string()),
                entity.path,
                entity.language,
                entity.line_start.map(|n| n as i64),
                entity.line_end.map(|n| n as i64),
                entity.visibility,
                entity.exported as i32,
            ],
        )?;
        Ok(())
    }

    fn get_entity(&self, id: &str) -> Result<Option<Entity>> {
        let entity = self.conn.query_row(
            "SELECT id, kind, name, component_id, path, language, line_start, line_end, visibility, exported
             FROM entities WHERE id = ?1",
            [id],
            |row| {
                let kind_str: String = row.get(1)?;
                let component_id: Option<String> = row.get(3)?;
                Ok(Entity {
                    id: row.get(0)?,
                    kind: kind_str.parse().map_err(|e: String| rusqlite::Error::ToSqlConversionFailure(e.into()))?,
                    name: row.get(2)?,
                    component_id: component_id.and_then(|c| ComponentId::parse(&c)),
                    path: row.get(4)?,
                    language: row.get(5)?,
                    line_start: row.get::<_, Option<i64>>(6)?.map(|n| n as u32),
                    line_end: row.get::<_, Option<i64>>(7)?.map(|n| n as u32),
                    visibility: row.get(8)?,
                    exported: row.get::<_, i32>(9)? != 0,
                })
            }
        ).optional()?;
        Ok(entity)
    }

    fn get_entities_by_kind(&self, kind: EntityKind) -> Result<Vec<Entity>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, name, component_id, path, language, line_start, line_end, visibility, exported
             FROM entities WHERE kind = ?1"
        )?;

        let entities = stmt
            .query_map([kind.to_string()], |row| {
                let kind_str: String = row.get(1)?;
                let component_id: Option<String> = row.get(3)?;
                Ok(Entity {
                    id: row.get(0)?,
                    kind: kind_str
                        .parse()
                        .map_err(|e: String| rusqlite::Error::ToSqlConversionFailure(e.into()))?,
                    name: row.get(2)?,
                    component_id: component_id.and_then(|c| ComponentId::parse(&c)),
                    path: row.get(4)?,
                    language: row.get(5)?,
                    line_start: row.get::<_, Option<i64>>(6)?.map(|n| n as u32),
                    line_end: row.get::<_, Option<i64>>(7)?.map(|n| n as u32),
                    visibility: row.get(8)?,
                    exported: row.get::<_, i32>(9)? != 0,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(entities)
    }

    fn get_entities_by_component(&self, component_id: &ComponentId) -> Result<Vec<Entity>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, name, component_id, path, language, line_start, line_end, visibility, exported
             FROM entities WHERE component_id = ?1"
        )?;

        let entities = stmt
            .query_map([component_id.to_string()], |row| {
                let kind_str: String = row.get(1)?;
                let cid: Option<String> = row.get(3)?;
                Ok(Entity {
                    id: row.get(0)?,
                    kind: kind_str
                        .parse()
                        .map_err(|e: String| rusqlite::Error::ToSqlConversionFailure(e.into()))?,
                    name: row.get(2)?,
                    component_id: cid.and_then(|c| ComponentId::parse(&c)),
                    path: row.get(4)?,
                    language: row.get(5)?,
                    line_start: row.get::<_, Option<i64>>(6)?.map(|n| n as u32),
                    line_end: row.get::<_, Option<i64>>(7)?.map(|n| n as u32),
                    visibility: row.get(8)?,
                    exported: row.get::<_, i32>(9)? != 0,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(entities)
    }

    fn delete_entity(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM entities WHERE id = ?1", [id])?;
        Ok(())
    }

    fn delete_entities_by_component(&self, component_id: &ComponentId) -> Result<usize> {
        let count = self.conn.execute(
            "DELETE FROM entities WHERE component_id = ?1",
            [component_id.to_string()],
        )?;
        Ok(count)
    }

    fn insert_edge(&self, edge: &Edge) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO edges 
             (src_id, rel, dst_id, provenance_path, provenance_line)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                edge.src_id,
                edge.rel.to_string(),
                edge.dst_id,
                edge.provenance_path,
                edge.provenance_line.map(|n| n as i64),
            ],
        )?;
        Ok(())
    }

    fn get_edges_from(&self, src_id: &str) -> Result<Vec<Edge>> {
        let mut stmt = self.conn.prepare(
            "SELECT src_id, rel, dst_id, provenance_path, provenance_line FROM edges WHERE src_id = ?1"
        )?;

        let edges = stmt
            .query_map([src_id], |row| {
                let rel_str: String = row.get(1)?;
                Ok(Edge {
                    src_id: row.get(0)?,
                    rel: rel_str
                        .parse()
                        .map_err(|e: String| rusqlite::Error::ToSqlConversionFailure(e.into()))?,
                    dst_id: row.get(2)?,
                    provenance_path: row.get(3)?,
                    provenance_line: row.get::<_, Option<i64>>(4)?.map(|n| n as u32),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(edges)
    }

    fn get_edges_to(&self, dst_id: &str) -> Result<Vec<Edge>> {
        let mut stmt = self.conn.prepare(
            "SELECT src_id, rel, dst_id, provenance_path, provenance_line FROM edges WHERE dst_id = ?1"
        )?;

        let edges = stmt
            .query_map([dst_id], |row| {
                let rel_str: String = row.get(1)?;
                Ok(Edge {
                    src_id: row.get(0)?,
                    rel: rel_str
                        .parse()
                        .map_err(|e: String| rusqlite::Error::ToSqlConversionFailure(e.into()))?,
                    dst_id: row.get(2)?,
                    provenance_path: row.get(3)?,
                    provenance_line: row.get::<_, Option<i64>>(4)?.map(|n| n as u32),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(edges)
    }

    fn get_edges_by_rel(&self, rel: EdgeKind) -> Result<Vec<Edge>> {
        let mut stmt = self.conn.prepare(
            "SELECT src_id, rel, dst_id, provenance_path, provenance_line FROM edges WHERE rel = ?1"
        )?;

        let edges = stmt
            .query_map([rel.to_string()], |row| {
                let rel_str: String = row.get(1)?;
                Ok(Edge {
                    src_id: row.get(0)?,
                    rel: rel_str
                        .parse()
                        .map_err(|e: String| rusqlite::Error::ToSqlConversionFailure(e.into()))?,
                    dst_id: row.get(2)?,
                    provenance_path: row.get(3)?,
                    provenance_line: row.get::<_, Option<i64>>(4)?.map(|n| n as u32),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(edges)
    }

    fn delete_edge(&self, src_id: &str, rel: EdgeKind, dst_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM edges WHERE src_id = ?1 AND rel = ?2 AND dst_id = ?3",
            params![src_id, rel.to_string(), dst_id],
        )?;
        Ok(())
    }

    fn delete_edges_by_component(&self, component_id: &ComponentId) -> Result<usize> {
        // Delete edges where either src or dst is in the component
        let count = self.conn.execute(
            "DELETE FROM edges WHERE 
             src_id IN (SELECT id FROM entities WHERE component_id = ?1) OR
             dst_id IN (SELECT id FROM entities WHERE component_id = ?1)",
            [component_id.to_string()],
        )?;
        Ok(count)
    }

    fn insert_file(&self, file: &FileRecord) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO files 
             (path, component_id, kind, hash, indexed, ignore_reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                file.path,
                file.component_id.as_ref().map(|c| c.to_string()),
                file.kind,
                file.hash,
                file.indexed as i32,
                file.ignore_reason,
            ],
        )?;
        Ok(())
    }

    fn get_file(&self, path: &str) -> Result<Option<FileRecord>> {
        let file = self.conn.query_row(
            "SELECT path, component_id, kind, hash, indexed, ignore_reason FROM files WHERE path = ?1",
            [path],
            |row| {
                let cid: Option<String> = row.get(1)?;
                Ok(FileRecord {
                    path: row.get(0)?,
                    component_id: cid.and_then(|c| ComponentId::parse(&c)),
                    kind: row.get(2)?,
                    hash: row.get(3)?,
                    indexed: row.get::<_, i32>(4)? != 0,
                    ignore_reason: row.get(5)?,
                })
            }
        ).optional()?;
        Ok(file)
    }

    fn get_all_files(&self) -> Result<Vec<FileRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, component_id, kind, hash, indexed, ignore_reason FROM files")?;

        let files = stmt
            .query_map([], |row| {
                let cid: Option<String> = row.get(1)?;
                Ok(FileRecord {
                    path: row.get(0)?,
                    component_id: cid.and_then(|c| ComponentId::parse(&c)),
                    kind: row.get(2)?,
                    hash: row.get(3)?,
                    indexed: row.get::<_, i32>(4)? != 0,
                    ignore_reason: row.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(files)
    }

    fn delete_file(&self, path: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM files WHERE path = ?1", [path])?;
        Ok(())
    }

    fn insert_summary(&self, summary: &Summary) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO summaries 
             (entity_id, short_summary, detailed_summary, keywords_json, updated_at, source_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                summary.entity_id,
                summary.short_summary,
                summary.detailed_summary,
                summary.keywords_json,
                summary.updated_at,
                summary.source_hash,
            ],
        )?;
        Ok(())
    }

    fn get_summary(&self, entity_id: &str) -> Result<Option<Summary>> {
        let summary = self.conn.query_row(
            "SELECT entity_id, short_summary, detailed_summary, keywords_json, updated_at, source_hash 
             FROM summaries WHERE entity_id = ?1",
            [entity_id],
            |row| {
                Ok(Summary {
                    entity_id: row.get(0)?,
                    short_summary: row.get(1)?,
                    detailed_summary: row.get(2)?,
                    keywords_json: row.get(3)?,
                    updated_at: row.get(4)?,
                    source_hash: row.get(5)?,
                })
            }
        ).optional()?;
        Ok(summary)
    }

    fn delete_summary(&self, entity_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM summaries WHERE entity_id = ?1", [entity_id])?;
        Ok(())
    }

    fn insert_task_route(&self, route: &TaskRoute) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO task_routes (task_name, entity_id, priority)
             VALUES (?1, ?2, ?3)",
            params![route.task_name, route.entity_id, route.priority],
        )?;
        Ok(())
    }

    fn get_task_routes(&self, task_name: &str) -> Result<Vec<TaskRoute>> {
        let mut stmt = self.conn.prepare(
            "SELECT task_name, entity_id, priority FROM task_routes WHERE task_name = ?1",
        )?;

        let routes = stmt
            .query_map([task_name], |row| {
                Ok(TaskRoute {
                    task_name: row.get(0)?,
                    entity_id: row.get(1)?,
                    priority: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(routes)
    }

    fn get_entity_task_routes(&self, entity_id: &str) -> Result<Vec<TaskRoute>> {
        let mut stmt = self.conn.prepare(
            "SELECT task_name, entity_id, priority FROM task_routes WHERE entity_id = ?1",
        )?;

        let routes = stmt
            .query_map([entity_id], |row| {
                Ok(TaskRoute {
                    task_name: row.get(0)?,
                    entity_id: row.get(1)?,
                    priority: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(routes)
    }

    fn delete_entity_task_routes(&self, entity_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM task_routes WHERE entity_id = ?1", [entity_id])?;
        Ok(())
    }

    fn insert_embedding_meta(&self, meta: &EmbeddingMeta) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO embeddings 
             (entity_id, model, dimensions, updated_at, usearch_key)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                meta.entity_id,
                meta.model,
                meta.dimensions as i64,
                meta.updated_at,
                meta.usearch_key,
            ],
        )?;
        Ok(())
    }

    fn get_embedding_meta(&self, entity_id: &str) -> Result<Option<EmbeddingMeta>> {
        let meta = self
            .conn
            .query_row(
                "SELECT entity_id, model, dimensions, updated_at, usearch_key 
             FROM embeddings WHERE entity_id = ?1",
                [entity_id],
                |row| {
                    Ok(EmbeddingMeta {
                        entity_id: row.get(0)?,
                        model: row.get(1)?,
                        dimensions: row.get::<_, i64>(2)? as u32,
                        updated_at: row.get(3)?,
                        usearch_key: row.get(4)?,
                    })
                },
            )
            .optional()?;
        Ok(meta)
    }

    fn delete_embedding_meta(&self, entity_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM embeddings WHERE entity_id = ?1", [entity_id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_store() -> (SqliteStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::open(&db_path).unwrap();
        (store, temp_dir)
    }

    #[test]
    fn test_insert_and_get_entity() {
        let (store, _temp) = create_test_store();

        let entity = Entity::new("test::1", EntityKind::Symbol, "test_func")
            .with_component(ComponentId::new("cargo", "."))
            .with_path("src/lib.rs")
            .with_language("rust")
            .with_exported(true);

        store.insert_entity(&entity).unwrap();

        let retrieved = store.get_entity("test::1").unwrap().unwrap();
        assert_eq!(retrieved.id, "test::1");
        assert_eq!(retrieved.name, "test_func");
        assert!(retrieved.exported);
    }

    #[test]
    fn test_get_entities_by_kind() {
        let (store, _temp) = create_test_store();

        store
            .insert_entity(&Entity::new("s1", EntityKind::Symbol, "s1"))
            .unwrap();
        store
            .insert_entity(&Entity::new("s2", EntityKind::Symbol, "s2"))
            .unwrap();
        store
            .insert_entity(&Entity::new("d1", EntityKind::Doc, "d1"))
            .unwrap();

        let symbols = store.get_entities_by_kind(EntityKind::Symbol).unwrap();
        assert_eq!(symbols.len(), 2);

        let docs = store.get_entities_by_kind(EntityKind::Doc).unwrap();
        assert_eq!(docs.len(), 1);
    }

    #[test]
    fn test_delete_entities_by_component() {
        let (store, _temp) = create_test_store();

        let cid = ComponentId::new("cargo", "crate1");
        store
            .insert_entity(&Entity::new("e1", EntityKind::Symbol, "e1").with_component(cid.clone()))
            .unwrap();
        store
            .insert_entity(&Entity::new("e2", EntityKind::Symbol, "e2").with_component(cid.clone()))
            .unwrap();
        store
            .insert_entity(&Entity::new("e3", EntityKind::Symbol, "e3"))
            .unwrap();

        let count = store.delete_entities_by_component(&cid).unwrap();
        assert_eq!(count, 2);

        assert!(store.get_entity("e1").unwrap().is_none());
        assert!(store.get_entity("e3").unwrap().is_some());
    }

    #[test]
    fn test_insert_and_get_edge() {
        let (store, _temp) = create_test_store();

        let edge = Edge::new("src", EdgeKind::Defines, "dst").with_provenance("file.rs", 10);

        store.insert_edge(&edge).unwrap();

        let from_edges = store.get_edges_from("src").unwrap();
        assert_eq!(from_edges.len(), 1);
        assert_eq!(from_edges[0].dst_id, "dst");

        let to_edges = store.get_edges_to("dst").unwrap();
        assert_eq!(to_edges.len(), 1);
    }

    #[test]
    fn test_file_record() {
        let (store, _temp) = create_test_store();

        let file = FileRecord::new("src/main.rs", "rust", "abc123")
            .with_component(ComponentId::new("cargo", "."));

        store.insert_file(&file).unwrap();

        let retrieved = store.get_file("src/main.rs").unwrap().unwrap();
        assert_eq!(retrieved.path, "src/main.rs");
        assert_eq!(retrieved.hash, "abc123");
    }

    #[test]
    fn test_schema_version() {
        let (store, _temp) = create_test_store();

        let version: i32 = store
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();

        assert_eq!(version, SCHEMA_VERSION);
    }
}
