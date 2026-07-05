use anyhow::Result;

use crate::context::GeneratorContext;
use crate::targets::typescript::context::TsTemplateContext;
use crate::targets::typescript::emit::render_and_write;
use crate::targets::typescript::render::render_engine;

/// Rendered via Tera — structurally static regardless of the spec.
const FILES: &[(&str, &str)] = &[
    (
        "services/embedding-service.ts.tera",
        "src/services/embedding-service.ts",
    ),
    (
        "data/store-repository.ts.tera",
        "src/data/store-repository.ts",
    ),
    ("services/api-client.ts.tera", "src/services/api-client.ts"),
    (
        "validation/validator.ts.tera",
        "src/validation/validator.ts",
    ),
    ("tools/search-tool.ts.tera", "src/tools/search-tool.ts"),
    ("tools/get-tool.ts.tera", "src/tools/get-tool.ts"),
    ("tools/call-tool.ts.tera", "src/tools/call-tool.ts"),
    ("tools/tool-executor.ts.tera", "src/tools/tool-executor.ts"),
    (
        "tools/register-tools.ts.tera",
        "src/tools/register-tools.ts",
    ),
];

pub(crate) const GENERATED_SCHEMAS_PATH: &str = "src/validation/generated-schemas.json.zst";

/// `generate_mcp_tools` (architecture.md §1, step 9): the 3 tool modules
/// against `mcp_store.db` and the target-API HTTP client, plus the
/// zstd-compressed generated-schemas.json.zst asset Ajv validates against
/// at runtime (decompressed once and cached — see validator.ts).
pub async fn generate_mcp_tools(ctx: &GeneratorContext) -> Result<()> {
    let view = TsTemplateContext::from_context(ctx);
    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in FILES {
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }

    write_generated_schemas(ctx).await?;

    Ok(())
}

async fn write_generated_schemas(ctx: &GeneratorContext) -> Result<()> {
    crate::schemas_asset::write_schemas_json_at(
        &ctx.normalized_operations,
        &ctx.output_dir.join(GENERATED_SCHEMAS_PATH),
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::{Map, Value};

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
            version_label: "default".to_string(),
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

        let contents = tokio::fs::read(dir.path().join(GENERATED_SCHEMAS_PATH))
            .await
            .unwrap();
        let decompressed = zstd::decode_all(contents.as_slice()).unwrap();
        let parsed: Value = serde_json::from_slice(&decompressed).unwrap();

        assert_eq!(parsed["listWidgets"]["inputSchema"]["type"], "object");
        assert_eq!(parsed["listWidgets"]["outputSchema"]["type"], "array");
    }

    #[tokio::test]
    async fn generated_schemas_json_is_empty_object_with_no_operations() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_operations(dir.path().to_path_buf(), Vec::new());

        generate_mcp_tools(&ctx).await.unwrap();

        let contents = tokio::fs::read(dir.path().join(GENERATED_SCHEMAS_PATH))
            .await
            .unwrap();
        let decompressed = zstd::decode_all(contents.as_slice()).unwrap();
        let parsed: Value = serde_json::from_slice(&decompressed).unwrap();
        assert_eq!(parsed, Value::Object(Map::new()));
    }
}
