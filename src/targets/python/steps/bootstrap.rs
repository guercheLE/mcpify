use anyhow::Result;

use crate::context::GeneratorContext;

/// Story P2: `pyproject.toml`, project skeleton, `.gitignore`, `README.md`.
/// Stubbed in P1.
pub async fn bootstrap_project(_ctx: &GeneratorContext) -> Result<()> {
    Ok(())
}
