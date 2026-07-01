//! Integration test combining the real directory guard (Story 3) with
//! `execute()`'s rollback semantics (Story 1), rather than hand-constructing
//! `output_dir_preexisted` as the in-crate unit tests do.

use async_trait::async_trait;
use mcpify::context::GeneratorContext;
use mcpify::pipeline::dir_guard::check_output_dir;
use mcpify::targets::McpServerTargetGenerator;

struct AlwaysFails;

#[async_trait]
impl McpServerTargetGenerator for AlwaysFails {
    fn name(&self) -> &'static str {
        "always-fails"
    }

    async fn bootstrap_project(&self, _ctx: &GeneratorContext) -> anyhow::Result<()> {
        anyhow::bail!("boom")
    }
    async fn generate_enterprise_scaffolding(&self, _ctx: &GeneratorContext) -> anyhow::Result<()> {
        unreachable!()
    }
    async fn generate_auth_strategies(&self, _ctx: &GeneratorContext) -> anyhow::Result<()> {
        unreachable!()
    }
    async fn generate_transports_and_roles(&self, _ctx: &GeneratorContext) -> anyhow::Result<()> {
        unreachable!()
    }
    async fn generate_mcp_tools(&self, _ctx: &GeneratorContext) -> anyhow::Result<()> {
        unreachable!()
    }
    async fn generate_setup_wizard_and_tests(&self, _ctx: &GeneratorContext) -> anyhow::Result<()> {
        unreachable!()
    }
    async fn run_generated_tests(&self, _ctx: &GeneratorContext) -> anyhow::Result<()> {
        unreachable!()
    }
}

#[tokio::test]
async fn fresh_dir_created_by_dir_guard_is_removed_on_failure() {
    let parent = tempfile::tempdir().unwrap();
    let output_dir = parent.path().join("generated");

    let output_dir_preexisted = check_output_dir(&output_dir, false).await.unwrap();
    assert!(
        !output_dir_preexisted,
        "a newly created dir must report as not preexisted"
    );
    assert!(output_dir.is_dir());

    let ctx = GeneratorContext {
        openapi_input: "spec.yaml".to_string(),
        output_dir: output_dir.clone(),
        force: false,
        output_dir_preexisted,
        auth_schemes: Vec::new(),
        normalized_operations: Vec::new(),
    };

    AlwaysFails.execute(&ctx).await.unwrap_err();

    assert!(
        !output_dir.exists(),
        "a freshly created output dir must be rolled back on failure"
    );
}

#[tokio::test]
async fn forced_preexisting_dir_survives_failure_with_partial_content_intact() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("keep-me.txt");
    tokio::fs::write(&marker, b"partial content from a previous run")
        .await
        .unwrap();

    let output_dir_preexisted = check_output_dir(dir.path(), true).await.unwrap();
    assert!(
        output_dir_preexisted,
        "a non-empty dir passed with --force must report as preexisted"
    );

    let ctx = GeneratorContext {
        openapi_input: "spec.yaml".to_string(),
        output_dir: dir.path().to_path_buf(),
        force: true,
        output_dir_preexisted,
        auth_schemes: Vec::new(),
        normalized_operations: Vec::new(),
    };

    AlwaysFails.execute(&ctx).await.unwrap_err();

    assert!(
        marker.exists(),
        "a pre-existing --force dir must never be deleted on failure"
    );
}
