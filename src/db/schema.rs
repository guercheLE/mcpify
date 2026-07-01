use anyhow::{Context, Result};
use rusqlite::Connection;

/// One row per operation: path, method, summary, description, JSON-encoded
/// input/output schemas, and a reference to the auth scheme it requires
/// (architecture.md §1, step 4).
pub const CREATE_ENDPOINTS_TABLE: &str = "
CREATE TABLE IF NOT EXISTS endpoints (
    operation_id    TEXT PRIMARY KEY,
    path            TEXT NOT NULL,
    method          TEXT NOT NULL,
    summary         TEXT,
    description     TEXT,
    input_schema    TEXT NOT NULL,
    output_schema   TEXT NOT NULL,
    auth_scheme_ref TEXT
)";

/// 768 dims to match the `Xenova/all-mpnet-base-v2` embedding model used by
/// the generated TypeScript project's `populate-embeddings` script and its
/// `search` tool (see the plan's embeddings decision). Rust only creates
/// this table's schema here — it never inserts a vector into it.
pub const CREATE_SEMANTIC_ENDPOINTS_TABLE: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS semantic_endpoints USING vec0(
    operation_id TEXT PRIMARY KEY,
    embedding FLOAT[768]
)";

pub fn create_tables(conn: &Connection) -> Result<()> {
    conn.execute(CREATE_ENDPOINTS_TABLE, [])
        .context("failed to create 'endpoints' table")?;
    conn.execute(CREATE_SEMANTIC_ENDPOINTS_TABLE, [])
        .context("failed to create 'semantic_endpoints' vec0 table")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open::open_store;

    #[test]
    fn creates_tables_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_store(&dir.path().join("mcp_store.db")).unwrap();

        create_tables(&conn).unwrap();
        create_tables(&conn).unwrap(); // must not error the second time

        let endpoints_exists: bool = conn
            .query_row(
                "SELECT count(*) > 0 FROM sqlite_master WHERE type='table' AND name='endpoints'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(endpoints_exists);
    }

    #[test]
    fn semantic_endpoints_table_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_store(&dir.path().join("mcp_store.db")).unwrap();
        create_tables(&conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT count(*) FROM semantic_endpoints", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }
}
