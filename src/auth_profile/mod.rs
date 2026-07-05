//! Auth scheme discovery and profiling (architecture.md §1, step 3).

pub mod classify;
pub mod descriptor;
pub mod prompt;

use anyhow::{Context, Result, bail};
use openapiv3::OpenAPI;

pub use descriptor::{
    AuthSchemeDescriptor, AuthSchemeKind, AuthSchemeLocation, default_location_for, location_view,
};

/// Classifies `components.securitySchemes` into `Vec<AuthSchemeDescriptor>`,
/// falling back to an interactive prompt (REQ-1.2.4) when nothing could be
/// classified. When `interactive` is false, an unclassifiable spec is a hard
/// error instead of blocking on a prompt (e.g. CI/scripted generation runs).
pub async fn profile_auth(doc: &OpenAPI, interactive: bool) -> Result<Vec<AuthSchemeDescriptor>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_without_schemes() -> OpenAPI {
        serde_yaml::from_str(
            r#"
openapi: 3.0.0
info:
  title: Test
  version: "1.0.0"
paths: {}
"#,
        )
        .unwrap()
    }

    fn doc_with_basic_auth() -> OpenAPI {
        serde_yaml::from_str(
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
        .unwrap()
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
        let doc: OpenAPI = serde_yaml::from_str(
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
        )
        .unwrap();

        let err = profile_auth(&doc, false).await.unwrap_err();
        assert!(err.to_string().contains("no usable auth scheme found"));
    }
}
