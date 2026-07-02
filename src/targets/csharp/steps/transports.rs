use anyhow::Result;

use crate::context::GeneratorContext;

/// Story C5: emits the dual-role `Program.cs` entry point (Terminal Client
/// vs. Harness Server, branched via `System.CommandLine`) and the
/// Kestrel/minimal-API HTTP transport middleware. Stubbed in Story C1
/// pending that story.
pub async fn generate_transports_and_roles(_ctx: &GeneratorContext) -> Result<()> {
    Ok(())
}
