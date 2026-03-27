pub mod edges;
pub mod embeddings;
pub mod entities;
pub mod files;
pub mod stats;
pub mod summaries;
pub mod task_routes;
mod usearch;

use rusqlite::Connection;

use super::schema;
use crate::error::Result;

pub struct SqliteStore {
    conn: Connection,
    vector_index: std::cell::RefCell<Option<::usearch::Index>>,
    db_path: Option<String>,
}

impl SqliteStore {
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        schema::init_schema(&conn)?;
        Ok(Self {
            conn,
            vector_index: std::cell::RefCell::new(None),
            db_path: None,
        })
    }

    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        schema::init_schema(&conn)?;
        Ok(Self {
            conn,
            vector_index: std::cell::RefCell::new(None),
            db_path: Some(path.to_string()),
        })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn schema_version(&self) -> Result<i64> {
        schema::schema_version(&self.conn)
    }

    pub fn begin_transaction(&self) -> Result<()> {
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        Ok(())
    }

    pub fn commit_transaction(&self) -> Result<()> {
        self.conn.execute_batch("COMMIT")?;
        Ok(())
    }

    pub fn rollback_transaction(&self) -> Result<()> {
        self.conn.execute_batch("ROLLBACK")?;
        Ok(())
    }
}
