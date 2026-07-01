use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;

use crate::context::GeneratorContext;

/// The blueprint every output-language target must satisfy. Each method
/// corresponds 1:1 to a step of the Compile-Time Lifecycle (architecture.md §1).
#[async_trait]
pub trait McpServerTargetGenerator: Send + Sync {
    #[allow(dead_code)] // ponytail: unused until build_registry() registers a real target (Story 7+)
    fn name(&self) -> &'static str;
    async fn bootstrap_project(&self, ctx: &GeneratorContext) -> Result<()>;
    async fn generate_enterprise_scaffolding(&self, ctx: &GeneratorContext) -> Result<()>;
    async fn generate_auth_strategies(&self, ctx: &GeneratorContext) -> Result<()>;
    async fn generate_transports_and_roles(&self, ctx: &GeneratorContext) -> Result<()>;
    async fn generate_mcp_tools(&self, ctx: &GeneratorContext) -> Result<()>;
    async fn generate_setup_wizard_and_tests(&self, ctx: &GeneratorContext) -> Result<()>;
    /// Installs dependencies and executes the generated project's own test
    /// suite to completion; a run that generates code but whose tests fail
    /// (or don't run) is not a successful `execute()` (PRD REQ-2.5.1).
    async fn run_generated_tests(&self, ctx: &GeneratorContext) -> Result<()>;

    async fn execute(&self, ctx: &GeneratorContext) -> Result<()> {
        let result = async {
            self.bootstrap_project(ctx).await?;
            self.generate_enterprise_scaffolding(ctx).await?;
            self.generate_auth_strategies(ctx).await?;
            self.generate_transports_and_roles(ctx).await?;
            self.generate_mcp_tools(ctx).await?;
            self.generate_setup_wizard_and_tests(ctx).await?;
            self.run_generated_tests(ctx).await
        }
        .await;

        // Roll back a failed run so it doesn't leave a broken, half-generated
        // project blocking the next attempt — but only when mcpify created
        // output_dir fresh. Never delete a pre-existing directory the user
        // pointed --force at.
        if result.is_err() && !ctx.output_dir_preexisted {
            let _ = tokio::fs::remove_dir_all(&ctx.output_dir).await;
        }
        result
    }
}

/// Dispatches on `--language` to the registered target implementation.
pub type TargetRegistry = HashMap<&'static str, Box<dyn McpServerTargetGenerator>>;

/// Target implementations (e.g. `TypeScriptTargetGenerator`) register here
/// as they land; v1 ships only "typescript" (Story 7+).
pub fn build_registry() -> TargetRegistry {
    HashMap::new()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A target whose `bootstrap_project` always fails, used to exercise
    /// `execute()`'s rollback wiring without depending on a real target.
    struct AlwaysFailsAtBootstrap {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl McpServerTargetGenerator for AlwaysFailsAtBootstrap {
        fn name(&self) -> &'static str {
            "always-fails"
        }

        async fn bootstrap_project(&self, _ctx: &GeneratorContext) -> Result<()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("boom")
        }

        async fn generate_enterprise_scaffolding(&self, _ctx: &GeneratorContext) -> Result<()> {
            unreachable!("must not run past a failed bootstrap_project")
        }
        async fn generate_auth_strategies(&self, _ctx: &GeneratorContext) -> Result<()> {
            unreachable!()
        }
        async fn generate_transports_and_roles(&self, _ctx: &GeneratorContext) -> Result<()> {
            unreachable!()
        }
        async fn generate_mcp_tools(&self, _ctx: &GeneratorContext) -> Result<()> {
            unreachable!()
        }
        async fn generate_setup_wizard_and_tests(&self, _ctx: &GeneratorContext) -> Result<()> {
            unreachable!()
        }
        async fn run_generated_tests(&self, _ctx: &GeneratorContext) -> Result<()> {
            unreachable!()
        }
    }

    fn ctx(output_dir: PathBuf, output_dir_preexisted: bool) -> GeneratorContext {
        GeneratorContext {
            openapi_input: "spec.yaml".to_string(),
            output_dir,
            force: output_dir_preexisted,
            output_dir_preexisted,
            auth_schemes: Vec::new(),
            normalized_operations: Vec::new(),
        }
    }

    #[tokio::test]
    async fn execute_stops_at_first_failing_step() {
        let target = AlwaysFailsAtBootstrap {
            calls: AtomicUsize::new(0),
        };
        let dir = tempfile::tempdir().unwrap();
        let result = target.execute(&ctx(dir.path().to_path_buf(), true)).await;
        assert!(result.is_err());
        assert_eq!(target.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn execute_removes_output_dir_on_failure_when_freshly_created() {
        let target = AlwaysFailsAtBootstrap {
            calls: AtomicUsize::new(0),
        };
        let parent = tempfile::tempdir().unwrap();
        let fresh_dir = parent.path().join("generated");
        tokio::fs::create_dir_all(&fresh_dir).await.unwrap();

        target
            .execute(&ctx(fresh_dir.clone(), false))
            .await
            .unwrap_err();

        assert!(!fresh_dir.exists());
    }

    #[tokio::test]
    async fn execute_preserves_output_dir_on_failure_when_preexisted() {
        let target = AlwaysFailsAtBootstrap {
            calls: AtomicUsize::new(0),
        };
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("keep-me.txt");
        tokio::fs::write(&marker, b"partial content").await.unwrap();

        target
            .execute(&ctx(dir.path().to_path_buf(), true))
            .await
            .unwrap_err();

        assert!(marker.exists());
    }

    #[test]
    fn build_registry_starts_empty() {
        assert!(build_registry().is_empty());
    }
}
