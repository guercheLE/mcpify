use anyhow::Result;

use crate::context::GeneratorContext;
use crate::targets::typescript::context::TsTemplateContext;
use crate::targets::typescript::emit::render_and_write;
use crate::targets::typescript::render::render_engine;

/// Every file this step emits, before any tool-specific code exists
/// (architecture.md §1, step 6): the ~17 inline core modules every later
/// file imports, plus Docker/CI packaging. This ordering is what guarantees
/// files generated later (Stories 10-13) never need to be revisited to "add"
/// enterprise features — they're already available to import.
const FILES: &[(&str, &str)] = &[
    ("core/errors.ts.tera", "src/core/errors.ts"),
    ("core/sanitizer.ts.tera", "src/core/sanitizer.ts"),
    (
        "core/correlation-context.ts.tera",
        "src/core/correlation-context.ts",
    ),
    ("core/log-transport.ts.tera", "src/core/log-transport.ts"),
    ("core/logger.ts.tera", "src/core/logger.ts"),
    ("core/tracing.ts.tera", "src/core/tracing.ts"),
    ("core/config-schema.ts.tera", "src/core/config-schema.ts"),
    ("core/config-manager.ts.tera", "src/core/config-manager.ts"),
    (
        "core/component-registry.ts.tera",
        "src/core/component-registry.ts",
    ),
    (
        "core/health-check-manager.ts.tera",
        "src/core/health-check-manager.ts",
    ),
    (
        "core/circuit-breaker.ts.tera",
        "src/core/circuit-breaker.ts",
    ),
    ("core/rate-limiter.ts.tera", "src/core/rate-limiter.ts"),
    ("core/cache-manager.ts.tera", "src/core/cache-manager.ts"),
    (
        "core/credential-storage.ts.tera",
        "src/core/credential-storage.ts",
    ),
    (
        "core/api-url-builder.ts.tera",
        "src/core/api-url-builder.ts",
    ),
    (
        "core/shutdown-handler.ts.tera",
        "src/core/shutdown-handler.ts",
    ),
    ("core/mcp-server.ts.tera", "src/core/mcp-server.ts"),
    ("healthcheck.ts.tera", "src/healthcheck.ts"),
    (
        "scripts/populate-embeddings.ts.tera",
        "scripts/populate-embeddings.ts",
    ),
    ("Dockerfile.tera", "Dockerfile"),
    ("docker-compose.yml.tera", "docker-compose.yml"),
    (".releaserc.json.tera", ".releaserc.json"),
    (".github/workflows/ci.yml.tera", ".github/workflows/ci.yml"),
    (
        ".github/workflows/docker-build.yml.tera",
        ".github/workflows/docker-build.yml",
    ),
    (
        ".github/workflows/release.yml.tera",
        ".github/workflows/release.yml",
    ),
];

/// `generate_enterprise_scaffolding` (architecture.md §1, step 6).
pub async fn generate_enterprise_scaffolding(ctx: &GeneratorContext) -> Result<()> {
    let view = TsTemplateContext::from_context(ctx);
    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in FILES {
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::auth_profile::{AuthSchemeDescriptor, AuthSchemeKind};

    fn ctx_with_schemes(
        output_dir: PathBuf,
        auth_schemes: Vec<AuthSchemeDescriptor>,
    ) -> GeneratorContext {
        GeneratorContext {
            publish_registry: false,
            openapi_input: "spec.yaml".to_string(),
            output_dir,
            force: false,
            output_dir_preexisted: false,
            auth_schemes,
            normalized_operations: Vec::new(),
            api_title: "Widget API".to_string(),
            version_label: "default".to_string(),
        }
    }

    #[tokio::test]
    async fn writes_every_enterprise_file() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_schemes(dir.path().to_path_buf(), Vec::new());

        generate_enterprise_scaffolding(&ctx).await.unwrap();

        for (_, out_name) in FILES {
            assert!(dir.path().join(out_name).is_file(), "missing {out_name}");
        }
    }

    #[tokio::test]
    async fn releaserc_is_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_schemes(dir.path().to_path_buf(), Vec::new());

        generate_enterprise_scaffolding(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.path().join(".releaserc.json"))
            .await
            .unwrap();
        serde_json::from_str::<serde_json::Value>(&contents)
            .expect(".releaserc.json must be valid JSON");
    }

    #[tokio::test]
    async fn config_schema_auth_union_has_two_members_for_two_schemes() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_schemes(
            dir.path().to_path_buf(),
            vec![
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
                    name: "oauth2".to_string(),
                    kind: AuthSchemeKind::OAuth2,
                    location: None,
                },
            ],
        );

        generate_enterprise_scaffolding(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.path().join("src/core/config-schema.ts"))
            .await
            .unwrap();
        assert!(contents.contains("'basic',"));
        assert!(contents.contains("'oauth2',"));
        assert!(!contents.contains("'pat',"));
        assert!(!contents.contains("'oauth1',"));
        assert!(!contents.contains("'apiKey',"));
    }

    #[tokio::test]
    async fn config_schema_auth_union_covers_all_four_scheme_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_schemes(
            dir.path().to_path_buf(),
            vec![
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
                    name: "pat".to_string(),
                    kind: AuthSchemeKind::BearerPat,
                    location: None,
                },
                AuthSchemeDescriptor {
                    scopes: Vec::new(),
                    authorization_url: None,
                    token_url: None,
                    name: "oauth1".to_string(),
                    kind: AuthSchemeKind::OAuth1,
                    location: None,
                },
                AuthSchemeDescriptor {
                    scopes: Vec::new(),
                    authorization_url: None,
                    token_url: None,
                    name: "oauth2".to_string(),
                    kind: AuthSchemeKind::OAuth2,
                    location: None,
                },
            ],
        );

        generate_enterprise_scaffolding(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.path().join("src/core/config-schema.ts"))
            .await
            .unwrap();
        for expected in ["'basic',", "'pat',", "'oauth1',", "'oauth2',"] {
            assert!(
                contents.contains(expected),
                "missing {expected} in config-schema.ts"
            );
        }
    }
}
