use anyhow::Result;

use crate::context::GeneratorContext;

/// Story G4: the 5 auth strategies (Basic, PAT, OAuth1 RSA-SHA1/HMAC-SHA1,
/// OAuth2 PKCE+refresh, stub) expressed as a Go `AuthStrategy` interface,
/// plus `authmanager`'s `map[string]AuthStrategy` dispatch. Stubbed in G1.
pub async fn generate_auth_strategies(_ctx: &GeneratorContext) -> Result<()> {
    Ok(())
}
