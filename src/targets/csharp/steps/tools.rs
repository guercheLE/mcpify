use anyhow::Result;

use crate::context::GeneratorContext;
use crate::targets::csharp::context::CsTemplateContext;
use crate::targets::csharp::emit::render_and_write;
use crate::targets::csharp::render::render_engine;

/// Rendered via Tera — structurally static regardless of the spec.
const FILES: &[(&str, &str)] = &[
    ("Data/EndpointRecord.cs.tera", "Data/EndpointRecord.cs"),
    ("Data/SqliteVecStore.cs.tera", "Data/SqliteVecStore.cs"),
    (
        "Services/EmbeddingService.cs.tera",
        "Services/EmbeddingService.cs",
    ),
    ("Services/ApiClient.cs.tera", "Services/ApiClient.cs"),
    (
        "Services/PopulateEmbeddingsService.cs.tera",
        "Services/PopulateEmbeddingsService.cs",
    ),
    ("Validation/Validator.cs.tera", "Validation/Validator.cs"),
    ("Tools/McpTools.cs.tera", "Tools/McpTools.cs"),
    ("Core/DataStore.cs.tera", "Core/DataStore.cs"),
    ("Core/EmbeddingService.cs.tera", "Core/EmbeddingService.cs"),
    ("Core/ApiClient.cs.tera", "Core/ApiClient.cs"),
    ("Cli/VersionsCommand.cs.tera", "Cli/VersionsCommand.cs"),
];

/// Re-rendered here even though earlier stories' steps already wrote
/// them once: this story's edits to the underlying `.tera` templates
/// (`.WithToolsFromAssembly()` in `Core/McpServer.cs`, `app.MapMcp("/mcp")`
/// in `Http/HttpServer.cs`, the `search`/`get`/`call`/
/// `populate-embeddings` subcommands in `Program.cs`) only take effect
/// once something re-renders them — there is exactly one authored copy
/// of each template, edited in place across stories, per the same
/// pattern `steps::enterprise` already established for `Program.cs`.
const RERENDERED_FILES: &[(&str, &str)] = &[
    ("Program.cs.tera", "Program.cs"),
    ("Core/McpServer.cs.tera", "Core/McpServer.cs"),
    ("Http/HttpServer.cs.tera", "Http/HttpServer.cs"),
];

pub(crate) const GENERATED_SCHEMAS_PATH: &str = "Validation/GeneratedSchemas.json.zst";

/// `generate_mcp_tools` (architecture.md §1, step 9): the data-access
/// layer, embedding/API-client services, validator, and the `search`/
/// `get`/`call` MCP tools against `mcp_store.db` and the target-API HTTP
/// client, plus the zstd-compressed `GeneratedSchemas.json.zst` asset
/// `Validation/Validator.cs` embeds as a resource at compile time,
/// decompressing it once into memory the first time it's needed.
pub async fn generate_mcp_tools(ctx: &GeneratorContext) -> Result<()> {
    let view = CsTemplateContext::from_context(ctx);
    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in FILES {
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }

    for (template, out_name) in RERENDERED_FILES {
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
        for (_, out_name) in RERENDERED_FILES {
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

    #[tokio::test]
    async fn program_cs_dispatches_search_get_call_and_populate_embeddings() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_operations(dir.path().to_path_buf(), vec![sample_operation()]);

        generate_mcp_tools(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.path().join("Program.cs"))
            .await
            .unwrap();
        for command in [
            "\"search\"",
            "\"get\"",
            "\"call\"",
            "\"populate-embeddings\"",
            "\"versions\"",
        ] {
            assert!(contents.contains(command), "missing {command} subcommand");
        }
    }

    #[tokio::test]
    async fn http_server_mounts_mcp_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_operations(dir.path().to_path_buf(), vec![sample_operation()]);

        generate_mcp_tools(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.path().join("Http").join("HttpServer.cs"))
            .await
            .unwrap();
        assert!(contents.contains("app.MapMcp(\"/mcp\")"));
    }
}
