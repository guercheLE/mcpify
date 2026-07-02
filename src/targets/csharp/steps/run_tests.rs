use anyhow::Result;

use crate::context::GeneratorContext;

/// Story C8: shells out to `dotnet restore` (if not folded into build)
/// then `dotnet test`, which itself compiles the generated project as a
/// prerequisite — the "one signal proves both build and functional
/// correctness" principle every other target's `run_generated_tests`
/// follows. Stubbed in Story C1 pending that story.
pub async fn run_generated_tests(_ctx: &GeneratorContext) -> Result<()> {
    Ok(())
}
