pub mod open;
pub mod populate;
pub mod schema;

use std::path::PathBuf;

use anyhow::Result;

use crate::context::GeneratorContext;

pub const STORE_FILE_NAME: &str = "mcp_store.db";

/// Creates and populates `mcp_store.db` in `ctx.output_dir` (architecture.md
/// §1, step 4): the relational `endpoints` table, fully populated from
/// `ctx.normalized_operations`, and the `semantic_endpoints` vec0 table
/// (schema only — vectors are computed later by the generated TypeScript
/// project's `populate-embeddings` script, not here; see the plan's
/// embeddings decision).
pub async fn assemble_store(ctx: &GeneratorContext) -> Result<PathBuf> {
    let store_path = ctx.output_dir.join(STORE_FILE_NAME);
    let operations = ctx.normalized_operations.clone();

    // A --force regeneration into a directory that already has a populated
    // mcp_store.db from a previous run must start from a clean slate, or
    // re-inserting the same operation_ids hits endpoints' PRIMARY KEY
    // constraint. CREATE TABLE IF NOT EXISTS alone doesn't clear stale rows.
    if ctx.output_dir_preexisted && tokio::fs::try_exists(&store_path).await.unwrap_or(false) {
        tokio::fs::remove_file(&store_path).await.ok();
    }

    // rusqlite::Connection isn't Send-friendly to hold across .await, so the
    // (fast, local) DB work runs on a blocking thread rather than inline on
    // the async runtime.
    let path_for_task = store_path.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let conn = open::open_store(&path_for_task)?;
        schema::create_tables(&conn)?;
        populate::insert_endpoints(&conn, &operations)?;
        Ok(())
    })
    .await??;

    Ok(store_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openapi::NormalizedOperation;

    fn ctx_with_operations(
        output_dir: PathBuf,
        operations: Vec<NormalizedOperation>,
    ) -> GeneratorContext {
        GeneratorContext {
            publish_registry: false,
            openapi_input: "spec.yaml".to_string(),
            output_dir,
            force: false,
            output_dir_preexisted: false,
            auth_schemes: Vec::new(),
            normalized_operations: operations,
            api_title: "Test API".to_string(),
        }
    }

    #[tokio::test]
    async fn assembles_store_with_no_operations() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_operations(dir.path().to_path_buf(), Vec::new());

        let store_path = assemble_store(&ctx).await.unwrap();

        assert_eq!(store_path, dir.path().join("mcp_store.db"));
        assert!(store_path.exists());
    }

    #[tokio::test]
    async fn assembles_store_with_operations() {
        let dir = tempfile::tempdir().unwrap();
        let operation = NormalizedOperation {
            operation_id: "listWidgets".to_string(),
            path: "/widgets".to_string(),
            method: "GET".to_string(),
            summary: None,
            description: None,
            input_schema: serde_json::json!({}),
            output_schema: serde_json::json!({}),
            auth_scheme_ref: None,
            validation_input_schema: serde_json::json!({}),
            validation_output_schema: serde_json::json!({}),
        };
        let ctx = ctx_with_operations(dir.path().to_path_buf(), vec![operation]);

        let store_path = assemble_store(&ctx).await.unwrap();

        let conn = open::open_store(&store_path).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM endpoints", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn reassembling_into_a_preexisting_dir_does_not_hit_a_duplicate_key_error() {
        let dir = tempfile::tempdir().unwrap();
        let operation = NormalizedOperation {
            operation_id: "listWidgets".to_string(),
            path: "/widgets".to_string(),
            method: "GET".to_string(),
            summary: None,
            description: None,
            input_schema: serde_json::json!({}),
            output_schema: serde_json::json!({}),
            auth_scheme_ref: None,
            validation_input_schema: serde_json::json!({}),
            validation_output_schema: serde_json::json!({}),
        };

        // First run, as if output_dir was created fresh.
        let mut ctx = ctx_with_operations(dir.path().to_path_buf(), vec![operation.clone()]);
        assemble_store(&ctx).await.unwrap();

        // A --force regeneration into the same (now preexisting) directory
        // must not fail with a PRIMARY KEY conflict on the same operation_id.
        ctx.output_dir_preexisted = true;
        let store_path = assemble_store(&ctx).await.unwrap();

        let conn = open::open_store(&store_path).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM endpoints", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
