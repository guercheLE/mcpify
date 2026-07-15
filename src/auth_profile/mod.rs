//! Auth scheme discovery and profiling (architecture.md §1, step 3).

pub mod classify;
pub mod descriptor;
pub mod prompt;

use crate::openapi::parse::OpenApiDocument;
use crate::project_config::{AuthOverride, AuthOverrideKind, AuthOverrideLocation};
use anyhow::{Context, Result, bail};

pub use descriptor::{
    AuthSchemeDescriptor, AuthSchemeKind, AuthSchemeLocation, default_location_for, location_view,
};

/// Classifies `components.securitySchemes` into `Vec<AuthSchemeDescriptor>`,
/// falling back to an interactive prompt (REQ-1.2.4) when nothing could be
/// classified. When `interactive` is false, an unclassifiable spec is a hard
/// error instead of blocking on a prompt (e.g. CI/scripted generation runs).
pub async fn profile_auth(
    doc: &OpenApiDocument,
    interactive: bool,
) -> Result<Vec<AuthSchemeDescriptor>> {
    let schemes = classify::classify_schemes(doc);
    if !schemes.is_empty() {
        return Ok(schemes);
    }

    if !interactive {
        bail!(
            "no usable auth scheme found in components.securitySchemes, and interactive prompting is disabled"
        );
    }

    let descriptor = tokio::task::spawn_blocking(prompt::prompt_for_scheme)
        .await
        .context("auth-scheme prompt task panicked")??;
    Ok(vec![descriptor])
}

/// Profiles auth declared by the OpenAPI document and then appends explicit
/// project-manifest schemes. This deliberately supplements rather than
/// replaces the spec so incomplete enterprise documents can declare Basic
/// while the project adds a PAT or API-key mode known to work in production.
pub async fn profile_auth_with_overrides(
    doc: &OpenApiDocument,
    interactive: bool,
    overrides: &[AuthOverride],
) -> Result<Vec<AuthSchemeDescriptor>> {
    let mut schemes = classify::classify_schemes(doc);
    for item in overrides {
        let kind = match item.kind {
            AuthOverrideKind::Basic => AuthSchemeKind::Basic,
            AuthOverrideKind::ApiKey => AuthSchemeKind::ApiKey,
            AuthOverrideKind::Pat => AuthSchemeKind::BearerPat,
            AuthOverrideKind::OAuth1 => AuthSchemeKind::OAuth1,
            AuthOverrideKind::OAuth2 => AuthSchemeKind::OAuth2,
        };
        let location = match (&item.location, &item.parameter_name) {
            (Some(AuthOverrideLocation::Header), Some(name)) => {
                Some(AuthSchemeLocation::Header { name: name.clone() })
            }
            (Some(AuthOverrideLocation::Query), Some(name)) => {
                Some(AuthSchemeLocation::Query { name: name.clone() })
            }
            (Some(AuthOverrideLocation::Cookie), Some(name)) => {
                Some(AuthSchemeLocation::Cookie { name: name.clone() })
            }
            _ => default_location_for(kind),
        };
        let descriptor = AuthSchemeDescriptor {
            name: item.name.clone(),
            kind,
            location,
        };
        if let Some(existing) = schemes.iter_mut().find(|scheme| scheme.name == item.name) {
            *existing = descriptor;
        } else {
            schemes.push(descriptor);
        }
    }

    if !schemes.is_empty() {
        return Ok(schemes);
    }
    profile_auth(doc, interactive).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> OpenApiDocument {
        crate::openapi::parse::parse_document(yaml, Some(crate::openapi::parse::Format::Yaml))
            .unwrap()
    }

    fn doc_without_schemes() -> OpenApiDocument {
        parse(
            r#"
openapi: 3.0.0
info:
  title: Test
  version: "1.0.0"
paths: {}
"#,
        )
    }

    fn doc_with_basic_auth() -> OpenApiDocument {
        parse(
            r#"
openapi: 3.0.0
info:
  title: Test
  version: "1.0.0"
paths: {}
components:
  securitySchemes:
    basicAuth:
      type: http
      scheme: basic
"#,
        )
    }

    #[tokio::test]
    async fn returns_classified_schemes_without_prompting() {
        let schemes = profile_auth(&doc_with_basic_auth(), false).await.unwrap();
        assert_eq!(schemes.len(), 1);
        assert_eq!(schemes[0].kind, AuthSchemeKind::Basic);
    }

    #[tokio::test]
    async fn empty_security_schemes_errors_when_non_interactive() {
        let err = profile_auth(&doc_without_schemes(), false)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no usable auth scheme found"));
    }

    #[tokio::test]
    async fn ambiguous_scheme_errors_when_non_interactive() {
        let doc = parse(
            r#"
openapi: 3.0.0
info:
  title: Test
  version: "1.0.0"
paths: {}
components:
  securitySchemes:
    oidc:
      type: openIdConnect
      openIdConnectUrl: https://example.com/.well-known/openid-configuration
"#,
        );

        let err = profile_auth(&doc, false).await.unwrap_err();
        assert!(err.to_string().contains("no usable auth scheme found"));
    }
}
