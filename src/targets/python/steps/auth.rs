use anyhow::Result;

use crate::context::GeneratorContext;

/// Story P4: the 5 auth strategies (Basic, PAT, OAuth1 RSA-SHA1, OAuth2
/// PKCE+refresh, stub) plus `auth_manager.py`'s dispatch dict. Stubbed in
/// P1.
pub async fn generate_auth_strategies(_ctx: &GeneratorContext) -> Result<()> {
    Ok(())
}
