use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::context::GeneratorContext;
use crate::targets::python::context::PyTemplateContext;
use crate::targets::python::emit::render_and_write;
use crate::targets::python::render::render_engine;

/// Rendered via Tera — structurally static regardless of the spec.
/// `core/mcp_server.py` isn't listed here even though this step is what
/// gives it its final, tool-bearing meaning: it's already emitted by
/// `steps::enterprise` (Story P3), which owns that file's place in the
/// generation pipeline — this step only supplies the modules that file
/// imports (`data.store`, `tools.*`), not the file itself.
const FILES: &[(&str, &str)] = &[
    ("data/__init__.py.tera", "data/__init__.py"),
    ("data/store.py.tera", "data/store.py"),
    ("services/__init__.py.tera", "services/__init__.py"),
    (
        "services/embedding_service.py.tera",
        "services/embedding_service.py",
    ),
    (
        "services/populate_embeddings.py.tera",
        "services/populate_embeddings.py",
    ),
    ("services/api_client.py.tera", "services/api_client.py"),
    ("validation/__init__.py.tera", "validation/__init__.py"),
    ("validation/validator.py.tera", "validation/validator.py"),
    ("tools/__init__.py.tera", "tools/__init__.py"),
    ("tools/search_tool.py.tera", "tools/search_tool.py"),
    ("tools/get_tool.py.tera", "tools/get_tool.py"),
    ("tools/call_tool.py.tera", "tools/call_tool.py"),
];

const GENERATED_SCHEMAS_RELATIVE_PATH: &str = "validation/generated_schemas.json";

/// `generate_mcp_tools` (architecture.md §1, step 9): the data-access
/// layer, embedding/API-client services, validator, and 3 tool modules
/// against `mcp_store.db` and the target-API HTTP client, plus the
/// `generated_schemas.json` asset `validation/validator.py` reads at
/// import time.
pub async fn generate_mcp_tools(ctx: &GeneratorContext) -> Result<()> {
    let view = PyTemplateContext::from_context(ctx);
    let package_root = ctx.output_dir.join("src").join(&view.module_name);
    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in FILES {
        render_and_write(&tera, template, &tera_ctx, &package_root.join(out_name)).await?;
    }

    write_generated_schemas(ctx, &package_root).await?;

    Ok(())
}

/// Built directly with `serde_json` rather than through a Tera loop: the
/// per-operation JSON Schema documents (from `schema_resolve.rs`, already
/// `$ref`-resolved) are genuine data, not boilerplate text, and a
/// hundreds-of-operations spec would make a loop-heavy `.tera` template
/// slow to render and unreadable to maintain — mirrors
/// `targets::rust::steps::tools::write_generated_schemas`.
async fn write_generated_schemas(
    ctx: &GeneratorContext,
    package_root: &std::path::Path,
) -> Result<()> {
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

    let out_path = package_root.join(GENERATED_SCHEMAS_RELATIVE_PATH);
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

    // A named subdirectory (rather than the tempdir root, whose name is
    // random) so `module_name` — and therefore `package_root` below — is
    // deterministic.
    fn output_dir(parent: &tempfile::TempDir) -> PathBuf {
        parent.path().join("output")
    }

    #[tokio::test]
    async fn writes_every_tool_file() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_operations(dir.clone(), vec![sample_operation()]);

        generate_mcp_tools(&ctx).await.unwrap();

        let package_root = dir.join("src").join("output");
        for (_, out_name) in FILES {
            assert!(
                package_root.join(out_name).is_file(),
                "missing src/output/{out_name}"
            );
        }
        assert!(package_root.join(GENERATED_SCHEMAS_RELATIVE_PATH).is_file());
    }

    #[tokio::test]
    async fn generated_schemas_json_round_trips_operation_schemas() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_operations(dir.clone(), vec![sample_operation()]);

        generate_mcp_tools(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(
            dir.join("src")
                .join("output")
                .join(GENERATED_SCHEMAS_RELATIVE_PATH),
        )
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

        let contents = tokio::fs::read_to_string(
            dir.join("src")
                .join("output")
                .join(GENERATED_SCHEMAS_RELATIVE_PATH),
        )
        .await
        .unwrap();
        let parsed: Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed, Value::Object(Map::new()));
    }
}
