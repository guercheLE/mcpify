use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::context::GeneratorContext;
use crate::targets::rust::context::RsTemplateContext;
use crate::targets::rust::emit::render_and_write;
use crate::targets::rust::render::render_engine;

/// Rendered via Tera — structurally static regardless of the spec.
/// `core/mcp_server.rs` isn't listed here even though this step is what
/// gives it its final, tool-bearing content: it's already emitted by
/// `steps::enterprise` (Story R3), which owns that file's place in the
/// generation pipeline — this step only changed what the *template*
/// renders, not which step writes it.
const FILES: &[(&str, &str)] = &[
    ("data/mod.rs.tera", "src/data/mod.rs"),
    ("data/store.rs.tera", "src/data/store.rs"),
    ("services/mod.rs.tera", "src/services/mod.rs"),
    (
        "services/embedding_service.rs.tera",
        "src/services/embedding_service.rs",
    ),
    ("services/api_client.rs.tera", "src/services/api_client.rs"),
    ("validation/mod.rs.tera", "src/validation/mod.rs"),
    (
        "validation/validator.rs.tera",
        "src/validation/validator.rs",
    ),
    ("tools/mod.rs.tera", "src/tools/mod.rs"),
    ("tools/search_tool.rs.tera", "src/tools/search_tool.rs"),
    ("tools/get_tool.rs.tera", "src/tools/get_tool.rs"),
    ("tools/call_tool.rs.tera", "src/tools/call_tool.rs"),
];

const GENERATED_SCHEMAS_PATH: &str = "src/validation/generated_schemas.json";

/// `generate_mcp_tools` (architecture.md §1, step 9): the data-access
/// layer, embedding/API-client services, validator, and 3 tool modules
/// against `mcp_store.db` and the target-API HTTP client, plus the
/// `generated_schemas.json` asset `validation/validator.rs`'s
/// `include_str!` bakes in at compile time.
pub async fn generate_mcp_tools(ctx: &GeneratorContext) -> Result<()> {
    let view = RsTemplateContext::from_context(ctx);
    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in FILES {
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }

    write_generated_schemas(ctx).await?;

    Ok(())
}

/// Built directly with `serde_json` rather than through a Tera loop: the
/// per-operation JSON Schema documents (from `schema_resolve.rs`, already
/// `$ref`-resolved) are genuine data, not boilerplate text, and a
/// hundreds-of-operations spec would make a loop-heavy `.tera` template
/// slow to render and unreadable to maintain — mirrors
/// `targets::typescript::steps::tools::write_generated_schemas`.
async fn write_generated_schemas(ctx: &GeneratorContext) -> Result<()> {
    let mut schemas = Map::new();
    for operation in &ctx.normalized_operations {
        schemas.insert(
            operation.operation_id.clone(),
            serde_json::json!({
                "inputSchema": operation.validation_input_schema,
                "outputSchema": operation.validation_output_schema,
            }),
        );
    }

    let json_text = serde_json::to_string_pretty(&Value::Object(schemas))
        .context("failed to serialize generated_schemas.json")?;

    let out_path = ctx.output_dir.join(GENERATED_SCHEMAS_PATH);
    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create directory '{}'", parent.display()))?;
    }
    tokio::fs::write(&out_path, json_text)
        .await
        .with_context(|| format!("failed to write '{}'", out_path.display()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::openapi::NormalizedOperation;

    fn ctx_with_operations(
        output_dir: PathBuf,
        normalized_operations: Vec<NormalizedOperation>,
    ) -> GeneratorContext {
        GeneratorContext {
            publish_registry: false,
            openapi_input: "spec.yaml".to_string(),
            output_dir,
            force: false,
            output_dir_preexisted: false,
            auth_schemes: Vec::new(),
            normalized_operations,
            api_title: "Widget API".to_string(),
        }
    }

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
    async fn writes_every_tool_file() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_operations(dir.path().to_path_buf(), vec![sample_operation()]);

        generate_mcp_tools(&ctx).await.unwrap();

        for (_, out_name) in FILES {
            assert!(dir.path().join(out_name).is_file(), "missing {out_name}");
        }
        assert!(dir.path().join(GENERATED_SCHEMAS_PATH).is_file());
    }

    #[tokio::test]
    async fn generated_schemas_json_round_trips_operation_schemas() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_operations(dir.path().to_path_buf(), vec![sample_operation()]);

        generate_mcp_tools(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.path().join(GENERATED_SCHEMAS_PATH))
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&contents).unwrap();

        assert_eq!(parsed["listWidgets"]["inputSchema"]["type"], "object");
        assert_eq!(parsed["listWidgets"]["outputSchema"]["type"], "array");
    }

    #[tokio::test]
    async fn generated_schemas_json_is_empty_object_with_no_operations() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_operations(dir.path().to_path_buf(), Vec::new());

        generate_mcp_tools(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.path().join(GENERATED_SCHEMAS_PATH))
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed, Value::Object(Map::new()));
    }
}
