use anyhow::Result;

use crate::context::GeneratorContext;
use crate::targets::rust::context::RsTemplateContext;
use crate::targets::rust::emit::render_and_write;
use crate::targets::rust::render::render_engine;

/// Every file this step emits, before any tool-specific code exists
/// (architecture.md §1, step 6): the 17 inline core modules (plus their
/// `mod.rs` aggregator — needed because Rust, unlike TypeScript, requires an
/// explicit module declaration per file) every later file imports, plus the
/// two auxiliary `[[bin]]` targets and Docker/CI packaging. Mirrors
/// `targets::typescript::steps::enterprise`'s `FILES` list file-for-file
/// where a Rust equivalent exists; `.releaserc.json` has no Rust
/// counterpart (semantic-release doesn't apply to an unpublished
/// application binary — see `release.yml.tera`'s own comment).
const FILES: &[(&str, &str)] = &[
    ("core/mod.rs.tera", "src/core/mod.rs"),
    ("core/errors.rs.tera", "src/core/errors.rs"),
    ("core/sanitizer.rs.tera", "src/core/sanitizer.rs"),
    (
        "core/correlation_context.rs.tera",
        "src/core/correlation_context.rs",
    ),
    ("core/log_transport.rs.tera", "src/core/log_transport.rs"),
    ("core/logger.rs.tera", "src/core/logger.rs"),
    ("core/otel.rs.tera", "src/core/otel.rs"),
    ("core/config_schema.rs.tera", "src/core/config_schema.rs"),
    ("core/config_manager.rs.tera", "src/core/config_manager.rs"),
    (
        "core/component_registry.rs.tera",
        "src/core/component_registry.rs",
    ),
    (
        "core/health_check_manager.rs.tera",
        "src/core/health_check_manager.rs",
    ),
    (
        "core/circuit_breaker.rs.tera",
        "src/core/circuit_breaker.rs",
    ),
    ("core/rate_limiter.rs.tera", "src/core/rate_limiter.rs"),
    ("core/cache_manager.rs.tera", "src/core/cache_manager.rs"),
    (
        "core/credential_storage.rs.tera",
        "src/core/credential_storage.rs",
    ),
    (
        "core/api_url_builder.rs.tera",
        "src/core/api_url_builder.rs",
    ),
    (
        "core/shutdown_handler.rs.tera",
        "src/core/shutdown_handler.rs",
    ),
    ("core/mcp_server.rs.tera", "src/core/mcp_server.rs"),
    ("bin/healthcheck.rs.tera", "src/bin/healthcheck.rs"),
    (
        "bin/populate_embeddings.rs.tera",
        "src/bin/populate_embeddings.rs",
    ),
    ("Dockerfile.tera", "Dockerfile"),
    ("docker-compose.yml.tera", "docker-compose.yml"),
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
    let view = RsTemplateContext::from_context(ctx);
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
    async fn config_schema_auth_enum_has_two_variants_for_two_schemes() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_schemes(
            dir.path().to_path_buf(),
            vec![
                AuthSchemeDescriptor {
                    name: "basicAuth".to_string(),
                    kind: AuthSchemeKind::Basic,
                },
                AuthSchemeDescriptor {
                    name: "oauth2".to_string(),
                    kind: AuthSchemeKind::OAuth2,
                },
            ],
        );

        generate_enterprise_scaffolding(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.path().join("src/core/config_schema.rs"))
            .await
            .unwrap();
        assert!(contents.contains("Basic,"));
        assert!(contents.contains("OAuth2,"));
        assert!(!contents.contains("Pat,"));
        assert!(!contents.contains("OAuth1,"));
        assert!(!contents.contains("ApiKey,"));
    }

    #[tokio::test]
    async fn config_schema_auth_enum_covers_all_four_scheme_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_schemes(
            dir.path().to_path_buf(),
            vec![
                AuthSchemeDescriptor {
                    name: "basicAuth".to_string(),
                    kind: AuthSchemeKind::Basic,
                },
                AuthSchemeDescriptor {
                    name: "pat".to_string(),
                    kind: AuthSchemeKind::BearerPat,
                },
                AuthSchemeDescriptor {
                    name: "oauth1".to_string(),
                    kind: AuthSchemeKind::OAuth1,
                },
                AuthSchemeDescriptor {
                    name: "oauth2".to_string(),
                    kind: AuthSchemeKind::OAuth2,
                },
            ],
        );

        generate_enterprise_scaffolding(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.path().join("src/core/config_schema.rs"))
            .await
            .unwrap();
        for expected in ["Basic,", "Pat,", "OAuth1,", "OAuth2,"] {
            assert!(
                contents.contains(expected),
                "missing {expected} in config_schema.rs"
            );
        }
    }
}
