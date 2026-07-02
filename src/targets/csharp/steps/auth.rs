use anyhow::Result;

use crate::context::GeneratorContext;

/// Story C4: emits the 5 auth strategies as an `IAuthStrategy` interface
/// plus DI-registered implementations, with the auth-manager resolving the
/// active one via a keyed-service lookup. Stubbed in Story C1 pending that
/// story.
pub async fn generate_auth_strategies(_ctx: &GeneratorContext) -> Result<()> {
    Ok(())
}
