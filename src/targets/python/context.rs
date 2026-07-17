use serde::Serialize;

use super::naming::{pascal_case, screaming_snake_case, snake_case};
use crate::auth_profile::{AuthSchemeKind, location_view};
use crate::context::{GeneratorContext, VersionEntryView};
use crate::project_config::{HeaderSetting, PublicationMetadata, read_settings};

/// One discovered auth scheme, in the shape templates need: `method_key` is
/// the literal string value the generated `auth_method` config field takes
/// (mirrors `targets::rust::context::RsAuthSchemeView`'s
/// `'basic' | 'oauth2' | 'oauth1' | 'pat'` union, plus `apiKey`).
#[derive(Debug, Clone, Serialize)]
pub struct PyAuthSchemeView {
    pub name: String,
    pub method_key: &'static str,
    pub header_location: &'static str,
    pub header_name: String,
    /// Space-joined OAuth2 scope identifiers declared under this scheme's
    /// `flows` in the spec — empty for non-OAuth2 schemes, or an OAuth2
    /// scheme that declares none. Feeds the setup wizard's scope prompt
    /// default.
    pub scopes: String,
    /// The declared `authorizationUrl`/`tokenUrl` for this scheme's OAuth2
    /// flow — `None` for non-OAuth2 schemes or a flow that doesn't declare
    /// one. Pre-fills the setup wizard's URL prompts.
    pub authorization_url: Option<String>,
    pub token_url: Option<String>,
}

/// One entry in the deduplicated auth-method list the config-schema
/// template emits: `key` is the literal wire value (`method_key` above),
/// `class_name` is its PascalCase Python identifier (used for the
/// per-strategy class name in `auth/strategies/`).
#[derive(Debug, Clone, Serialize)]
pub struct PyAuthMethodView {
    pub key: &'static str,
    pub class_name: &'static str,
    pub header_location: &'static str,
    pub header_name: String,
    /// Union of every same-`key` scheme's declared scopes, deduplicated
    /// and space-joined — empty for anything but `oauth2`.
    pub scopes: String,
    /// The first same-`key` scheme's declared `authorizationUrl`/`tokenUrl`
    /// — `None` for anything but `oauth2`, or an `oauth2` scheme with no
    /// declared flow URLs.
    pub authorization_url: Option<String>,
    pub token_url: Option<String>,
}

/// One operation, in the shape templates need to render tool/schema files.
#[derive(Debug, Clone, Serialize)]
pub struct PyOperationView {
    pub operation_id: String,
    pub path: String,
    pub method: String,
    pub summary: Option<String>,
    pub description: Option<String>,
}

/// The single Tera render context every Python template is fed
/// (architecture.md's target-generation steps, Stories P2-P7). Derived once
/// from `GeneratorContext` via `from_context`. Mirrors `RsTemplateContext`
/// field for field, substituting Python naming conventions where the two
/// diverge (a hyphenated `pyproject.toml` package name vs. an
/// underscored, import-safe module/package directory name).
#[derive(Debug, Clone, Serialize)]
pub struct PyTemplateContext {
    /// kebab-case slug used as the project directory name and display slug.
    pub project_name: String,
    /// `pyproject.toml` `[project] name` — same as `project_name`, kebab-case
    /// per PyPI convention (PyPI normalizes hyphens/underscores anyway, but
    /// hyphenated is the more common convention for distribution names).
    pub package_name: String,
    /// `project_name` in `snake_case`, since Python import paths (the
    /// `src/<module_name>/` package directory, `import <module_name>`)
    /// can't contain hyphens the way `package_name` can.
    pub module_name: String,
    /// Human-readable name (from the OpenAPI `info.title`), used in
    /// generated docs/descriptions.
    pub display_name: String,
    /// PascalCase class name for the generated target-API HTTP client.
    pub client_class_name: String,
    /// kebab-case slug identifying this project — same as `project_name`.
    pub tool_prefix: String,
    /// `tool_prefix` as `SCREAMING_SNAKE_CASE`, since kebab-case hyphens
    /// aren't valid in env var names (`{{ tool_prefix_env }}_URL`).
    pub tool_prefix_env: String,
    pub auth_schemes: Vec<PyAuthSchemeView>,
    /// Deduplicated `method_key`s, in discovery order — the literal values
    /// the generated `auth_manager.py` dispatch dict is keyed by.
    /// Deduplicated here in Rust rather than in the template, since Tera
    /// has no reliable cross-version `unique` filter to depend on.
    pub auth_method_keys: Vec<&'static str>,
    /// Deduplicated (key, PascalCase class name) pairs, in discovery
    /// order — what the generated auth-strategy dispatch dict is built
    /// from.
    pub auth_methods: Vec<PyAuthMethodView>,
    pub operations: Vec<PyOperationView>,
    /// v6 Part PUB: `--publish-registry` — whether `release.yml` emits a
    /// real `uv publish` step instead of GitHub-Release-only.
    pub publish_registry: bool,
    pub default_headers: Vec<HeaderSetting>,
    pub publication: PublicationMetadata,
    /// v8 multi-version support — see `targets::typescript::context::TsTemplateContext::version_entries`.
    pub version_entries: Vec<VersionEntryView>,
    pub default_version_label: String,
}

impl PyTemplateContext {
    pub fn from_context(ctx: &GeneratorContext) -> Self {
        let settings = read_settings(&ctx.output_dir);
        let project_name = ctx
            .output_dir
            .file_name()
            .and_then(|name| name.to_str())
            .map(kebab_slug)
            .filter(|slug| !slug.is_empty())
            .unwrap_or_else(|| kebab_slug(&ctx.api_title));

        let module_name = snake_case(&project_name);
        let client_class_name = format!("{}Client", pascal_case(&ctx.api_title));

        let auth_schemes: Vec<PyAuthSchemeView> = ctx
            .auth_schemes
            .iter()
            .map(|scheme| {
                let (header_location, header_name) = location_view(&scheme.location);
                PyAuthSchemeView {
                    name: scheme.name.clone(),
                    method_key: auth_method_key(scheme.kind),
                    header_location,
                    header_name,
                    scopes: scheme.scopes.join(" "),
                    authorization_url: scheme.authorization_url.clone(),
                    token_url: scheme.token_url.clone(),
                }
            })
            .collect();

        let operations = ctx
            .normalized_operations
            .iter()
            .map(|op| PyOperationView {
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
            .map(|&key| {
                let first_of_kind = auth_schemes
                    .iter()
                    .find(|scheme| scheme.method_key == key)
                    .expect("every auth_method_keys entry comes from an auth_schemes entry");
                let mut scopes: Vec<&str> = auth_schemes
                    .iter()
                    .filter(|scheme| scheme.method_key == key)
                    .flat_map(|scheme| scheme.scopes.split_whitespace())
                    .collect();
                scopes.sort_unstable();
                scopes.dedup();
                PyAuthMethodView {
                    key,
                    class_name: auth_method_class_name(key),
                    header_location: first_of_kind.header_location,
                    header_name: first_of_kind.header_name.clone(),
                    scopes: scopes.join(" "),
                    authorization_url: first_of_kind.authorization_url.clone(),
                    token_url: first_of_kind.token_url.clone(),
                }
            })
            .collect();

        let schemas_relative = format!(
            "src/{}/{}",
            module_name,
            super::steps::tools::GENERATED_SCHEMAS_RELATIVE_PATH
        );
        let version_entries = vec![VersionEntryView::from_project_relative_paths(
            &ctx.version_label,
            crate::db::STORE_FILE_NAME,
            &schemas_relative,
        )];

        Self {
            package_name: project_name.clone(),
            module_name,
            tool_prefix: project_name.clone(),
            project_name,
            tool_prefix_env,
            display_name: ctx.api_title.clone(),
            client_class_name,
            auth_schemes,
            auth_method_keys,
            auth_methods,
            operations,
            publish_registry: ctx.publish_registry,
            default_headers: settings.default_headers,
            publication: settings.publication,
            version_entries,
            default_version_label: ctx.version_label.clone(),
        }
    }
}

/// kebab-case via `snake_case` + hyphen substitution, so `project_name`
/// matches PyPI's hyphenated-distribution-name convention rather than the
/// underscored form `snake_case` alone would produce.
fn kebab_slug(input: &str) -> String {
    snake_case(input).replace('_', "-")
}

/// Maps a classified auth scheme onto the literal `auth_method` config value
/// the generated project's auth-manager dispatch dict is keyed by, mirroring
/// the same 5-way discriminant every other target uses.
fn auth_method_key(kind: AuthSchemeKind) -> &'static str {
    match kind {
        AuthSchemeKind::Basic => "basic",
        AuthSchemeKind::ApiKey => "apiKey",
        AuthSchemeKind::BearerPat => "pat",
        AuthSchemeKind::OAuth2 => "oauth2",
        AuthSchemeKind::OAuth1 => "oauth1",
    }
}

/// Maps an `auth_method_key` literal onto its PascalCase Python
/// class-name identifier. A closed match over the same 5 literals
/// `auth_method_key` can produce, so this can never actually hit its
/// `unreachable!` arm.
fn auth_method_class_name(key: &str) -> &'static str {
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
    use crate::auth_profile::{AuthSchemeDescriptor, AuthSchemeLocation, default_location_for};
    use crate::openapi::NormalizedOperation;

    fn sample_context() -> GeneratorContext {
        GeneratorContext {
            publish_registry: false,
            openapi_input: "spec.yaml".to_string(),
            output_dir: PathBuf::from("./my-api-mcp"),
            force: false,
            output_dir_preexisted: false,
            auth_schemes: vec![AuthSchemeDescriptor {
                scopes: Vec::new(),
                authorization_url: None,
                token_url: None,
                name: "basicAuth".to_string(),
                kind: AuthSchemeKind::Basic,
                location: default_location_for(AuthSchemeKind::Basic),
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
            version_label: "default".to_string(),
        }
    }

    #[test]
    fn derives_project_name_from_output_dir() {
        let view = PyTemplateContext::from_context(&sample_context());
        assert_eq!(view.project_name, "my-api-mcp");
        assert_eq!(view.package_name, "my-api-mcp");
        assert_eq!(view.tool_prefix, "my-api-mcp");
    }

    #[test]
    fn derives_snake_case_module_name_from_kebab_case_project_name() {
        let view = PyTemplateContext::from_context(&sample_context());
        assert_eq!(view.module_name, "my_api_mcp");
    }

    #[test]
    fn derives_display_name_and_client_class_name_from_api_title() {
        let view = PyTemplateContext::from_context(&sample_context());
        assert_eq!(view.display_name, "Widget API");
        assert_eq!(view.client_class_name, "WidgetApiClient");
    }

    #[test]
    fn derives_screaming_snake_case_env_prefix_from_kebab_case_project_name() {
        let view = PyTemplateContext::from_context(&sample_context());
        assert_eq!(view.project_name, "my-api-mcp");
        assert_eq!(view.tool_prefix_env, "MY_API_MCP");
    }

    #[test]
    fn maps_auth_schemes_to_method_keys() {
        let view = PyTemplateContext::from_context(&sample_context());
        assert_eq!(view.auth_schemes.len(), 1);
        assert_eq!(view.auth_schemes[0].method_key, "basic");
        assert_eq!(view.auth_schemes[0].header_location, "header");
        assert_eq!(view.auth_schemes[0].header_name, "Authorization");
    }

    #[test]
    fn maps_query_and_cookie_api_key_locations() {
        let mut ctx = sample_context();
        ctx.auth_schemes = vec![
            AuthSchemeDescriptor {
                scopes: Vec::new(),
                authorization_url: None,
                token_url: None,
                name: "queryKey".to_string(),
                kind: AuthSchemeKind::ApiKey,
                location: Some(AuthSchemeLocation::Query {
                    name: "api_key".to_string(),
                }),
            },
            AuthSchemeDescriptor {
                scopes: Vec::new(),
                authorization_url: None,
                token_url: None,
                name: "cookieKey".to_string(),
                kind: AuthSchemeKind::ApiKey,
                location: Some(AuthSchemeLocation::Cookie {
                    name: "session".to_string(),
                }),
            },
        ];
        let view = PyTemplateContext::from_context(&ctx);
        assert_eq!(view.auth_schemes[0].header_location, "query");
        assert_eq!(view.auth_schemes[0].header_name, "api_key");
        assert_eq!(view.auth_schemes[1].header_location, "cookie");
        assert_eq!(view.auth_schemes[1].header_name, "session");
    }

    #[test]
    fn oauth1_scheme_has_no_relayable_location() {
        let mut ctx = sample_context();
        ctx.auth_schemes = vec![AuthSchemeDescriptor {
            scopes: Vec::new(),
            authorization_url: None,
            token_url: None,
            name: "oauth1".to_string(),
            kind: AuthSchemeKind::OAuth1,
            location: default_location_for(AuthSchemeKind::OAuth1),
        }];
        let view = PyTemplateContext::from_context(&ctx);
        assert_eq!(view.auth_schemes[0].header_location, "none");
        assert_eq!(view.auth_schemes[0].header_name, "");
    }

    #[test]
    fn carries_operations_through() {
        let view = PyTemplateContext::from_context(&sample_context());
        assert_eq!(view.operations.len(), 1);
        assert_eq!(view.operations[0].operation_id, "listWidgets");
    }

    #[test]
    fn dedupes_auth_method_keys_preserving_discovery_order() {
        let mut ctx = sample_context();
        ctx.auth_schemes = vec![
            AuthSchemeDescriptor {
                scopes: Vec::new(),
                authorization_url: None,
                token_url: None,
                name: "oauth2Primary".to_string(),
                kind: AuthSchemeKind::OAuth2,
                location: None,
            },
            AuthSchemeDescriptor {
                scopes: Vec::new(),
                authorization_url: None,
                token_url: None,
                name: "basicAuth".to_string(),
                kind: AuthSchemeKind::Basic,
                location: None,
            },
            AuthSchemeDescriptor {
                scopes: Vec::new(),
                authorization_url: None,
                token_url: None,
                name: "oauth2Secondary".to_string(),
                kind: AuthSchemeKind::OAuth2,
                location: None,
            },
        ];
        let view = PyTemplateContext::from_context(&ctx);
        assert_eq!(view.auth_method_keys, vec!["oauth2", "basic"]);
    }

    #[test]
    fn derives_pascal_case_class_names_for_auth_methods() {
        let mut ctx = sample_context();
        ctx.auth_schemes = vec![AuthSchemeDescriptor {
            scopes: Vec::new(),
            authorization_url: None,
            token_url: None,
            name: "apiKeyAuth".to_string(),
            kind: AuthSchemeKind::ApiKey,
            location: None,
        }];
        let view = PyTemplateContext::from_context(&ctx);
        assert_eq!(view.auth_methods.len(), 1);
        assert_eq!(view.auth_methods[0].key, "apiKey");
        assert_eq!(view.auth_methods[0].class_name, "ApiKey");
    }

    #[test]
    fn falls_back_to_api_title_when_output_dir_has_no_usable_file_name() {
        let mut ctx = sample_context();
        ctx.output_dir = PathBuf::from("/");
        let view = PyTemplateContext::from_context(&ctx);
        assert_eq!(view.project_name, "widget-api");
    }
}
