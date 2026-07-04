pub mod dir_guard;

use std::path::PathBuf;

use anyhow::Result;
use openapiv3::OpenAPI;

use crate::context::GeneratorContext;
use crate::openapi::normalize::normalize_operations;
use crate::{auth_profile, db, openapi};

/// Glues the shared, target-independent steps of the compile-time lifecycle
/// (architecture.md §1, steps 1-4) into the fully populated
/// `GeneratorContext` every `McpServerTargetGenerator` method then receives:
/// ingest & parse -> directory guard -> auth profiling -> operation
/// normalization -> `mcp_store.db` assembly, run once regardless of
/// `--language`.
///
/// Mirrors `execute()`'s rollback semantics (architecture.md's default
/// `execute` body): once the directory guard has run, any later failure in
/// this same shared pipeline removes a freshly-created `output_dir` before
/// returning the error, so a spec that fails auth profiling (for example)
/// doesn't leave an empty directory behind for the next attempt to trip
/// over. A pre-existing (`--force`) directory is never touched.
#[allow(clippy::too_many_arguments)]
pub async fn run_shared_pipeline(
    input: &str,
    output_dir: PathBuf,
    force: bool,
    interactive_auth_prompt: bool,
    publish_registry: bool,
    version_label: &str,
) -> Result<GeneratorContext> {
    let doc = openapi::ingest(input).await?;
    let output_dir_preexisted = dir_guard::check_output_dir(&output_dir, force).await?;

    let result = assemble_context(
        input,
        output_dir.clone(),
        force,
        output_dir_preexisted,
        interactive_auth_prompt,
        publish_registry,
        version_label,
        &doc,
    )
    .await;

    if result.is_err() && !output_dir_preexisted {
        let _ = tokio::fs::remove_dir_all(&output_dir).await;
    }

    result
}

#[allow(clippy::too_many_arguments)]
async fn assemble_context(
    input: &str,
    output_dir: PathBuf,
    force: bool,
    output_dir_preexisted: bool,
    interactive_auth_prompt: bool,
    publish_registry: bool,
    version_label: &str,
    doc: &OpenAPI,
) -> Result<GeneratorContext> {
    let auth_schemes = auth_profile::profile_auth(doc, interactive_auth_prompt).await?;
    let normalized_operations = normalize_operations(doc);

    let ctx = GeneratorContext {
        publish_registry,
        openapi_input: input.to_string(),
        output_dir,
        force,
        output_dir_preexisted,
        auth_schemes,
        normalized_operations,
        api_title: doc.info.title.clone(),
        version_label: version_label.to_string(),
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
            false,
            "default",
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
            false,
            "default",
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("failed to read OpenAPI spec"));
        assert!(!output_dir.exists());
    }

    #[tokio::test]
    async fn rolls_back_a_freshly_created_dir_when_auth_profiling_fails() {
        let parent = tempfile::tempdir().unwrap();
        let output_dir = parent.path().join("generated");

        let err = run_shared_pipeline(
            "tests/fixtures/openapi/minimal-no-auth-scheme.json",
            output_dir.clone(),
            false,
            false,
            false,
            "default",
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("no usable auth scheme found"));
        assert!(!output_dir.exists());
    }

    #[tokio::test]
    async fn preserves_a_preexisting_dir_when_auth_profiling_fails() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("keep-me.txt");
        tokio::fs::write(&marker, b"partial content from a previous run")
            .await
            .unwrap();

        run_shared_pipeline(
            "tests/fixtures/openapi/minimal-no-auth-scheme.json",
            dir.path().to_path_buf(),
            true,
            false,
            false,
            "default",
        )
        .await
        .unwrap_err();

        assert!(marker.exists());
    }
}
