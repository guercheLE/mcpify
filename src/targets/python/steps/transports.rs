use anyhow::Result;

use crate::context::GeneratorContext;

/// Story P5: the dual-role CLI entry point (Terminal Client / Harness
/// Server) and the `fastapi`+`uvicorn` HTTP transport. Stubbed in P1.
pub async fn generate_transports_and_roles(_ctx: &GeneratorContext) -> Result<()> {
    Ok(())
}
