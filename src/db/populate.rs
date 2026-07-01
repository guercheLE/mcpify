use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::openapi::NormalizedOperation;

/// Inserts one row per operation into `endpoints`, JSON-encoding the
/// input/output schema snapshots.
pub fn insert_endpoints(conn: &Connection, operations: &[NormalizedOperation]) -> Result<()> {
    let mut statement = conn.prepare(
        "INSERT INTO endpoints
            (operation_id, path, method, summary, description, input_schema, output_schema, auth_scheme_ref)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;

    for operation in operations {
        statement
            .execute(rusqlite::params![
                operation.operation_id,
                operation.path,
                operation.method,
                operation.summary,
                operation.description,
                serde_json::to_string(&operation.input_schema)?,
                serde_json::to_string(&operation.output_schema)?,
                operation.auth_scheme_ref,
            ])
            .with_context(|| format!("failed to insert endpoint '{}'", operation.operation_id))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::db::open::open_store;
    use crate::db::schema::create_tables;

    fn sample_operation(operation_id: &str) -> NormalizedOperation {
        NormalizedOperation {
            operation_id: operation_id.to_string(),
            path: "/widgets".to_string(),
            method: "GET".to_string(),
            summary: Some("List widgets".to_string()),
            description: None,
            input_schema: json!({"parameters": []}),
            output_schema: json!({"200": {"description": "OK"}}),
            auth_scheme_ref: Some("basicAuth".to_string()),
            validation_input_schema: json!({}),
            validation_output_schema: json!({}),
        }
    }

    #[test]
    fn round_trips_a_row() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_store(&dir.path().join("mcp_store.db")).unwrap();
        create_tables(&conn).unwrap();

        insert_endpoints(&conn, &[sample_operation("listWidgets")]).unwrap();

        let (path, method, auth_ref): (String, String, Option<String>) = conn
            .query_row(
                "SELECT path, method, auth_scheme_ref FROM endpoints WHERE operation_id = ?1",
                ["listWidgets"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();

        assert_eq!(path, "/widgets");
        assert_eq!(method, "GET");
        assert_eq!(auth_ref.as_deref(), Some("basicAuth"));
    }

    #[test]
    fn inserts_multiple_operations() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_store(&dir.path().join("mcp_store.db")).unwrap();
        create_tables(&conn).unwrap();

        insert_endpoints(
            &conn,
            &[
                sample_operation("listWidgets"),
                sample_operation("getWidget"),
            ],
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT count(*) FROM endpoints", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }
}
