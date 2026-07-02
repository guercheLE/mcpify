pub mod context;
pub mod emit;
pub mod naming;
pub mod render;
pub mod steps;

use anyhow::Result;
use async_trait::async_trait;

use crate::context::GeneratorContext;
use crate::targets::McpServerTargetGenerator;

/// The v5 output target (`-l go`; docs/v5-implementation-plan.md). Mirrors
/// `CSharpTargetGenerator`'s structure: each method corresponds to one step
/// of the per-target lifecycle (architecture.md §1, steps 5-11). Story G1
/// stubs all 7 methods; not yet registered in `targets::build_registry()` —
/// that happens in Story G8, the v5 launch milestone, once
/// `run_generated_tests` is real and green.
#[derive(Debug, Default)]
pub struct GoTargetGenerator;

#[async_trait]
impl McpServerTargetGenerator for GoTargetGenerator {
    fn name(&self) -> &'static str {
        "go"
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

    async fn generate_transports_and_roles(&self, ctx: &GeneratorContext) -> Result<()> {
        steps::transports::generate_transports_and_roles(ctx).await
    }

    async fn generate_mcp_tools(&self, ctx: &GeneratorContext) -> Result<()> {
        steps::tools::generate_mcp_tools(ctx).await
    }

    async fn generate_setup_wizard_and_tests(&self, ctx: &GeneratorContext) -> Result<()> {
        steps::setup_and_tests::generate_setup_wizard_and_tests(ctx).await
    }

    async fn run_generated_tests(&self, ctx: &GeneratorContext) -> Result<()> {
        steps::run_tests::run_generated_tests(ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_its_name() {
        assert_eq!(GoTargetGenerator.name(), "go");
    }
}
