use anyhow::Result;

use crate::context::GeneratorContext;
use crate::targets::go::context::GoTemplateContext;
use crate::targets::go::emit::render_and_write;
use crate::targets::go::render::render_engine;

/// Rendered via Tera — structurally static regardless of the spec.
/// `internal/core/mcpserver.go` isn't listed here even though this step is
/// what gives it its final, tool-bearing meaning: it's already emitted by
/// `steps::enterprise` (Story G3), which owns that file's place in the
/// generation pipeline — this step only supplies the packages that file's
/// eventual callers (Story G5's transports/roles) import
/// (`internal/data`, `internal/tools`, `internal/services`), not the file
/// itself. Mirrors `targets::python::steps::tools::FILES`.
const FILES: &[(&str, &str)] = &[
    ("internal/data/store.go.tera", "internal/data/store.go"),
    (
        "internal/services/embedding.go.tera",
        "internal/services/embedding.go",
    ),
    (
        "internal/services/vectorstore.go.tera",
        "internal/services/vectorstore.go",
    ),
    (
        "internal/services/populate.go.tera",
        "internal/services/populate.go",
    ),
    (
        "internal/services/apiclient.go.tera",
        "internal/services/apiclient.go",
    ),
    (
        "internal/validation/validator.go.tera",
        "internal/validation/validator.go",
    ),
    ("internal/tools/search.go.tera", "internal/tools/search.go"),
    ("internal/tools/get.go.tera", "internal/tools/get.go"),
    ("internal/tools/call.go.tera", "internal/tools/call.go"),
    (
        "cmd/populate-embeddings/main.go.tera",
        "cmd/populate-embeddings/main.go",
    ),
];

pub(crate) const GENERATED_SCHEMAS_RELATIVE_PATH: &str =
    "internal/validation/generated_schemas.json";

/// `generate_mcp_tools` (architecture.md §1, step 9): the data-access
/// layer, embedding/vector-store/API-client services, validator, and 3
/// tool packages against `mcp_store.db` and the target-API HTTP client,
/// plus the `generated_schemas.json` asset `internal/validation/validator.go`
/// embeds via `go:embed` at compile time (rather than reading it from disk
/// at runtime — Go's `go:embed` directive is the natural fit here, closer
/// to `targets::rust`'s `include_str!` approach than
/// `targets::python`/`targets::csharp`'s runtime file reads).
///
/// This is also where the embeddings decision (v5-implementation-plan.md's
/// open decision #5) actually gets proven: `services/embedding.go`
/// composes `yalue/onnxruntime_go` + `sugarme/tokenizer` directly
/// (`clems4ever/all-minilm-l6-v2-go` was dropped after its `go:embed`-bundled
/// model turned out to be a broken Git LFS pointer when installed as a
/// normal dependency), downloading the `Xenova/all-MiniLM-L6-v2` model and
/// tokenizer from Hugging Face at first run and caching them locally, fed
/// into a `philippgille/chromem-go` collection persisted alongside
/// `mcp_store.db` — one `Embed` function reused by both
/// `services/populate.go` and the live `search` tool's query embedding.
pub async fn generate_mcp_tools(ctx: &GeneratorContext) -> Result<()> {
    let view = GoTemplateContext::from_context(ctx);
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
        &ctx.output_dir.join(GENERATED_SCHEMAS_RELATIVE_PATH),
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

    fn output_dir(parent: &tempfile::TempDir) -> PathBuf {
        parent.path().join("output")
    }

    #[tokio::test]
    async fn writes_every_tool_file() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_operations(dir.clone(), vec![sample_operation()]);

        generate_mcp_tools(&ctx).await.unwrap();

        for (_, out_name) in FILES {
            assert!(dir.join(out_name).is_file(), "missing {out_name}");
        }
        assert!(dir.join(GENERATED_SCHEMAS_RELATIVE_PATH).is_file());
    }

    #[tokio::test]
    async fn generated_schemas_json_round_trips_operation_schemas() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_operations(dir.clone(), vec![sample_operation()]);

        generate_mcp_tools(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.join(GENERATED_SCHEMAS_RELATIVE_PATH))
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&contents).unwrap();

        assert_eq!(parsed["listWidgets"]["inputSchema"]["type"], "object");
        assert_eq!(parsed["listWidgets"]["outputSchema"]["type"], "array");
    }

    #[tokio::test]
    async fn generated_schemas_json_is_empty_object_with_no_operations() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_operations(dir.clone(), Vec::new());

        generate_mcp_tools(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.join(GENERATED_SCHEMAS_RELATIVE_PATH))
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed, Value::Object(Map::new()));
    }
}
