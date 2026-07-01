pub mod dir_guard;

use std::path::PathBuf;

use anyhow::Result;

use crate::context::GeneratorContext;
use crate::openapi::normalize::normalize_operations;
use crate::{auth_profile, db, openapi};

/// Glues the shared, target-independent steps of the compile-time lifecycle
/// (architecture.md §1, steps 1-4) into the fully populated
/// `GeneratorContext` every `McpServerTargetGenerator` method then receives:
/// ingest & parse -> directory guard -> auth profiling -> operation
/// normalization -> `mcp_store.db` assembly, run once regardless of
/// `--language`.
pub async fn run_shared_pipeline(
    input: &str,
    output_dir: PathBuf,
    force: bool,
    interactive_auth_prompt: bool,
) -> Result<GeneratorContext> {
    let doc = openapi::ingest(input).await?;
    let output_dir_preexisted = dir_guard::check_output_dir(&output_dir, force).await?;
    let auth_schemes = auth_profile::profile_auth(&doc, interactive_auth_prompt).await?;
    let normalized_operations = normalize_operations(&doc);

    let ctx = GeneratorContext {
        openapi_input: input.to_string(),
        output_dir,
        force,
        output_dir_preexisted,
        auth_schemes,
        normalized_operations,
        api_title: doc.info.title.clone(),
    };

    db::assemble_store(&ctx).await?;

    Ok(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn assembles_a_full_context_from_a_fixture_spec() {
        let parent = tempfile::tempdir().unwrap();
        let output_dir = parent.path().join("generated"); // does not exist yet

        let ctx = run_shared_pipeline(
            "tests/fixtures/openapi/minimal-with-auth.yaml",
            output_dir.clone(),
            false,
            false,
        )
        .await
        .unwrap();

        assert_eq!(ctx.auth_schemes.len(), 1);
        assert!(!ctx.output_dir_preexisted);
        assert_eq!(ctx.normalized_operations.len(), 1);
        assert_eq!(ctx.normalized_operations[0].operation_id, "ping");
        assert!(output_dir.join("mcp_store.db").exists());
    }

    #[tokio::test]
    async fn propagates_ingest_failure_without_creating_output_dir() {
        let parent = tempfile::tempdir().unwrap();
        let output_dir = parent.path().join("generated");

        let err = run_shared_pipeline(
            "tests/fixtures/openapi/does-not-exist.yaml",
            output_dir.clone(),
            false,
            false,
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("failed to read OpenAPI spec"));
        assert!(!output_dir.exists());
    }
}
