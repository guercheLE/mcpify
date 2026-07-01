use serde::Serialize;

use super::naming::{kebab_case, pascal_case, screaming_snake_case};
use crate::auth_profile::AuthSchemeKind;
use crate::context::GeneratorContext;

/// One discovered auth scheme, in the shape templates need: `method_key` is
/// the literal string value the generated `auth_method` config field takes
/// (mirrors the reference servers' `'basic' | 'oauth2' | 'oauth1' | 'pat'`
/// union, plus `apiKey`).
#[derive(Debug, Clone, Serialize)]
pub struct TsAuthSchemeView {
    pub name: String,
    pub method_key: &'static str,
}

/// One operation, in the shape templates need to render tool/schema files.
#[derive(Debug, Clone, Serialize)]
pub struct TsOperationView {
    pub operation_id: String,
    pub path: String,
    pub method: String,
    pub summary: Option<String>,
    pub description: Option<String>,
}

/// The single Tera render context every TypeScript template is fed
/// (architecture.md's target-generation steps, Stories 8-13). Derived once
/// from `GeneratorContext` via `from_context`.
#[derive(Debug, Clone, Serialize)]
pub struct TsTemplateContext {
    /// kebab-case slug used as the npm package name and CLI binary name.
    pub project_name: String,
    /// npm package name — same as `project_name` in v1.
    pub package_name: String,
    /// Human-readable name (from the OpenAPI `info.title`), used in
    /// generated docs/descriptions.
    pub display_name: String,
    /// PascalCase class name for the generated target-API HTTP client.
    pub client_class_name: String,
    /// kebab-case slug identifying this project — same as `project_name` in v1.
    pub tool_prefix: String,
    /// `tool_prefix` as `SCREAMING_SNAKE_CASE`, since kebab-case hyphens
    /// aren't valid in env var names (`{{ tool_prefix_env }}_URL`).
    pub tool_prefix_env: String,
    pub auth_schemes: Vec<TsAuthSchemeView>,
    /// Deduplicated `method_key`s, in discovery order — the literal union
    /// members of the generated `AuthMethod` TS type
    /// (`'basic' | 'oauth2' | ...`). Deduplicated here in Rust rather than
    /// in the template, since Tera has no reliable cross-version `unique`
    /// filter to depend on.
    pub auth_method_keys: Vec<&'static str>,
    pub operations: Vec<TsOperationView>,
}

impl TsTemplateContext {
    pub fn from_context(ctx: &GeneratorContext) -> Self {
        let project_name = ctx
            .output_dir
            .file_name()
            .and_then(|name| name.to_str())
            .map(kebab_case)
            .filter(|slug| !slug.is_empty())
            .unwrap_or_else(|| kebab_case(&ctx.api_title));

        let client_class_name = format!("{}ClientService", pascal_case(&ctx.api_title));

        let auth_schemes: Vec<TsAuthSchemeView> = ctx
            .auth_schemes
            .iter()
            .map(|scheme| TsAuthSchemeView {
                name: scheme.name.clone(),
                method_key: auth_method_key(scheme.kind),
            })
            .collect();

        let operations = ctx
            .normalized_operations
            .iter()
            .map(|op| TsOperationView {
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

        Self {
            package_name: project_name.clone(),
            tool_prefix: project_name.clone(),
            project_name,
            tool_prefix_env,
            display_name: ctx.api_title.clone(),
            client_class_name,
            auth_schemes,
            auth_method_keys,
            operations,
        }
    }
}

/// Maps a classified auth scheme onto the literal `auth_method` config value
/// the generated project's auth-manager selects on, mirroring the 4-way
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
        let view = TsTemplateContext::from_context(&sample_context());
        assert_eq!(view.project_name, "my-api-mcp");
        assert_eq!(view.package_name, "my-api-mcp");
        assert_eq!(view.tool_prefix, "my-api-mcp");
    }

    #[test]
    fn derives_display_name_and_client_class_name_from_api_title() {
        let view = TsTemplateContext::from_context(&sample_context());
        assert_eq!(view.display_name, "Widget API");
        assert_eq!(view.client_class_name, "WidgetApiClientService");
    }

    #[test]
    fn derives_screaming_snake_case_env_prefix_from_kebab_case_project_name() {
        let view = TsTemplateContext::from_context(&sample_context());
        assert_eq!(view.project_name, "my-api-mcp");
        assert_eq!(view.tool_prefix_env, "MY_API_MCP");
    }

    #[test]
    fn maps_auth_schemes_to_method_keys() {
        let view = TsTemplateContext::from_context(&sample_context());
        assert_eq!(view.auth_schemes.len(), 1);
        assert_eq!(view.auth_schemes[0].method_key, "basic");
    }

    #[test]
    fn carries_operations_through() {
        let view = TsTemplateContext::from_context(&sample_context());
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
        let view = TsTemplateContext::from_context(&ctx);
        assert_eq!(view.auth_method_keys, vec!["oauth2", "basic"]);
    }

    #[test]
    fn falls_back_to_api_title_when_output_dir_has_no_usable_file_name() {
        let mut ctx = sample_context();
        ctx.output_dir = PathBuf::from("/");
        let view = TsTemplateContext::from_context(&ctx);
        assert_eq!(view.project_name, "widget-api");
    }
}
