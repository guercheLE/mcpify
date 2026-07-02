use anyhow::{Context, Result};
use serde_json::{Map, Value};

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

const GENERATED_SCHEMAS_PATH: &str = "Validation/GeneratedSchemas.json";

/// `generate_mcp_tools` (architecture.md §1, step 9): the data-access
/// layer, embedding/API-client services, validator, and the `search`/
/// `get`/`call` MCP tools against `mcp_store.db` and the target-API HTTP
/// client, plus the `GeneratedSchemas.json` asset `Validation/Validator.cs`
/// embeds as a resource at compile time.
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

/// Built directly with `serde_json` rather than through a Tera loop: the
/// per-operation JSON Schema documents (already `$ref`-resolved) are
/// genuine data, not boilerplate text, and a hundreds-of-operations spec
/// would make a loop-heavy `.tera` template slow to render and unreadable
/// to maintain — mirrors
/// `targets::rust::steps::tools::write_generated_schemas`.
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
        .context("failed to serialize GeneratedSchemas.json")?;

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
    #[ignore = "manual sanity check: requires the dotnet SDK; not part of CI until C9 wires .NET into the pipeline"]
    async fn full_scaffold_through_c6_builds_with_dotnet() {
        let dir = tempfile::tempdir().unwrap();
        let output_dir = dir.path().to_path_buf();
        let generator_ctx = ctx_with_operations(
            output_dir.clone(),
            vec![
                sample_operation(),
                NormalizedOperation {
                    operation_id: "createWidget".to_string(),
                    path: "/widgets/{id}".to_string(),
                    method: "POST".to_string(),
                    summary: Some("Create a widget".to_string()),
                    description: None,
                    input_schema: serde_json::json!({}),
                    output_schema: serde_json::json!({}),
                    auth_scheme_ref: None,
                    validation_input_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "id": {"type": "string"},
                            "body": {"type": "object"},
                        },
                    }),
                    validation_output_schema: serde_json::json!({"type": "object"}),
                },
            ],
        );
        let auth_ctx = GeneratorContext {
            auth_schemes: vec![crate::auth_profile::AuthSchemeDescriptor {
                name: "basicAuth".to_string(),
                kind: crate::auth_profile::AuthSchemeKind::Basic,
            }],
            ..generator_ctx
        };

        crate::targets::csharp::steps::bootstrap::bootstrap_project(&auth_ctx)
            .await
            .unwrap();
        crate::targets::csharp::steps::enterprise::generate_enterprise_scaffolding(&auth_ctx)
            .await
            .unwrap();
        crate::targets::csharp::steps::auth::generate_auth_strategies(&auth_ctx)
            .await
            .unwrap();
        crate::targets::csharp::steps::transports::generate_transports_and_roles(&auth_ctx)
            .await
            .unwrap();
        generate_mcp_tools(&auth_ctx).await.unwrap();

        let status = std::process::Command::new("dotnet")
            .arg("build")
            .current_dir(&output_dir)
            .status()
            .unwrap();
        assert!(
            status.success(),
            "dotnet build failed for the full C2-C6 scaffold"
        );

        let format_status = std::process::Command::new("dotnet")
            .args(["format", "--verify-no-changes"])
            .current_dir(&output_dir)
            .status()
            .unwrap();
        assert!(
            format_status.success(),
            "dotnet format --verify-no-changes failed for the full C2-C6 scaffold"
        );
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
