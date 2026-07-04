use anyhow::Result;

use crate::context::GeneratorContext;
use crate::targets::go::context::GoTemplateContext;
use crate::targets::go::emit::render_and_write;
use crate::targets::go::render::render_engine;

/// Files rendered under `internal/core/` — one Go package (`package core`)
/// spanning multiple files, one per concern (architecture.md §1, step 6).
/// Mirrors `targets::csharp::steps::enterprise::CORE_FILES`'s file-per-concern
/// split, but keeps `circuitbreaker`/`ratelimiter` and
/// `credentialstorage`/`cache` as separate files rather than C#'s
/// DI-container-driven folding of those pairs — Go has no first-party
/// package like C#'s `Microsoft.Extensions.Http.Resilience` bundling
/// circuit-breaking and rate-limiting together, so this target keeps the
/// ~17-core-module split v5-implementation-plan.md's G3 goal text
/// describes.
const CORE_FILES: &[(&str, &str)] = &[
    ("internal/core/errors.go.tera", "internal/core/errors.go"),
    ("internal/core/logger.go.tera", "internal/core/logger.go"),
    ("internal/core/tracing.go.tera", "internal/core/tracing.go"),
    ("internal/core/config.go.tera", "internal/core/config.go"),
    (
        "internal/core/circuitbreaker.go.tera",
        "internal/core/circuitbreaker.go",
    ),
    (
        "internal/core/ratelimiter.go.tera",
        "internal/core/ratelimiter.go",
    ),
    (
        "internal/core/credentialstorage.go.tera",
        "internal/core/credentialstorage.go",
    ),
    ("internal/core/cache.go.tera", "internal/core/cache.go"),
    (
        "internal/core/healthcheck.go.tera",
        "internal/core/healthcheck.go",
    ),
    (
        "internal/core/mcpserver.go.tera",
        "internal/core/mcpserver.go",
    ),
];

/// Root-level packaging/CI files, rendered directly into `ctx.output_dir`.
const ROOT_FILES: &[(&str, &str)] = &[
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
    let view = GoTemplateContext::from_context(ctx);
    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in CORE_FILES {
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }

    for (template, out_name) in ROOT_FILES {
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
        }
    }

    fn output_dir(parent: &tempfile::TempDir) -> PathBuf {
        parent.path().join("output")
    }

    #[tokio::test]
    async fn writes_every_core_file() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_enterprise_scaffolding(&ctx).await.unwrap();

        for (_, out_name) in CORE_FILES {
            assert!(dir.join(out_name).is_file(), "missing {out_name}");
        }
    }

    #[tokio::test]
    async fn writes_every_root_file() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_enterprise_scaffolding(&ctx).await.unwrap();

        for (_, out_name) in ROOT_FILES {
            assert!(dir.join(out_name).is_file(), "missing {out_name}");
        }
    }

    #[tokio::test]
    async fn config_auth_method_consts_cover_only_discovered_schemes() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(
            dir.clone(),
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

        let contents = tokio::fs::read_to_string(dir.join("internal/core/config.go"))
            .await
            .unwrap();
        assert!(contents.contains(r#"AuthMethodBasic AuthMethod = "basic""#));
        assert!(contents.contains(r#"AuthMethodOAuth2 AuthMethod = "oauth2""#));
        assert!(!contents.contains("AuthMethodPat"));
        assert!(!contents.contains("AuthMethodOAuth1"));
    }

    #[tokio::test]
    async fn config_declares_no_auth_method_consts_with_no_schemes() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_enterprise_scaffolding(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.join("internal/core/config.go"))
            .await
            .unwrap();
        assert!(contents.contains("type AuthMethod string"));
        assert!(contents.contains(r#"const AuthMethodNone AuthMethod = """#));
        assert!(!contents.contains("const AuthMethodBasic"));
    }

    #[tokio::test]
    async fn dockerfile_stages_the_onnx_runtime_shared_library() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_enterprise_scaffolding(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.join("Dockerfile"))
            .await
            .unwrap();
        assert!(contents.contains("libonnxruntime"));
        assert!(contents.contains("AS onnxruntime"));
    }
}
