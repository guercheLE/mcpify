use anyhow::Result;

use crate::context::GeneratorContext;

/// Story P8 (the v3 launch milestone): shell out to the chosen packaging
/// tool's install step, then `pytest`. `PythonTargetGenerator` is only
/// registered in `targets::build_registry()` once this is real and green.
/// Stubbed in P1.
pub async fn run_generated_tests(_ctx: &GeneratorContext) -> Result<()> {
    Ok(())
}
