use anyhow::Result;

use crate::context::GeneratorContext;

/// Story C7: emits the interactive `Spectre.Console` setup wizard and the
/// generated test suite (xUnit; open decision #3), conditionally emitting
/// auth-strategy tests only for discovered schemes. Stubbed in Story C1
/// pending that story.
pub async fn generate_setup_wizard_and_tests(_ctx: &GeneratorContext) -> Result<()> {
    Ok(())
}
