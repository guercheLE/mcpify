use serde::Serialize;

use super::naming::{pascal_case, screaming_snake_case, snake_case};
use crate::auth_profile::AuthSchemeKind;
use crate::context::GeneratorContext;

/// One discovered auth scheme, in the shape templates need: `method_key` is
/// the literal string value the generated `auth_method` config field takes
/// (mirrors `targets::typescript::context::TsAuthSchemeView`'s
/// `'basic' | 'oauth2' | 'oauth1' | 'pat'` union, plus `apiKey`).
#[derive(Debug, Clone, Serialize)]
pub struct RsAuthSchemeView {
    pub name: String,
    pub method_key: &'static str,
}

/// One entry in the deduplicated `AuthMethod` enum the config-schema
/// template emits: `key` is the literal wire value (`method_key` above),
/// `variant_name` is its PascalCase Rust identifier. Precomputed here
/// rather than in the template, since Tera's built-in `capitalize` filter
/// doesn't preserve mixed-case input like `"apiKey"` -> `ApiKey`.
#[derive(Debug, Clone, Serialize)]
pub struct RsAuthMethodView {
    pub key: &'static str,
    pub variant_name: &'static str,
}

/// One operation, in the shape templates need to render tool/schema files.
#[derive(Debug, Clone, Serialize)]
pub struct RsOperationView {
    pub operation_id: String,
    pub path: String,
    pub method: String,
    pub summary: Option<String>,
    pub description: Option<String>,
}

/// The single Tera render context every Rust template is fed (architecture.md's
/// target-generation steps, Stories R2-R7). Derived once from
/// `GeneratorContext` via `from_context`. Mirrors `TsTemplateContext` field for
/// field, substituting Rust naming conventions where the two diverge.
#[derive(Debug, Clone, Serialize)]
pub struct RsTemplateContext {
    /// kebab-case slug used as the project directory name and display slug.
    /// Cargo itself accepts hyphenated package names (`[package] name`), so
    /// this doubles as `package_name` below.
    pub project_name: String,
    /// Cargo.toml `[package] name` — same as `project_name` in v2, kebab-case
    /// per Cargo convention (crates.io is full of hyphenated names).
    pub package_name: String,
    /// `project_name` in `snake_case`, since Rust identifiers (module paths,
    /// `use` statements, the `[[bin]] name` used as an internal symbol) can't
    /// contain hyphens the way `package_name` can.
    pub crate_name: String,
    /// Human-readable name (from the OpenAPI `info.title`), used in
    /// generated docs/descriptions.
    pub display_name: String,
    /// PascalCase struct name for the generated target-API HTTP client.
    pub client_struct_name: String,
    /// kebab-case slug identifying this project — same as `project_name`.
    pub tool_prefix: String,
    /// `tool_prefix` as `SCREAMING_SNAKE_CASE`, since kebab-case hyphens
    /// aren't valid in env var names (`{{ tool_prefix_env }}_URL`).
    pub tool_prefix_env: String,
    pub auth_schemes: Vec<RsAuthSchemeView>,
    /// Deduplicated `method_key`s, in discovery order — the literal match
    /// arms of the generated `AuthMethod` Rust enum. Deduplicated here in
    /// Rust rather than in the template, since Tera has no reliable
    /// cross-version `unique` filter to depend on.
    pub auth_method_keys: Vec<&'static str>,
    /// Deduplicated (key, PascalCase variant name) pairs, in discovery
    /// order — what the generated `AuthMethod` enum's variants are built
    /// from.
    pub auth_methods: Vec<RsAuthMethodView>,
    pub operations: Vec<RsOperationView>,
}

impl RsTemplateContext {
    pub fn from_context(ctx: &GeneratorContext) -> Self {
        let project_name = ctx
            .output_dir
            .file_name()
            .and_then(|name| name.to_str())
            .map(kebab_slug)
            .filter(|slug| !slug.is_empty())
            .unwrap_or_else(|| kebab_slug(&ctx.api_title));

        let crate_name = snake_case(&project_name);
        let client_struct_name = format!("{}Client", pascal_case(&ctx.api_title));

        let auth_schemes: Vec<RsAuthSchemeView> = ctx
            .auth_schemes
            .iter()
            .map(|scheme| RsAuthSchemeView {
                name: scheme.name.clone(),
                method_key: auth_method_key(scheme.kind),
            })
            .collect();

        let operations = ctx
            .normalized_operations
            .iter()
            .map(|op| RsOperationView {
                operation_id: op.operation_id.clone(),
                path: op.path.clone(),
                method: op.method.clone(),
                summary: op.summary.clone(),
                description: op.description.clone(),
            })
            .collect();

        let tool_prefix_env = screaming_snake_case(&project_name);

        let mut auth_method_keys: Vec<&'static str> = Vec::new();
        for scheme in &auth_schemes {
            if !auth_method_keys.contains(&scheme.method_key) {
                auth_method_keys.push(scheme.method_key);
            }
        }
        let auth_methods = auth_method_keys
            .iter()
            .map(|&key| RsAuthMethodView {
                key,
                variant_name: auth_method_variant_name(key),
            })
            .collect();

        Self {
            package_name: project_name.clone(),
            crate_name,
            tool_prefix: project_name.clone(),
            project_name,
            tool_prefix_env,
            display_name: ctx.api_title.clone(),
            client_struct_name,
            auth_schemes,
            auth_method_keys,
            auth_methods,
            operations,
        }
    }
}

/// kebab-case via `snake_case` + hyphen substitution, so `project_name`
/// matches Cargo's own hyphenated-package-name convention rather than the
/// underscored form `snake_case` alone would produce.
fn kebab_slug(input: &str) -> String {
    snake_case(input).replace('_', "-")
}

/// Maps a classified auth scheme onto the literal `auth_method` config value
/// the generated project's auth-manager `match`es on, mirroring the 4-way
/// discriminant proven by bitbucket-dc-mcp/jira-dc-mcp plus `apiKey`.
fn auth_method_key(kind: AuthSchemeKind) -> &'static str {
    match kind {
        AuthSchemeKind::Basic => "basic",
        AuthSchemeKind::ApiKey => "apiKey",
        AuthSchemeKind::BearerPat => "pat",
        AuthSchemeKind::OAuth2 => "oauth2",
        AuthSchemeKind::OAuth1 => "oauth1",
    }
}

/// Maps an `auth_method_key` literal onto its PascalCase Rust enum-variant
/// identifier. A closed match over the same 5 literals `auth_method_key`
/// can produce, so this can never actually hit its `unreachable!` arm.
fn auth_method_variant_name(key: &str) -> &'static str {
    match key {
        "basic" => "Basic",
        "apiKey" => "ApiKey",
        "pat" => "Pat",
        "oauth2" => "OAuth2",
        "oauth1" => "OAuth1",
        other => unreachable!("auth_method_key never returns '{other}'"),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::auth_profile::AuthSchemeDescriptor;
    use crate::openapi::NormalizedOperation;

    fn sample_context() -> GeneratorContext {
        GeneratorContext {
            openapi_input: "spec.yaml".to_string(),
            output_dir: PathBuf::from("./my-api-mcp"),
            force: false,
            output_dir_preexisted: false,
            auth_schemes: vec![AuthSchemeDescriptor {
                name: "basicAuth".to_string(),
                kind: AuthSchemeKind::Basic,
            }],
            normalized_operations: vec![NormalizedOperation {
                operation_id: "listWidgets".to_string(),
                path: "/widgets".to_string(),
                method: "GET".to_string(),
                summary: Some("List widgets".to_string()),
                description: None,
                input_schema: serde_json::json!({}),
                output_schema: serde_json::json!({}),
                auth_scheme_ref: Some("basicAuth".to_string()),
                validation_input_schema: serde_json::json!({}),
                validation_output_schema: serde_json::json!({}),
            }],
            api_title: "Widget API".to_string(),
        }
    }

    #[test]
    fn derives_project_name_from_output_dir() {
        let view = RsTemplateContext::from_context(&sample_context());
        assert_eq!(view.project_name, "my-api-mcp");
        assert_eq!(view.package_name, "my-api-mcp");
        assert_eq!(view.tool_prefix, "my-api-mcp");
    }

    #[test]
    fn derives_snake_case_crate_name_from_kebab_case_project_name() {
        let view = RsTemplateContext::from_context(&sample_context());
        assert_eq!(view.crate_name, "my_api_mcp");
    }

    #[test]
    fn derives_display_name_and_client_struct_name_from_api_title() {
        let view = RsTemplateContext::from_context(&sample_context());
        assert_eq!(view.display_name, "Widget API");
        assert_eq!(view.client_struct_name, "WidgetApiClient");
    }

    #[test]
    fn derives_screaming_snake_case_env_prefix_from_kebab_case_project_name() {
        let view = RsTemplateContext::from_context(&sample_context());
        assert_eq!(view.project_name, "my-api-mcp");
        assert_eq!(view.tool_prefix_env, "MY_API_MCP");
    }

    #[test]
    fn maps_auth_schemes_to_method_keys() {
        let view = RsTemplateContext::from_context(&sample_context());
        assert_eq!(view.auth_schemes.len(), 1);
        assert_eq!(view.auth_schemes[0].method_key, "basic");
    }

    #[test]
    fn carries_operations_through() {
        let view = RsTemplateContext::from_context(&sample_context());
        assert_eq!(view.operations.len(), 1);
        assert_eq!(view.operations[0].operation_id, "listWidgets");
    }

    #[test]
    fn dedupes_auth_method_keys_preserving_discovery_order() {
        let mut ctx = sample_context();
        ctx.auth_schemes = vec![
            AuthSchemeDescriptor {
                name: "oauth2Primary".to_string(),
                kind: AuthSchemeKind::OAuth2,
            },
            AuthSchemeDescriptor {
                name: "basicAuth".to_string(),
                kind: AuthSchemeKind::Basic,
            },
            AuthSchemeDescriptor {
                name: "oauth2Secondary".to_string(),
                kind: AuthSchemeKind::OAuth2,
            },
        ];
        let view = RsTemplateContext::from_context(&ctx);
        assert_eq!(view.auth_method_keys, vec!["oauth2", "basic"]);
    }

    #[test]
    fn derives_pascal_case_variant_names_for_auth_methods() {
        let mut ctx = sample_context();
        ctx.auth_schemes = vec![AuthSchemeDescriptor {
            name: "apiKeyAuth".to_string(),
            kind: AuthSchemeKind::ApiKey,
        }];
        let view = RsTemplateContext::from_context(&ctx);
        assert_eq!(view.auth_methods.len(), 1);
        assert_eq!(view.auth_methods[0].key, "apiKey");
        assert_eq!(view.auth_methods[0].variant_name, "ApiKey");
    }

    #[test]
    fn falls_back_to_api_title_when_output_dir_has_no_usable_file_name() {
        let mut ctx = sample_context();
        ctx.output_dir = PathBuf::from("/");
        let view = RsTemplateContext::from_context(&ctx);
        assert_eq!(view.project_name, "widget-api");
    }
}
