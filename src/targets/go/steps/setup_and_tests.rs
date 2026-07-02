use anyhow::Result;

use crate::context::GeneratorContext;

/// Story G7: the interactive setup wizard via `AlecAivazis/survey`, plus the
/// generated `go test` suite (table-driven unit tests, build-tag-gated
/// integration tests). Stubbed in G1.
pub async fn generate_setup_wizard_and_tests(_ctx: &GeneratorContext) -> Result<()> {
    Ok(())
}
