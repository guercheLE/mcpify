pub mod context;
pub mod emit;
pub mod naming;
pub mod render;
pub mod steps;

use anyhow::Result;
use async_trait::async_trait;

use crate::context::GeneratorContext;
use crate::targets::McpServerTargetGenerator;

/// The v2 output target (`-l rust`; docs/v2-implementation-plan.md). Mirrors
/// `TypeScriptTargetGenerator`'s structure: each method corresponds to one
/// step of the per-target lifecycle (architecture.md §1, steps 5-11).
/// Stubbed out per Story R1; each stub is replaced with a real
/// `steps::*` call as its story (R2-R7) lands — R2 (`bootstrap_project`),
/// R3 (`generate_enterprise_scaffolding`), and R4
/// (`generate_auth_strategies`) are done, R5-R7 remain stubbed. Deliberately
/// **not**
/// registered in `targets::build_registry()` yet — Story R8 registers it
/// only once `run_generated_tests` is real and green, the same
/// "don't register a target whose tests can't actually prove anything"
/// discipline v1 followed for `TypeScriptTargetGenerator`.
#[derive(Debug, Default)]
pub struct RustTargetGenerator;

#[async_trait]
impl McpServerTargetGenerator for RustTargetGenerator {
    fn name(&self) -> &'static str {
        "rust"
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
        anyhow::bail!("rust target: generate_transports_and_roles not yet implemented (Story R5)")
    }

    async fn generate_mcp_tools(&self, _ctx: &GeneratorContext) -> Result<()> {
        anyhow::bail!("rust target: generate_mcp_tools not yet implemented (Story R6)")
    }

    async fn generate_setup_wizard_and_tests(&self, _ctx: &GeneratorContext) -> Result<()> {
        anyhow::bail!("rust target: generate_setup_wizard_and_tests not yet implemented (Story R7)")
    }

    async fn run_generated_tests(&self, _ctx: &GeneratorContext) -> Result<()> {
        anyhow::bail!("rust target: run_generated_tests not yet implemented (Story R8)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_its_name() {
        assert_eq!(RustTargetGenerator.name(), "rust");
    }
}
