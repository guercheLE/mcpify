use anyhow::Result;

use crate::context::GeneratorContext;

/// Story G5: the dual-role entry point via `spf13/cobra` (Terminal Client
/// vs. Harness Server subcommands), plus the `net/http`-based HTTP
/// transport and its middleware chain. Stubbed in G1.
pub async fn generate_transports_and_roles(_ctx: &GeneratorContext) -> Result<()> {
    Ok(())
}
