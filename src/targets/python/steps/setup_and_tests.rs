use anyhow::Result;

use crate::context::GeneratorContext;

/// Story P7: the interactive setup wizard and the generated `pytest` suite
/// (conditionally emitting auth-strategy tests only for discovered
/// schemes). Stubbed in P1.
pub async fn generate_setup_wizard_and_tests(_ctx: &GeneratorContext) -> Result<()> {
    Ok(())
}
