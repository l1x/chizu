use rusqlite::Connection;

use crate::error::Result;

const SCHEMA_VERSION: i64 = 2;

const DDL: &str = "
CREATE TABLE IF NOT EXISTS _schema_version (
    version INTEGER NOT NULL
);

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
    vector_ref TEXT,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_entities_kind_component_path
    ON entities(kind, component_id, path);

CREATE INDEX IF NOT EXISTS idx_edges_src ON edges(src_id);
CREATE INDEX IF NOT EXISTS idx_edges_dst ON edges(dst_id);
CREATE INDEX IF NOT EXISTS idx_edges_rel ON edges(rel);

CREATE INDEX IF NOT EXISTS idx_files_component ON files(component_id);

CREATE INDEX IF NOT EXISTS idx_task_routes_task ON task_routes(task_name);
CREATE INDEX IF NOT EXISTS idx_task_routes_entity ON task_routes(entity_id);
";

pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(DDL)?;

    let count: i64 =
        conn.query_row("SELECT COUNT(*) FROM _schema_version", [], |row| row.get(0))?;

    if count == 0 {
        conn.execute(
            "INSERT INTO _schema_version (version) VALUES (?1)",
            [SCHEMA_VERSION],
        )?;
    } else {
        migrate_v1_to_v2(conn)?;
    }

    Ok(())
}

fn migrate_v1_to_v2(conn: &Connection) -> Result<()> {
    let version: i64 =
        conn.query_row("SELECT version FROM _schema_version LIMIT 1", [], |row| {
            row.get(0)
        })?;

    if version < 2 {
        // Check if column already exists (idempotent)
        let has_column: bool = conn
            .prepare("SELECT source_hash FROM summaries LIMIT 0")
            .is_ok();
        if !has_column {
            conn.execute_batch("ALTER TABLE summaries ADD COLUMN source_hash TEXT")?;
        }
        conn.execute("UPDATE _schema_version SET version = ?1", [SCHEMA_VERSION])?;
    }

    Ok(())
}

pub fn schema_version(conn: &Connection) -> Result<i64> {
    let version = conn.query_row("SELECT version FROM _schema_version LIMIT 1", [], |row| {
        row.get(0)
    })?;
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_schema_sets_version() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn init_schema_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        init_schema(&conn).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn migrate_v1_to_v2_adds_source_hash() {
        let conn = Connection::open_in_memory().unwrap();
        // Simulate a v1 database
        conn.execute_batch(DDL).unwrap();
        conn.execute("INSERT INTO _schema_version (version) VALUES (1)", [])
            .unwrap();

        // Run migration
        migrate_v1_to_v2(&conn).unwrap();

        assert_eq!(schema_version(&conn).unwrap(), 2);
        // Verify the column exists by inserting with it
        conn.execute(
            "INSERT INTO summaries (entity_id, short_summary, updated_at, source_hash) VALUES ('x', 'y', 'z', 'hash123')",
            [],
        )
        .unwrap();
        let hash: String = conn
            .query_row(
                "SELECT source_hash FROM summaries WHERE entity_id = 'x'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(hash, "hash123");
    }
}
