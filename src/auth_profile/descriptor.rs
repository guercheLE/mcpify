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

/// Where a scheme's credential value travels on the wire — the OpenAPI
/// `in`/`name` pair for `apiKey` schemes, or the implied `Authorization`
/// header for `http`/`oauth2` schemes. `None` for OAuth1 (its header can't
/// be relayed as-is; see AuthSchemeKind::OAuth1's HTTP-transport handling).
#[derive(Debug, Clone, PartialEq)]
pub enum AuthSchemeLocation {
    Header { name: String },
    Query { name: String },
    Cookie { name: String },
}

impl AuthSchemeLocation {
    /// The literal string each target's template context serializes as
    /// `header_location` — `"header"` is the only one an HTTP-transport
    /// per-request header relay can act on (§4/§5 of the plan); `"query"`
    /// and `"cookie"` are surfaced too so templates can render an accurate
    /// diagnostic instead of silently doing nothing.
    pub fn kind_str(&self) -> &'static str {
        match self {
            AuthSchemeLocation::Header { .. } => "header",
            AuthSchemeLocation::Query { .. } => "query",
            AuthSchemeLocation::Cookie { .. } => "cookie",
        }
    }

    pub fn name(&self) -> &str {
        match self {
            AuthSchemeLocation::Header { name }
            | AuthSchemeLocation::Query { name }
            | AuthSchemeLocation::Cookie { name } => name,
        }
    }
}

/// `(header_location, header_name)` template-context pair for a scheme's
/// optional location — `"none"`/`""` for OAuth1, which has no relayable
/// location at all.
pub fn location_view(location: &Option<AuthSchemeLocation>) -> (&'static str, String) {
    match location {
        Some(loc) => (loc.kind_str(), loc.name().to_string()),
        None => ("none", String::new()),
    }
}

/// One discovered (or operator-provided) auth mechanism, keyed by its name in
/// `components.securitySchemes` — or a synthetic name when it came from the
/// interactive fallback prompt.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthSchemeDescriptor {
    pub name: String,
    pub kind: AuthSchemeKind,
    pub location: Option<AuthSchemeLocation>,
}

/// The location a scheme uses when the spec doesn't say (or wasn't
/// consulted, e.g. the interactive fallback prompt) — `Authorization`
/// header for anything bearer-shaped, `X-Api-Key` for a generic API key,
/// and no location at all for OAuth1 (its header isn't relayable as-is).
pub fn default_location_for(kind: AuthSchemeKind) -> Option<AuthSchemeLocation> {
    match kind {
        AuthSchemeKind::Basic | AuthSchemeKind::BearerPat | AuthSchemeKind::OAuth2 => {
            Some(AuthSchemeLocation::Header {
                name: "Authorization".to_string(),
            })
        }
        AuthSchemeKind::ApiKey => Some(AuthSchemeLocation::Header {
            name: "X-Api-Key".to_string(),
        }),
        AuthSchemeKind::OAuth1 => None,
    }
}
