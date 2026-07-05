use anyhow::Result;

use crate::context::GeneratorContext;
use crate::targets::python::context::PyTemplateContext;
use crate::targets::python::emit::render_and_write;
use crate::targets::python::render::render_engine;

/// Files rendered under `src/<module_name>/core/` — everything later
/// tool-specific code depends on (architecture.md §1, step 6). Mirrors
/// `targets::rust::steps::enterprise`'s `FILES` list in spirit: Python's
/// `structlog`/`FastMCP`/dataclass idioms fold several of Rust's ~17
/// separate core modules into fewer, denser files (redaction lives in
/// `logger.py` rather than a standalone `sanitizer.py`; the component
/// registry lives in `health_check.py` rather than a standalone
/// `component_registry.rs`), matching this story's own stated goal list.
const CORE_FILES: &[(&str, &str)] = &[
    ("core/errors.py.tera", "core/errors.py"),
    ("core/logger.py.tera", "core/logger.py"),
    ("core/tracing.py.tera", "core/tracing.py"),
    ("core/config.py.tera", "core/config.py"),
    ("core/circuit_breaker.py.tera", "core/circuit_breaker.py"),
    ("core/rate_limiter.py.tera", "core/rate_limiter.py"),
    ("core/cache.py.tera", "core/cache.py"),
    (
        "core/credential_storage.py.tera",
        "core/credential_storage.py",
    ),
    ("core/health_check.py.tera", "core/health_check.py"),
    ("core/mcp_server.py.tera", "core/mcp_server.py"),
];

/// Root-level packaging/CI files, rendered directly into `ctx.output_dir`
/// rather than under the package tree.
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
    let view = PyTemplateContext::from_context(ctx);
    let package_root = ctx.output_dir.join("src").join(&view.module_name);
    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in CORE_FILES {
        render_and_write(&tera, template, &tera_ctx, &package_root.join(out_name)).await?;
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
            version_label: "default".to_string(),
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

        let package_root = dir.join("src").join("output");
        for (_, out_name) in CORE_FILES {
            assert!(
                package_root.join(out_name).is_file(),
                "missing src/output/{out_name}"
            );
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
    async fn config_auth_method_enum_has_two_members_for_two_schemes() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(
            dir.clone(),
            vec![
                AuthSchemeDescriptor {
                    name: "basicAuth".to_string(),
                    kind: AuthSchemeKind::Basic,
                    location: None,
                },
                AuthSchemeDescriptor {
                    name: "oauth2".to_string(),
                    kind: AuthSchemeKind::OAuth2,
                    location: None,
                },
            ],
        );

        generate_enterprise_scaffolding(&ctx).await.unwrap();

        let contents =
            tokio::fs::read_to_string(dir.join("src").join("output").join("core/config.py"))
                .await
                .unwrap();
        assert!(contents.contains("BASIC = \"basic\""));
        assert!(contents.contains("OAUTH2 = \"oauth2\""));
        assert!(!contents.contains("PAT = \"pat\""));
        assert!(!contents.contains("OAUTH1 = \"oauth1\""));
    }

    #[tokio::test]
    async fn config_auth_method_enum_covers_all_four_scheme_kinds() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(
            dir.clone(),
            vec![
                AuthSchemeDescriptor {
                    name: "basicAuth".to_string(),
                    kind: AuthSchemeKind::Basic,
                    location: None,
                },
                AuthSchemeDescriptor {
                    name: "pat".to_string(),
                    kind: AuthSchemeKind::BearerPat,
                    location: None,
                },
                AuthSchemeDescriptor {
                    name: "oauth1".to_string(),
                    kind: AuthSchemeKind::OAuth1,
                    location: None,
                },
                AuthSchemeDescriptor {
                    name: "oauth2".to_string(),
                    kind: AuthSchemeKind::OAuth2,
                    location: None,
                },
            ],
        );

        generate_enterprise_scaffolding(&ctx).await.unwrap();

        let contents =
            tokio::fs::read_to_string(dir.join("src").join("output").join("core/config.py"))
                .await
                .unwrap();
        for expected in [
            "BASIC = \"basic\"",
            "PAT = \"pat\"",
            "OAUTH1 = \"oauth1\"",
            "OAUTH2 = \"oauth2\"",
        ] {
            assert!(
                contents.contains(expected),
                "missing {expected} in config.py"
            );
        }
    }

    #[tokio::test]
    async fn config_auth_method_enum_falls_back_to_none_with_no_schemes() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_enterprise_scaffolding(&ctx).await.unwrap();

        let contents =
            tokio::fs::read_to_string(dir.join("src").join("output").join("core/config.py"))
                .await
                .unwrap();
        assert!(contents.contains("NONE = \"none\""));
    }
}
