use anyhow::Result;

use crate::context::GeneratorContext;

/// Story G2: `go.mod`, project skeleton (`cmd/<binary>/`,
/// `internal/{auth,cli,core,data,http,services,tools,validation}/`),
/// `.gitignore`, `README.md`. Stubbed in G1.
pub async fn bootstrap_project(_ctx: &GeneratorContext) -> Result<()> {
    Ok(())
}
