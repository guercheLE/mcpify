use serde::Serialize;

use super::naming::{pascal_case, screaming_snake_case};
use crate::auth_profile::AuthSchemeKind;
use crate::context::{GeneratorContext, VersionEntryView};

/// One discovered auth scheme, in the shape templates need: `method_key` is
/// the literal string value the generated `AuthMethod` config field takes
/// (mirrors `targets::csharp::context::CsAuthSchemeView`'s
/// `"basic" | "oauth2" | "oauth1" | "pat"` union, plus `apiKey`).
#[derive(Debug, Clone, Serialize)]
pub struct GoAuthSchemeView {
    pub name: String,
    pub method_key: &'static str,
}

/// One entry in the deduplicated auth-method list the config-schema
/// template emits: `key` is the literal wire value (`method_key` above),
/// `type_name` is its PascalCase Go identifier (used for the per-strategy
/// struct name in `internal/auth/`, e.g. `BasicAuthStrategy`).
#[derive(Debug, Clone, Serialize)]
pub struct GoAuthMethodView {
    pub key: &'static str,
    pub type_name: &'static str,
}

/// One operation, in the shape templates need to render tool/schema files.
#[derive(Debug, Clone, Serialize)]
pub struct GoOperationView {
    pub operation_id: String,
    pub path: String,
    pub method: String,
    pub summary: Option<String>,
    pub description: Option<String>,
}

/// The single Tera render context every Go template is fed (architecture.md's
/// target-generation steps, Stories G2-G7). Derived once from
/// `GeneratorContext` via `from_context`. Mirrors `CsTemplateContext` /
/// `PyTemplateContext` field for field, substituting Go conventions where
/// they diverge: unlike C#'s capitalized namespace or Python's underscored
/// module directory, a Go module path is conventionally lowercase and can
/// contain hyphens, so `module_path` reuses the same kebab-case slug as
/// `project_name` rather than needing its own case conversion (Go import
/// paths tolerate hyphens; only Go *identifiers* — handled by `naming.rs`
/// — cannot).
#[derive(Debug, Clone, Serialize)]
pub struct GoTemplateContext {
    /// kebab-case slug used as the project directory name and display slug.
    pub project_name: String,
    /// `go.mod`'s `module` directive and the root of every internal import
    /// path (`{{ module_path }}/internal/...`) — same kebab-case value as
    /// `project_name` (see struct-level doc comment).
    pub module_path: String,
    /// Human-readable name (from the OpenAPI `info.title`), used in
    /// generated docs/descriptions.
    pub display_name: String,
    /// PascalCase Go struct name for the generated target-API HTTP client.
    pub client_type_name: String,
    /// kebab-case slug identifying this project — same as `project_name`.
    pub tool_prefix: String,
    /// `tool_prefix` as `SCREAMING_SNAKE_CASE`, since kebab-case hyphens
    /// aren't valid in env var names (`{{ tool_prefix_env }}_URL`).
    pub tool_prefix_env: String,
    pub auth_schemes: Vec<GoAuthSchemeView>,
    /// Deduplicated `method_key`s, in discovery order — the literal values
    /// the generated `authmanager` dispatch map is keyed by. Deduplicated
    /// here in Rust rather than in the template, since Tera has no reliable
    /// cross-version `unique` filter to depend on.
    pub auth_method_keys: Vec<&'static str>,
    /// Deduplicated (key, PascalCase type name) pairs, in discovery
    /// order — what the generated `map[string]AuthStrategy` registration is
    /// built from.
    pub auth_methods: Vec<GoAuthMethodView>,
    pub operations: Vec<GoOperationView>,
    /// v8 multi-version support — see `targets::typescript::context::TsTemplateContext::version_entries`.
    pub version_entries: Vec<VersionEntryView>,
    pub default_version_label: String,
}

impl GoTemplateContext {
    pub fn from_context(ctx: &GeneratorContext) -> Self {
        let project_name = ctx
            .output_dir
            .file_name()
            .and_then(|name| name.to_str())
            .map(kebab_slug)
            .filter(|slug| !slug.is_empty())
            .unwrap_or_else(|| kebab_slug(&ctx.api_title));

        let client_type_name = format!("{}Client", pascal_case(&ctx.api_title));

        let auth_schemes: Vec<GoAuthSchemeView> = ctx
            .auth_schemes
            .iter()
            .map(|scheme| GoAuthSchemeView {
                name: scheme.name.clone(),
                method_key: auth_method_key(scheme.kind),
            })
            .collect();

        let operations = ctx
            .normalized_operations
            .iter()
            .map(|op| GoOperationView {
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
            .map(|&key| GoAuthMethodView {
                key,
                type_name: auth_method_type_name(key),
            })
            .collect();

        let version_entries = vec![VersionEntryView::from_project_relative_paths(
            &ctx.version_label,
            crate::db::STORE_FILE_NAME,
            super::steps::tools::GENERATED_SCHEMAS_RELATIVE_PATH,
        )];

        Self {
            module_path: project_name.clone(),
            tool_prefix: project_name.clone(),
            project_name,
            tool_prefix_env,
            display_name: ctx.api_title.clone(),
            client_type_name,
            auth_schemes,
            auth_method_keys,
            auth_methods,
            operations,
            version_entries,
            default_version_label: ctx.version_label.clone(),
        }
    }
}

/// kebab-case slug used for the project directory name and module path —
/// mirrors `targets::csharp::context::kebab_slug`.
fn kebab_slug(input: &str) -> String {
    let mut slug = String::new();
    let mut prev_is_sep = true;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_is_sep = false;
        } else if !prev_is_sep {
            slug.push('-');
            prev_is_sep = true;
        }
    }
    slug.trim_end_matches('-').to_string()
}

/// Maps a classified auth scheme onto the literal `AuthMethod` config value
/// the generated project's `authmanager` dispatch map is keyed by,
/// mirroring the same 5-way discriminant every other target uses.
fn auth_method_key(kind: AuthSchemeKind) -> &'static str {
    match kind {
        AuthSchemeKind::Basic => "basic",
        AuthSchemeKind::ApiKey => "apiKey",
        AuthSchemeKind::BearerPat => "pat",
        AuthSchemeKind::OAuth2 => "oauth2",
        AuthSchemeKind::OAuth1 => "oauth1",
    }
}

/// Maps an `auth_method_key` literal onto its PascalCase Go type-name
/// identifier. A closed match over the same 5 literals `auth_method_key`
/// can produce, so this can never actually hit its `unreachable!` arm.
fn auth_method_type_name(key: &str) -> &'static str {
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
            publish_registry: false,
            openapi_input: "spec.yaml".to_string(),
            output_dir: PathBuf::from("./my-api-mcp"),
            force: false,
            output_dir_preexisted: false,
            auth_schemes: vec![
                AuthSchemeDescriptor {
                    name: "basicAuth".to_string(),
                    kind: AuthSchemeKind::Basic,
                },
                AuthSchemeDescriptor {
                    name: "legacyBasicAuth".to_string(),
                    kind: AuthSchemeKind::Basic,
                },
            ],
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
    fn derives_kebab_project_name_from_output_dir() {
        let ctx = sample_context();
        let view = GoTemplateContext::from_context(&ctx);
        assert_eq!(view.project_name, "my-api-mcp");
    }

    #[test]
    fn derives_module_path_matching_project_name() {
        let ctx = sample_context();
        let view = GoTemplateContext::from_context(&ctx);
        assert_eq!(view.module_path, "my-api-mcp");
    }

    #[test]
    fn derives_screaming_snake_tool_prefix_env() {
        let ctx = sample_context();
        let view = GoTemplateContext::from_context(&ctx);
        assert_eq!(view.tool_prefix_env, "MY_API_MCP");
    }

    #[test]
    fn derives_client_type_name_from_api_title() {
        let ctx = sample_context();
        let view = GoTemplateContext::from_context(&ctx);
        assert_eq!(view.client_type_name, "WidgetApiClient");
    }

    #[test]
    fn falls_back_to_api_title_when_output_dir_has_no_usable_name() {
        let mut ctx = sample_context();
        ctx.output_dir = PathBuf::from("/");
        let view = GoTemplateContext::from_context(&ctx);
        assert_eq!(view.project_name, "widget-api");
    }

    #[test]
    fn deduplicates_auth_method_keys_and_methods_in_discovery_order() {
        let ctx = sample_context();
        let view = GoTemplateContext::from_context(&ctx);
        assert_eq!(view.auth_method_keys, vec!["basic"]);
        assert_eq!(view.auth_methods.len(), 1);
        assert_eq!(view.auth_methods[0].key, "basic");
        assert_eq!(view.auth_methods[0].type_name, "Basic");
    }
}
