use serde_json::{Map, Value};

use crate::openapi::parse::OpenApiDocument;

use super::descriptor::{
    AuthSchemeDescriptor, AuthSchemeKind, AuthSchemeLocation, default_location_for,
};

/// Turns `components.securitySchemes` into `Vec<AuthSchemeDescriptor>`
/// (architecture.md §1, step 3). `$ref`-based scheme entries are skipped —
/// mcpify only classifies inline scheme definitions.
pub fn classify_schemes(doc: &OpenApiDocument) -> Vec<AuthSchemeDescriptor> {
    let Some(schemes) = doc
        .raw()
        .pointer("/components/securitySchemes")
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };

    schemes
        .iter()
        .filter_map(|(name, scheme)| {
            let scheme = scheme.as_object()?;
            classify_one(scheme).map(|kind| AuthSchemeDescriptor {
                name: name.clone(),
                kind,
                location: location_for(scheme, kind),
            })
        })
        .collect()
}

/// Where this scheme's credential value travels on the wire. `apiKey`
/// schemes carry their own declared `in`/`name`; everything else falls back
/// to the same default an operator-selected (interactive-prompt) scheme
/// would get, since `http`/`oauth2`/`oauth1` have no per-spec location to
/// read (OAuth1's vendor extension can attach to either an `apiKey` or an
/// `http` scheme shape, but OAuth1 itself has no relayable HTTP location
/// regardless of which shape carried the hint).
fn location_for(scheme: &Map<String, Value>, kind: AuthSchemeKind) -> Option<AuthSchemeLocation> {
    if kind == AuthSchemeKind::OAuth1 {
        return None;
    }
    if scheme.get("type").and_then(Value::as_str) == Some("apiKey") {
        let name = scheme.get("name").and_then(Value::as_str)?.to_string();
        return match scheme.get("in").and_then(Value::as_str) {
            Some("header") => Some(AuthSchemeLocation::Header { name }),
            Some("query") => Some(AuthSchemeLocation::Query { name }),
            Some("cookie") => Some(AuthSchemeLocation::Cookie { name }),
            _ => None,
        };
    }
    default_location_for(kind)
}

/// OpenAPI 3 has no native OAuth1 scheme `type`, so OAuth1 is detected via
/// the vendor extension `x-auth-type: oauth1` on any scheme shape — a real
/// ambiguity source, since a spec author could omit or misspell this hint.
fn has_oauth1_extension(scheme: &Map<String, Value>) -> bool {
    scheme
        .get("x-auth-type")
        .and_then(|value| value.as_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("oauth1"))
}

fn classify_one(scheme: &Map<String, Value>) -> Option<AuthSchemeKind> {
    match scheme.get("type").and_then(Value::as_str)? {
        "apiKey" => Some(if has_oauth1_extension(scheme) {
            AuthSchemeKind::OAuth1
        } else {
            AuthSchemeKind::ApiKey
        }),
        "http" => {
            if has_oauth1_extension(scheme) {
                return Some(AuthSchemeKind::OAuth1);
            }
            match scheme
                .get("scheme")
                .and_then(Value::as_str)?
                .to_ascii_lowercase()
                .as_str()
            {
                "basic" => Some(AuthSchemeKind::Basic),
                // ponytail: both reference servers only ever implement a
                // PAT-style bearer strategy (no separate generic-bearer
                // kind), so any http/bearer scheme maps straight to
                // BearerPat; add a Bearer variant if a future spec needs one.
                "bearer" => Some(AuthSchemeKind::BearerPat),
                _ => None,
            }
        }
        "oauth2" => Some(AuthSchemeKind::OAuth2),
        "openIdConnect" => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with_scheme(scheme_yaml: &str) -> OpenApiDocument {
        let yaml = format!(
            r#"
openapi: 3.0.0
info:
  title: Test
  version: "1.0.0"
paths: {{}}
components:
  securitySchemes:
    testScheme:
{scheme_yaml}
"#
        );
        crate::openapi::parse::parse_document(&yaml, Some(crate::openapi::parse::Format::Yaml))
            .expect("fixture must parse as OpenAPI")
    }

    #[test]
    fn classifies_basic_auth() {
        let doc = doc_with_scheme("      type: http\n      scheme: basic\n");
        let schemes = classify_schemes(&doc);
        assert_eq!(schemes.len(), 1);
        assert_eq!(schemes[0].kind, AuthSchemeKind::Basic);
        assert_eq!(
            schemes[0].location,
            Some(AuthSchemeLocation::Header {
                name: "Authorization".to_string()
            })
        );
    }

    #[test]
    fn classifies_bearer_as_pat() {
        let doc =
            doc_with_scheme("      type: http\n      scheme: bearer\n      bearerFormat: PAT\n");
        let schemes = classify_schemes(&doc);
        assert_eq!(schemes[0].kind, AuthSchemeKind::BearerPat);
        assert_eq!(
            schemes[0].location,
            Some(AuthSchemeLocation::Header {
                name: "Authorization".to_string()
            })
        );
    }

    #[test]
    fn classifies_api_key_in_header() {
        let doc = doc_with_scheme("      type: apiKey\n      in: header\n      name: X-Api-Key\n");
        let schemes = classify_schemes(&doc);
        assert_eq!(schemes[0].kind, AuthSchemeKind::ApiKey);
        assert_eq!(
            schemes[0].location,
            Some(AuthSchemeLocation::Header {
                name: "X-Api-Key".to_string()
            })
        );
    }

    #[test]
    fn classifies_api_key_in_query() {
        let doc = doc_with_scheme("      type: apiKey\n      in: query\n      name: api_key\n");
        let schemes = classify_schemes(&doc);
        assert_eq!(schemes[0].kind, AuthSchemeKind::ApiKey);
        assert_eq!(
            schemes[0].location,
            Some(AuthSchemeLocation::Query {
                name: "api_key".to_string()
            })
        );
    }

    #[test]
    fn classifies_api_key_in_cookie() {
        let doc = doc_with_scheme("      type: apiKey\n      in: cookie\n      name: session\n");
        let schemes = classify_schemes(&doc);
        assert_eq!(schemes[0].kind, AuthSchemeKind::ApiKey);
        assert_eq!(
            schemes[0].location,
            Some(AuthSchemeLocation::Cookie {
                name: "session".to_string()
            })
        );
    }

    #[test]
    fn classifies_oauth2() {
        let doc = doc_with_scheme(
            "      type: oauth2\n      flows:\n        clientCredentials:\n          tokenUrl: https://example.com/token\n          scopes: {}\n",
        );
        let schemes = classify_schemes(&doc);
        assert_eq!(schemes[0].kind, AuthSchemeKind::OAuth2);
        assert_eq!(
            schemes[0].location,
            Some(AuthSchemeLocation::Header {
                name: "Authorization".to_string()
            })
        );
    }

    #[test]
    fn classifies_oauth1_via_vendor_extension_on_api_key() {
        let doc = doc_with_scheme(
            "      type: apiKey\n      in: header\n      name: Authorization\n      x-auth-type: oauth1\n",
        );
        let schemes = classify_schemes(&doc);
        assert_eq!(schemes[0].kind, AuthSchemeKind::OAuth1);
        assert_eq!(schemes[0].location, None);
    }

    #[test]
    fn classifies_oauth1_via_vendor_extension_on_http() {
        let doc =
            doc_with_scheme("      type: http\n      scheme: oauth\n      x-auth-type: oauth1\n");
        let schemes = classify_schemes(&doc);
        assert_eq!(schemes[0].kind, AuthSchemeKind::OAuth1);
        assert_eq!(schemes[0].location, None);
    }

    #[test]
    fn open_id_connect_is_ambiguous_and_yields_no_scheme() {
        let doc = doc_with_scheme(
            "      type: openIdConnect\n      openIdConnectUrl: https://example.com/.well-known/openid-configuration\n",
        );
        assert!(classify_schemes(&doc).is_empty());
    }

    #[test]
    fn missing_components_yields_no_schemes() {
        let doc = crate::openapi::parse::parse_document(
            r#"
openapi: 3.0.0
info:
  title: Test
  version: "1.0.0"
paths: {}
"#,
            Some(crate::openapi::parse::Format::Yaml),
        )
        .unwrap();
        assert!(classify_schemes(&doc).is_empty());
    }
}
