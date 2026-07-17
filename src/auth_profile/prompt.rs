use anyhow::Result;
use inquire::Select;

use super::descriptor::{AuthSchemeDescriptor, AuthSchemeKind, default_location_for};

const CHOICES: &[(&str, AuthSchemeKind)] = &[
    ("Basic (username/password)", AuthSchemeKind::Basic),
    ("API Key (header/query/cookie)", AuthSchemeKind::ApiKey),
    (
        "Bearer token / Personal Access Token",
        AuthSchemeKind::BearerPat,
    ),
    ("OAuth 1.0a", AuthSchemeKind::OAuth1),
    ("OAuth 2.0", AuthSchemeKind::OAuth2),
];

/// REQ-1.2.4: when `components.securitySchemes` is missing or ambiguous,
/// ask the operator to pick the auth mechanism rather than silently
/// guessing. Blocking — callers on an async runtime should run this via
/// `tokio::task::spawn_blocking`.
pub fn prompt_for_scheme() -> Result<AuthSchemeDescriptor> {
    let labels: Vec<&str> = CHOICES.iter().map(|(label, _)| *label).collect();
    let selection = Select::new(
        "No usable auth scheme was found in the OpenAPI spec. Which auth mechanism does this API use?",
        labels,
    )
    .prompt()?;

    let kind = CHOICES
        .iter()
        .find(|(label, _)| *label == selection)
        .map(|(_, kind)| *kind)
        .expect("selection must be one of the offered choices");

    Ok(AuthSchemeDescriptor {
        name: "prompted".to_string(),
        kind,
        location: default_location_for(kind),
        // No spec was found to read `flows.*.scopes` from at all — that's
        // the whole reason this fallback prompt exists.
        scopes: Vec::new(),
        authorization_url: None,
        token_url: None,
    })
}
