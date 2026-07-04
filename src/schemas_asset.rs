use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::openapi::NormalizedOperation;

/// Builds the "generated schemas" JSON asset every target's validator reads
/// (Ajv/jsonschema/etc.) at runtime, keyed by `operation_id`. Identical
/// shape across all 5 targets — only the *destination path* and how the
/// runtime loads it differ — so this is shared rather than duplicated in
/// each `targets::<lang>::steps::tools` module and re-used as-is by v8's
/// `add-version` command (which writes this same shape at a version-suffixed
/// path, without needing any target-specific knowledge).
///
/// Built directly with `serde_json` rather than through a Tera loop: the
/// per-operation JSON Schema documents (from `openapi::schema_resolve`,
/// already `$ref`-resolved) are genuine data, not boilerplate text, and a
/// hundreds-of-operations spec would make a loop-heavy `.tera` template
/// slow to render and unreadable to maintain.
pub async fn write_schemas_json_at(
    operations: &[NormalizedOperation],
    out_path: &Path,
) -> Result<()> {
    let mut schemas = Map::new();
    for operation in operations {
        schemas.insert(
            operation.operation_id.clone(),
            serde_json::json!({
                "inputSchema": operation.validation_input_schema,
                "outputSchema": operation.validation_output_schema,
            }),
        );
    }

    let json_text = serde_json::to_string_pretty(&Value::Object(schemas))
        .context("failed to serialize generated schemas JSON")?;

    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create directory '{}'", parent.display()))?;
    }
    tokio::fs::write(out_path, json_text)
        .await
        .with_context(|| format!("failed to write '{}'", out_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_operation() -> NormalizedOperation {
        NormalizedOperation {
            operation_id: "listWidgets".to_string(),
            path: "/widgets".to_string(),
            method: "GET".to_string(),
            summary: Some("List widgets".to_string()),
            description: None,
            input_schema: serde_json::json!({}),
            output_schema: serde_json::json!({}),
            auth_scheme_ref: None,
            validation_input_schema: serde_json::json!({"type": "object", "properties": {}}),
            validation_output_schema: serde_json::json!({"type": "array"}),
        }
    }

    #[tokio::test]
    async fn round_trips_operation_schemas() {
        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("generated-schemas.json");

        write_schemas_json_at(&[sample_operation()], &out_path)
            .await
            .unwrap();

        let contents = tokio::fs::read_to_string(&out_path).await.unwrap();
        let parsed: Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed["listWidgets"]["inputSchema"]["type"], "object");
        assert_eq!(parsed["listWidgets"]["outputSchema"]["type"], "array");
    }

    #[tokio::test]
    async fn is_an_empty_object_with_no_operations() {
        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("generated-schemas.json");

        write_schemas_json_at(&[], &out_path).await.unwrap();

        let contents = tokio::fs::read_to_string(&out_path).await.unwrap();
        let parsed: Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed, Value::Object(Map::new()));
    }

    #[tokio::test]
    async fn creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let out_path = dir
            .path()
            .join("nested")
            .join("dir")
            .join("generated-schemas.json");

        write_schemas_json_at(&[sample_operation()], &out_path)
            .await
            .unwrap();

        assert!(out_path.is_file());
    }
}
