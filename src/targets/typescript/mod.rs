pub mod context;
pub mod emit;
pub mod naming;
pub mod render;
pub mod steps;

use anyhow::Result;
use async_trait::async_trait;

use crate::context::GeneratorContext;
use crate::targets::McpServerTargetGenerator;

/// The v1 output target (PRD REQ-1.1.4/1.1.5). Each method below corresponds
/// to one step of the per-target lifecycle (architecture.md §1, steps 5-11);
/// they're stubbed here so the trait compiles end to end, and filled in by
/// Stories 8-14 in that order — enterprise scaffolding is generated before
/// any tool-specific code, per architecture.md's explicit ordering rationale.
///
/// Deliberately NOT registered in `targets::build_registry()` yet: a stub
/// that returns `Ok(())` for every step would make `mcpify -l typescript`
/// silently "succeed" while generating nothing, which would violate the
/// zero-placeholder quality bar (PRD REQ-2.5.1). It gets registered once
/// `run_generated_tests` (Story 14) is real enough for a green run to mean
/// something.
#[derive(Debug, Default)]
pub struct TypeScriptTargetGenerator;

#[async_trait]
impl McpServerTargetGenerator for TypeScriptTargetGenerator {
    fn name(&self) -> &'static str {
        "typescript"
    }

    async fn bootstrap_project(&self, ctx: &GeneratorContext) -> Result<()> {
        steps::bootstrap::bootstrap_project(ctx).await
    }

    async fn generate_enterprise_scaffolding(&self, ctx: &GeneratorContext) -> Result<()> {
        steps::enterprise::generate_enterprise_scaffolding(ctx).await
    }

    async fn generate_auth_strategies(&self, ctx: &GeneratorContext) -> Result<()> {
        steps::auth::generate_auth_strategies(ctx).await
    }

    async fn generate_transports_and_roles(&self, _ctx: &GeneratorContext) -> Result<()> {
        Ok(())
    }

    async fn generate_mcp_tools(&self, _ctx: &GeneratorContext) -> Result<()> {
        Ok(())
    }

    async fn generate_setup_wizard_and_tests(&self, _ctx: &GeneratorContext) -> Result<()> {
        Ok(())
    }

    async fn run_generated_tests(&self, _ctx: &GeneratorContext) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_its_name() {
        assert_eq!(TypeScriptTargetGenerator.name(), "typescript");
    }
}
