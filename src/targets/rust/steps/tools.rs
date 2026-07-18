use anyhow::Result;

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

pub(crate) const GENERATED_SCHEMAS_PATH: &str = "src/validation/generated_schemas.json.zst";

/// `generate_mcp_tools` (architecture.md §1, step 9): the data-access
/// layer, embedding/API-client services, validator, and 3 tool modules
/// against `mcp_store.db` and the target-API HTTP client, plus the
/// zstd-compressed `generated_schemas.json.zst` asset `validation/validator.rs`'s
/// `include_bytes!` bakes in at compile time.
pub async fn generate_mcp_tools(ctx: &GeneratorContext) -> Result<()> {
    let view = RsTemplateContext::from_context(ctx);
    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in FILES {
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }

    write_generated_schemas(ctx).await?;

    // `store.rs.tera`'s `VERSION_STORE_BYTES` (just rendered above) embeds
    // this version's store `.db.zst`, not the raw `.db` `db::assemble_store`
    // wrote before any target-specific step ran — compress it in place now
    // so a `cargo build` of the freshly generated project can already
    // resolve the `include_bytes!` path.
    crate::store_compress::compress_and_remove_raw(
        &ctx.output_dir.join(crate::db::STORE_FILE_NAME),
    )
    .await?;

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
