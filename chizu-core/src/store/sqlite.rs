use rusqlite::{Connection, OptionalExtension, Row, params};
use std::path::Path;

use super::Result;
#[cfg(test)]
use super::StoreError;
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
CREATE INDEX IF NOT EXISTS idx_embeddings_usearch_key ON embeddings(usearch_key);
CREATE INDEX IF NOT EXISTS idx_entities_path ON entities(path);
CREATE INDEX IF NOT EXISTS idx_edges_provenance ON edges(provenance_path);
"#;

// ── Column lists (single source of truth for SELECT projections) ────────

const ENTITY_COLUMNS: &str =
    "id, kind, name, component_id, path, language, line_start, line_end, visibility, exported";

const EDGE_COLUMNS: &str = "src_id, rel, dst_id, provenance_path, provenance_line";

const SUMMARY_COLUMNS: &str =
    "entity_id, short_summary, detailed_summary, keywords_json, updated_at, source_hash";

const EMBEDDING_META_COLUMNS: &str = "entity_id, model, dimensions, updated_at, usearch_key";

// ── Row mapping helpers ─────────────────────────────────────────────────

fn parse_text_column<T: std::str::FromStr<Err = String>>(s: String) -> rusqlite::Result<T> {
    s.parse()
        .map_err(|e: String| rusqlite::Error::ToSqlConversionFailure(e.into()))
}

fn entity_from_row(row: &Row<'_>) -> rusqlite::Result<Entity> {
    let kind_str: String = row.get(1)?;
    let component_id: Option<String> = row.get(3)?;
    let visibility_str: Option<String> = row.get(8)?;
    Ok(Entity {
        id: row.get(0)?,
        kind: parse_text_column(kind_str)?,
        name: row.get(2)?,
        component_id: component_id.and_then(|c| ComponentId::parse(&c)),
        path: row.get(4)?,
        language: row.get(5)?,
        line_start: row.get::<_, Option<i64>>(6)?.map(|n| n as u32),
        line_end: row.get::<_, Option<i64>>(7)?.map(|n| n as u32),
        visibility: visibility_str.map(parse_text_column).transpose()?,
        exported: row.get::<_, i32>(9)? != 0,
    })
}

fn edge_from_row(row: &Row<'_>) -> rusqlite::Result<Edge> {
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
}

fn file_from_row(row: &Row<'_>) -> rusqlite::Result<FileRecord> {
    let cid: Option<String> = row.get(1)?;
    let kind_str: String = row.get(2)?;
    Ok(FileRecord {
        path: row.get(0)?,
        component_id: cid.and_then(|c| ComponentId::parse(&c)),
        kind: parse_text_column(kind_str)?,
        hash: row.get(3)?,
        indexed: row.get::<_, i32>(4)? != 0,
        ignore_reason: row.get(5)?,
    })
}

fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<Summary> {
    let keywords_json: Option<String> = row.get(3)?;
    Ok(Summary {
        entity_id: row.get(0)?,
        short_summary: row.get(1)?,
        detailed_summary: row.get(2)?,
        keywords: keywords_json.and_then(|s| serde_json::from_str(&s).ok()),
        updated_at: row.get(4)?,
        source_hash: row.get(5)?,
    })
}

fn task_route_from_row(row: &Row<'_>) -> rusqlite::Result<TaskRoute> {
    Ok(TaskRoute {
        task_name: row.get(0)?,
        entity_id: row.get(1)?,
        priority: row.get(2)?,
    })
}

fn embedding_meta_from_row(row: &Row<'_>) -> rusqlite::Result<EmbeddingMeta> {
    Ok(EmbeddingMeta {
        entity_id: row.get(0)?,
        model: row.get(1)?,
        dimensions: row.get::<_, i64>(2)? as u32,
        updated_at: row.get(3)?,
        usearch_key: row.get(4)?,
    })
}

// ── SqliteStore ─────────────────────────────────────────────────────────

impl SqliteStore {
    /// Open or create a SQLite store at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;

        // Enable WAL mode for better concurrency.
        // NORMAL sync is safe: the index is regenerable from source.
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;

        let store = Self { conn };
        store.migrate()?;

        Ok(store)
    }

    /// Run database migrations.
    fn migrate(&self) -> Result<()> {
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

    // ── Transaction support ─────────────────────────────────────────────

    /// Begin a transaction. Use `commit_transaction` or `rollback_transaction`.
    pub(crate) fn begin_transaction(&self) -> Result<()> {
        self.conn.execute_batch("BEGIN")?;
        Ok(())
    }

    /// Commit the current transaction.
    pub(crate) fn commit_transaction(&self) -> Result<()> {
        self.conn.execute_batch("COMMIT")?;
        Ok(())
    }

    /// Roll back the current transaction.
    pub(crate) fn rollback_transaction(&self) -> Result<()> {
        self.conn.execute_batch("ROLLBACK")?;
        Ok(())
    }

    /// Execute a closure inside a SQLite transaction.
    pub fn in_transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Self) -> Result<T>,
    {
        self.begin_transaction()?;
        match f(self) {
            Ok(val) => {
                self.commit_transaction()?;
                Ok(val)
            }
            Err(e) => {
                let _ = self.rollback_transaction();
                Err(e)
            }
        }
    }

    // ── Entity operations ───────────────────────────────────────────────

    pub fn insert_entity(&self, entity: &Entity) -> Result<()> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT OR REPLACE INTO entities
             (id, kind, name, component_id, path, language, line_start, line_end, visibility, exported)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        stmt.execute(params![
            entity.id,
            entity.kind.to_string(),
            entity.name,
            entity.component_id.as_ref().map(|c| c.as_str()),
            entity.path,
            entity.language,
            entity.line_start.map(|n| n as i64),
            entity.line_end.map(|n| n as i64),
            entity.visibility.as_ref().map(|v| v.to_string()),
            entity.exported as i32,
        ])?;
        Ok(())
    }

    pub fn get_entity(&self, id: &str) -> Result<Option<Entity>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {ENTITY_COLUMNS} FROM entities WHERE id = ?1"
        ))?;
        let entity = stmt.query_row([id], entity_from_row).optional()?;
        Ok(entity)
    }

    pub fn get_entities_by_kind(&self, kind: EntityKind) -> Result<Vec<Entity>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {ENTITY_COLUMNS} FROM entities WHERE kind = ?1"
        ))?;
        let entities = stmt
            .query_map([kind.to_string()], entity_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(entities)
    }

    pub fn get_entities_by_component(&self, component_id: &ComponentId) -> Result<Vec<Entity>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {ENTITY_COLUMNS} FROM entities WHERE component_id = ?1"
        ))?;
        let entities = stmt
            .query_map([component_id.as_str()], entity_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(entities)
    }

    pub fn delete_entity(&self, id: &str) -> Result<()> {
        self.conn
            .prepare_cached("DELETE FROM entities WHERE id = ?1")?
            .execute([id])?;
        Ok(())
    }

    pub fn delete_entities_by_component(&self, component_id: &ComponentId) -> Result<usize> {
        let count = self.conn.execute(
            "DELETE FROM entities WHERE component_id = ?1",
            [component_id.as_str()],
        )?;
        Ok(count)
    }

    pub fn get_entities_by_path(&self, path: &str) -> Result<Vec<Entity>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {ENTITY_COLUMNS} FROM entities WHERE path = ?1"
        ))?;
        let entities = stmt
            .query_map([path], entity_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(entities)
    }

    pub fn get_all_entities(&self) -> Result<Vec<Entity>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {ENTITY_COLUMNS} FROM entities"))?;
        let entities = stmt
            .query_map([], entity_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(entities)
    }

    pub fn search_entities_by_name_or_path(
        &self,
        like_patterns: &[String],
        kinds: &[EntityKind],
    ) -> Result<Vec<Entity>> {
        if like_patterns.is_empty() || kinds.is_empty() {
            return Ok(Vec::new());
        }

        // Build: WHERE kind IN (?,?,...) AND (name LIKE ? OR path LIKE ? OR name LIKE ? OR ...)
        let kind_placeholders: String = kinds.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let text_conditions: String = like_patterns
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let p = kinds.len() + 1 + i * 2;
                format!(
                    "LOWER(name) LIKE ?{p} OR LOWER(COALESCE(path,'')) LIKE ?{}",
                    p + 1
                )
            })
            .collect::<Vec<_>>()
            .join(" OR ");

        let sql = format!(
            "SELECT {ENTITY_COLUMNS} FROM entities WHERE kind IN ({kind_placeholders}) AND ({text_conditions})"
        );

        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        for kind in kinds {
            params.push(Box::new(kind.to_string()));
        }
        for pattern in like_patterns {
            // Each pattern is used twice (name and path)
            params.push(Box::new(pattern.clone()));
            params.push(Box::new(pattern.clone()));
        }

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = self.conn.prepare(&sql)?;
        let entities = stmt
            .query_map(param_refs.as_slice(), entity_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(entities)
    }

    pub fn delete_entities_by_path(&self, path: &str) -> Result<usize> {
        let count = self
            .conn
            .execute("DELETE FROM entities WHERE path = ?1", [path])?;
        Ok(count)
    }

    // ── Edge operations ─────────────────────────────────────────────────

    pub fn insert_edge(&self, edge: &Edge) -> Result<()> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT OR REPLACE INTO edges
             (src_id, rel, dst_id, provenance_path, provenance_line)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        stmt.execute(params![
            edge.src_id,
            edge.rel.to_string(),
            edge.dst_id,
            edge.provenance_path,
            edge.provenance_line.map(|n| n as i64),
        ])?;
        Ok(())
    }

    pub fn get_edges_from(&self, src_id: &str) -> Result<Vec<Edge>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {EDGE_COLUMNS} FROM edges WHERE src_id = ?1"
        ))?;
        let edges = stmt
            .query_map([src_id], edge_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(edges)
    }

    pub fn get_edges_to(&self, dst_id: &str) -> Result<Vec<Edge>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {EDGE_COLUMNS} FROM edges WHERE dst_id = ?1"
        ))?;
        let edges = stmt
            .query_map([dst_id], edge_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(edges)
    }

    pub fn get_all_edges(&self) -> Result<Vec<Edge>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {EDGE_COLUMNS} FROM edges"))?;
        let edges = stmt
            .query_map([], edge_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(edges)
    }

    pub fn get_edges_by_rel(&self, rel: EdgeKind) -> Result<Vec<Edge>> {
        let mut stmt = self
            .conn
            .prepare_cached(&format!("SELECT {EDGE_COLUMNS} FROM edges WHERE rel = ?1"))?;
        let edges = stmt
            .query_map([rel.to_string()], edge_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(edges)
    }

    pub fn delete_edge(&self, src_id: &str, rel: EdgeKind, dst_id: &str) -> Result<()> {
        self.conn
            .prepare_cached("DELETE FROM edges WHERE src_id = ?1 AND rel = ?2 AND dst_id = ?3")?
            .execute(params![src_id, rel.to_string(), dst_id])?;
        Ok(())
    }

    pub fn delete_edges_by_component(&self, component_id: &ComponentId) -> Result<usize> {
        let count = self.conn.execute(
            "WITH component_entities(id) AS (
                SELECT id FROM entities WHERE component_id = ?1
             )
             DELETE FROM edges WHERE
             src_id IN (SELECT id FROM component_entities) OR
             dst_id IN (SELECT id FROM component_entities)",
            [component_id.as_str()],
        )?;
        Ok(count)
    }

    pub fn delete_edges_for_entity_ids(&self, ids: &[String]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let placeholders: String = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "DELETE FROM edges WHERE src_id IN ({placeholders}) OR dst_id IN ({placeholders})"
        );
        let mut params: Vec<&dyn rusqlite::types::ToSql> = Vec::with_capacity(ids.len() * 2);
        for id in ids {
            params.push(id);
        }
        for id in ids {
            params.push(id);
        }
        let count = self.conn.execute(&sql, params.as_slice())?;
        Ok(count)
    }

    pub fn delete_edges_by_provenance_path(&self, path: &str) -> Result<usize> {
        let count = self
            .conn
            .execute("DELETE FROM edges WHERE provenance_path = ?1", [path])?;
        Ok(count)
    }

    // ── File operations ─────────────────────────────────────────────────

    pub fn insert_file(&self, file: &FileRecord) -> Result<()> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT OR REPLACE INTO files
             (path, component_id, kind, hash, indexed, ignore_reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        stmt.execute(params![
            file.path,
            file.component_id.as_ref().map(|c| c.as_str()),
            file.kind.to_string(),
            file.hash,
            file.indexed as i32,
            file.ignore_reason,
        ])?;
        Ok(())
    }

    pub fn get_file(&self, path: &str) -> Result<Option<FileRecord>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT path, component_id, kind, hash, indexed, ignore_reason FROM files WHERE path = ?1",
        )?;
        let file = stmt.query_row([path], file_from_row).optional()?;
        Ok(file)
    }

    pub fn get_all_files(&self) -> Result<Vec<FileRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, component_id, kind, hash, indexed, ignore_reason FROM files")?;
        let files = stmt
            .query_map([], file_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(files)
    }

    pub fn delete_file(&self, path: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM files WHERE path = ?1", [path])?;
        Ok(())
    }

    pub fn delete_files_by_component(&self, component_id: &ComponentId) -> Result<usize> {
        let count = self.conn.execute(
            "DELETE FROM files WHERE component_id = ?1",
            [component_id.as_str()],
        )?;
        Ok(count)
    }

    // ── Summary operations ──────────────────────────────────────────────

    pub fn insert_summary(&self, summary: &Summary) -> Result<()> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT OR REPLACE INTO summaries
             (entity_id, short_summary, detailed_summary, keywords_json, updated_at, source_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        stmt.execute(params![
            summary.entity_id,
            summary.short_summary,
            summary.detailed_summary,
            summary
                .keywords
                .as_ref()
                .map(|k| serde_json::to_string(k).unwrap_or_default()),
            summary.updated_at,
            summary.source_hash,
        ])?;
        Ok(())
    }

    pub fn get_summary(&self, entity_id: &str) -> Result<Option<Summary>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {SUMMARY_COLUMNS} FROM summaries WHERE entity_id = ?1"
        ))?;
        let summary = stmt.query_row([entity_id], summary_from_row).optional()?;
        Ok(summary)
    }

    pub fn get_all_summaries(&self) -> Result<Vec<Summary>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {SUMMARY_COLUMNS} FROM summaries"))?;
        let summaries = stmt
            .query_map([], summary_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(summaries)
    }

    pub fn search_summaries_by_text(&self, like_patterns: &[String]) -> Result<Vec<Summary>> {
        if like_patterns.is_empty() {
            return Ok(Vec::new());
        }

        // Build: WHERE LOWER(short_summary || ' ' || COALESCE(keywords_json,'')) LIKE ?1
        //           OR LOWER(short_summary || ' ' || COALESCE(keywords_json,'')) LIKE ?2 ...
        let haystack_expr = "LOWER(short_summary || ' ' || COALESCE(keywords_json, ''))";
        let conditions: String = like_patterns
            .iter()
            .enumerate()
            .map(|(i, _)| format!("{haystack_expr} LIKE ?{}", i + 1))
            .collect::<Vec<_>>()
            .join(" OR ");

        let sql = format!("SELECT {SUMMARY_COLUMNS} FROM summaries WHERE {conditions}");

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = like_patterns
            .iter()
            .map(|p| p as &dyn rusqlite::types::ToSql)
            .collect();
        let mut stmt = self.conn.prepare(&sql)?;
        let summaries = stmt
            .query_map(param_refs.as_slice(), summary_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(summaries)
    }

    pub fn delete_summary(&self, entity_id: &str) -> Result<()> {
        self.conn
            .prepare_cached("DELETE FROM summaries WHERE entity_id = ?1")?
            .execute([entity_id])?;
        Ok(())
    }

    // ── Task route operations ───────────────────────────────────────────

    pub fn insert_task_route(&self, route: &TaskRoute) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT OR REPLACE INTO task_routes (task_name, entity_id, priority)
                 VALUES (?1, ?2, ?3)",
            )?
            .execute(params![route.task_name, route.entity_id, route.priority])?;
        Ok(())
    }

    pub fn get_task_routes(&self, task_name: &str) -> Result<Vec<TaskRoute>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT task_name, entity_id, priority FROM task_routes WHERE task_name = ?1",
        )?;
        let routes = stmt
            .query_map([task_name], task_route_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(routes)
    }

    pub fn get_entity_task_routes(&self, entity_id: &str) -> Result<Vec<TaskRoute>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT task_name, entity_id, priority FROM task_routes WHERE entity_id = ?1",
        )?;
        let routes = stmt
            .query_map([entity_id], task_route_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(routes)
    }

    pub fn delete_entity_task_routes(&self, entity_id: &str) -> Result<()> {
        self.conn
            .prepare_cached("DELETE FROM task_routes WHERE entity_id = ?1")?
            .execute([entity_id])?;
        Ok(())
    }

    // ── Embedding metadata operations ───────────────────────────────────

    pub fn insert_embedding_meta(&self, meta: &EmbeddingMeta) -> Result<()> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT OR REPLACE INTO embeddings
             (entity_id, model, dimensions, updated_at, usearch_key)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        stmt.execute(params![
            meta.entity_id,
            meta.model,
            meta.dimensions as i64,
            meta.updated_at,
            meta.usearch_key,
        ])?;
        Ok(())
    }

    pub fn get_embedding_meta(&self, entity_id: &str) -> Result<Option<EmbeddingMeta>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {EMBEDDING_META_COLUMNS} FROM embeddings WHERE entity_id = ?1"
        ))?;
        let meta = stmt
            .query_row([entity_id], embedding_meta_from_row)
            .optional()?;
        Ok(meta)
    }

    pub fn get_all_embedding_metas(&self) -> Result<Vec<EmbeddingMeta>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {EMBEDDING_META_COLUMNS} FROM embeddings"))?;
        let metas = stmt
            .query_map([], embedding_meta_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(metas)
    }

    pub fn delete_embedding_meta(&self, entity_id: &str) -> Result<()> {
        self.conn
            .prepare_cached("DELETE FROM embeddings WHERE entity_id = ?1")?
            .execute([entity_id])?;
        Ok(())
    }

    /// Look up embedding metadata by usearch key (for collision detection).
    pub fn get_embedding_meta_by_usearch_key(
        &self,
        usearch_key: i64,
    ) -> Result<Option<EmbeddingMeta>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {EMBEDDING_META_COLUMNS} FROM embeddings WHERE usearch_key = ?1"
        ))?;
        let meta = stmt
            .query_row([usearch_key], embedding_meta_from_row)
            .optional()?;
        Ok(meta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FileKind;
    use tempfile::TempDir;

    fn create_test_store() -> (SqliteStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::open(&db_path).unwrap();
        (store, temp_dir)
    }

    // ── Entity tests ────────────────────────────────────────────────────

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

    // ── Edge tests ──────────────────────────────────────────────────────

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

    // ── File tests ──────────────────────────────────────────────────────

    #[test]
    fn test_file_record() {
        let (store, _temp) = create_test_store();

        let file = FileRecord::new("src/main.rs", FileKind::Source, "abc123")
            .with_component(ComponentId::new("cargo", "."));

        store.insert_file(&file).unwrap();

        let retrieved = store.get_file("src/main.rs").unwrap().unwrap();
        assert_eq!(retrieved.path, "src/main.rs");
        assert_eq!(retrieved.hash, "abc123");
    }

    // ── Summary tests ───────────────────────────────────────────────────

    #[test]
    fn test_insert_and_get_summary() {
        let (store, _temp) = create_test_store();

        let summary = Summary::new("entity::1", "A short summary")
            .with_detailed("Detailed text here")
            .with_keywords(&["foo", "bar"])
            .with_source_hash("hash123");

        store.insert_summary(&summary).unwrap();

        let retrieved = store.get_summary("entity::1").unwrap().unwrap();
        assert_eq!(retrieved.entity_id, "entity::1");
        assert_eq!(retrieved.short_summary, "A short summary");
        assert_eq!(
            retrieved.detailed_summary,
            Some("Detailed text here".to_string())
        );
        assert!(retrieved.keywords.is_some());
        assert_eq!(retrieved.source_hash, Some("hash123".to_string()));
    }

    #[test]
    fn test_summary_replace_on_reinsert() {
        let (store, _temp) = create_test_store();

        store.insert_summary(&Summary::new("e1", "first")).unwrap();
        store.insert_summary(&Summary::new("e1", "second")).unwrap();

        let retrieved = store.get_summary("e1").unwrap().unwrap();
        assert_eq!(retrieved.short_summary, "second");
    }

    #[test]
    fn test_delete_summary() {
        let (store, _temp) = create_test_store();

        store.insert_summary(&Summary::new("e1", "text")).unwrap();
        assert!(store.get_summary("e1").unwrap().is_some());

        store.delete_summary("e1").unwrap();
        assert!(store.get_summary("e1").unwrap().is_none());
    }

    #[test]
    fn test_get_nonexistent_summary() {
        let (store, _temp) = create_test_store();
        assert!(store.get_summary("does_not_exist").unwrap().is_none());
    }

    // ── Task route tests ────────────────────────────────────────────────

    #[test]
    fn test_insert_and_get_task_routes() {
        let (store, _temp) = create_test_store();

        store
            .insert_task_route(&TaskRoute::new("debug", "entity::1", 80))
            .unwrap();
        store
            .insert_task_route(&TaskRoute::new("debug", "entity::2", 60))
            .unwrap();
        store
            .insert_task_route(&TaskRoute::new("build", "entity::1", 90))
            .unwrap();

        let debug_routes = store.get_task_routes("debug").unwrap();
        assert_eq!(debug_routes.len(), 2);

        let build_routes = store.get_task_routes("build").unwrap();
        assert_eq!(build_routes.len(), 1);
        assert_eq!(build_routes[0].priority, 90);
    }

    #[test]
    fn test_get_entity_task_routes() {
        let (store, _temp) = create_test_store();

        store
            .insert_task_route(&TaskRoute::new("debug", "entity::1", 80))
            .unwrap();
        store
            .insert_task_route(&TaskRoute::new("build", "entity::1", 90))
            .unwrap();
        store
            .insert_task_route(&TaskRoute::new("debug", "entity::2", 70))
            .unwrap();

        let routes = store.get_entity_task_routes("entity::1").unwrap();
        assert_eq!(routes.len(), 2);
    }

    #[test]
    fn test_delete_entity_task_routes() {
        let (store, _temp) = create_test_store();

        store
            .insert_task_route(&TaskRoute::new("debug", "e1", 80))
            .unwrap();
        store
            .insert_task_route(&TaskRoute::new("build", "e1", 90))
            .unwrap();

        store.delete_entity_task_routes("e1").unwrap();

        assert!(store.get_entity_task_routes("e1").unwrap().is_empty());
    }

    #[test]
    fn test_task_route_replace_on_reinsert() {
        let (store, _temp) = create_test_store();

        store
            .insert_task_route(&TaskRoute::new("debug", "e1", 50))
            .unwrap();
        store
            .insert_task_route(&TaskRoute::new("debug", "e1", 99))
            .unwrap();

        let routes = store.get_task_routes("debug").unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].priority, 99);
    }

    // ── Embedding metadata tests ────────────────────────────────────────

    #[test]
    fn test_insert_and_get_embedding_meta() {
        let (store, _temp) = create_test_store();

        let meta = EmbeddingMeta::new("entity::1", "nomic-embed-text", 768).with_usearch_key(42);

        store.insert_embedding_meta(&meta).unwrap();

        let retrieved = store.get_embedding_meta("entity::1").unwrap().unwrap();
        assert_eq!(retrieved.entity_id, "entity::1");
        assert_eq!(retrieved.model, "nomic-embed-text");
        assert_eq!(retrieved.dimensions, 768);
        assert_eq!(retrieved.usearch_key, Some(42));
    }

    #[test]
    fn test_delete_embedding_meta() {
        let (store, _temp) = create_test_store();

        let meta = EmbeddingMeta::new("e1", "model", 768);
        store.insert_embedding_meta(&meta).unwrap();

        store.delete_embedding_meta("e1").unwrap();
        assert!(store.get_embedding_meta("e1").unwrap().is_none());
    }

    #[test]
    fn test_get_nonexistent_embedding_meta() {
        let (store, _temp) = create_test_store();
        assert!(
            store
                .get_embedding_meta("does_not_exist")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn test_embedding_meta_replace_on_reinsert() {
        let (store, _temp) = create_test_store();

        store
            .insert_embedding_meta(&EmbeddingMeta::new("e1", "model_v1", 768))
            .unwrap();
        store
            .insert_embedding_meta(&EmbeddingMeta::new("e1", "model_v2", 1024))
            .unwrap();

        let retrieved = store.get_embedding_meta("e1").unwrap().unwrap();
        assert_eq!(retrieved.model, "model_v2");
        assert_eq!(retrieved.dimensions, 1024);
    }

    #[test]
    fn test_get_embedding_meta_by_usearch_key() {
        let (store, _temp) = create_test_store();

        let meta = EmbeddingMeta::new("e1", "model", 768).with_usearch_key(999);
        store.insert_embedding_meta(&meta).unwrap();

        let retrieved = store
            .get_embedding_meta_by_usearch_key(999)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.entity_id, "e1");

        assert!(
            store
                .get_embedding_meta_by_usearch_key(0)
                .unwrap()
                .is_none()
        );
    }

    // ── Schema / migration tests ────────────────────────────────────────

    #[test]
    fn test_schema_version() {
        let (store, _temp) = create_test_store();

        let version: i32 = store
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();

        assert_eq!(version, SCHEMA_VERSION);
    }

    // ── Error-path tests ────────────────────────────────────────────────

    #[test]
    fn test_get_nonexistent_entity_returns_none() {
        let (store, _temp) = create_test_store();
        assert!(store.get_entity("nonexistent::id").unwrap().is_none());
    }

    #[test]
    fn test_get_nonexistent_file_returns_none() {
        let (store, _temp) = create_test_store();
        assert!(store.get_file("no/such/file.rs").unwrap().is_none());
    }

    #[test]
    fn test_delete_nonexistent_entity_succeeds() {
        let (store, _temp) = create_test_store();
        // Deleting something that doesn't exist should not error
        store.delete_entity("nonexistent::id").unwrap();
    }

    #[test]
    fn test_delete_nonexistent_summary_succeeds() {
        let (store, _temp) = create_test_store();
        store.delete_summary("nonexistent").unwrap();
    }

    #[test]
    fn test_delete_nonexistent_file_succeeds() {
        let (store, _temp) = create_test_store();
        store.delete_file("no/such/file").unwrap();
    }

    #[test]
    fn test_entity_replace_on_reinsert() {
        let (store, _temp) = create_test_store();

        store
            .insert_entity(&Entity::new("e1", EntityKind::Symbol, "old_name"))
            .unwrap();
        store
            .insert_entity(&Entity::new("e1", EntityKind::Symbol, "new_name"))
            .unwrap();

        let retrieved = store.get_entity("e1").unwrap().unwrap();
        assert_eq!(retrieved.name, "new_name");
    }

    #[test]
    fn test_edge_with_nonexistent_entity_ids() {
        let (store, _temp) = create_test_store();
        // Edges referencing non-existent entity IDs should succeed (no FK constraints)
        let edge = Edge::new("ghost_src", EdgeKind::Defines, "ghost_dst");
        store.insert_edge(&edge).unwrap();

        let edges = store.get_edges_from("ghost_src").unwrap();
        assert_eq!(edges.len(), 1);
    }

    #[test]
    fn test_get_edges_from_nonexistent_returns_empty() {
        let (store, _temp) = create_test_store();
        let edges = store.get_edges_from("nothing").unwrap();
        assert!(edges.is_empty());
    }

    #[test]
    fn test_get_task_routes_nonexistent_returns_empty() {
        let (store, _temp) = create_test_store();
        let routes = store.get_task_routes("no_such_task").unwrap();
        assert!(routes.is_empty());
    }

    #[test]
    fn test_get_all_files_empty() {
        let (store, _temp) = create_test_store();
        let files = store.get_all_files().unwrap();
        assert!(files.is_empty());
    }

    // ── Transaction tests ───────────────────────────────────────────────

    #[test]
    fn test_transaction_commit() {
        let (store, _temp) = create_test_store();

        store
            .in_transaction(|s| {
                s.insert_entity(&Entity::new("tx1", EntityKind::Symbol, "committed"))?;
                s.insert_entity(&Entity::new("tx2", EntityKind::Symbol, "committed"))?;
                Ok(())
            })
            .unwrap();

        assert!(store.get_entity("tx1").unwrap().is_some());
        assert!(store.get_entity("tx2").unwrap().is_some());
    }

    #[test]
    fn test_transaction_rollback_on_error() {
        let (store, _temp) = create_test_store();

        let result: Result<()> = store.in_transaction(|s| {
            s.insert_entity(&Entity::new("tx_fail", EntityKind::Symbol, "should_vanish"))?;
            Err(StoreError::Other("intentional failure".into()))
        });

        assert!(result.is_err());
        assert!(store.get_entity("tx_fail").unwrap().is_none());
    }
}
