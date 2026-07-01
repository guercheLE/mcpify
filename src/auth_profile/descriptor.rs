/// Auth mechanism family a discovered security scheme maps onto. Mirrors the
/// 4-way `AuthMethod` discriminant proven in production by bitbucket-dc-mcp
/// and jira-dc-mcp (`'basic' | 'oauth2' | 'oauth1' | 'pat'`), plus a generic
/// `ApiKey` kind for schemes that are neither bearer tokens nor OAuth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSchemeKind {
    Basic,
    ApiKey,
    BearerPat,
    OAuth2,
    OAuth1,
}

/// One discovered (or operator-provided) auth mechanism, keyed by its name in
/// `components.securitySchemes` — or a synthetic name when it came from the
/// interactive fallback prompt.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthSchemeDescriptor {
    pub name: String,
    pub kind: AuthSchemeKind,
}
