use anyhow::Result;

use crate::context::GeneratorContext;
use crate::targets::csharp::context::CsTemplateContext;
use crate::targets::csharp::emit::render_and_write;
use crate::targets::csharp::render::render_engine;

/// Files rendered under `Core/` — one DI-registered service per concern
/// (architecture.md §1, step 6), each implementing one of the
/// `static partial void AddXxx` methods `Core/ServiceCollectionExtensions.cs`
/// (Story C2) declared. Mirrors `targets::python::steps::enterprise`'s
/// `CORE_FILES` list, with `Config`, `CircuitBreaker`, `CredentialStorage`,
/// and `HealthChecks` folding several of Python's/Rust's separate
/// concerns (config_schema+config_manager, circuit_breaker+rate_limiter,
/// credential_storage+cache) into fewer, first-party-package-backed
/// files — per this story's own goal text, C#'s DI-first ecosystem makes
/// several of those hand-rolled modules unnecessary.
const CORE_FILES: &[(&str, &str)] = &[
    ("Core/Errors.cs.tera", "Core/Errors.cs"),
    ("Core/Logging.cs.tera", "Core/Logging.cs"),
    ("Core/Tracing.cs.tera", "Core/Tracing.cs"),
    ("Core/Config.cs.tera", "Core/Config.cs"),
    ("Core/CircuitBreaker.cs.tera", "Core/CircuitBreaker.cs"),
    (
        "Core/CredentialStorage.cs.tera",
        "Core/CredentialStorage.cs",
    ),
    ("Core/HealthChecks.cs.tera", "Core/HealthChecks.cs"),
    ("Core/McpServer.cs.tera", "Core/McpServer.cs"),
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

/// `generate_enterprise_scaffolding` (architecture.md §1, step 6). Also
/// re-renders `Program.cs` from the (by now config-cascade-aware) template
/// bootstrap_project (Story C2) already wrote once — there is exactly one
/// `Program.cs.tera` authored across every story that touches it, so
/// re-rendering here just picks up whatever that template currently says
/// rather than requiring incremental in-place patching of generated output.
pub async fn generate_enterprise_scaffolding(ctx: &GeneratorContext) -> Result<()> {
    let view = CsTemplateContext::from_context(ctx);
    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in CORE_FILES {
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }

    for (template, out_name) in ROOT_FILES {
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }

    render_and_write(
        &tera,
        "Program.cs.tera",
        &tera_ctx,
        &ctx.output_dir.join("Program.cs"),
    )
    .await?;

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
    async fn program_cs_still_renders_after_enterprise_scaffolding() {
        // Program.cs.tera is a shared template every story that touches it
        // re-renders through (bootstrap_project actually owns writing it;
        // this just confirms this step doesn't corrupt/skip it). Story
        // C5's `targets::csharp::steps::transports` tests own asserting on
        // Program.cs's actual dual-role content.
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_enterprise_scaffolding(&ctx).await.unwrap();

        assert!(dir.join("Program.cs").is_file());
    }

    #[tokio::test]
    async fn config_auth_method_enum_has_two_members_for_two_schemes() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(
            dir.clone(),
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

        let contents = tokio::fs::read_to_string(dir.join("Core").join("Config.cs"))
            .await
            .unwrap();
        assert!(contents.contains("Basic,"));
        assert!(contents.contains("OAuth2,"));
        assert!(!contents.contains("Pat,"));
        assert!(!contents.contains("OAuth1,"));
        assert!(!contents.contains("None,"));
    }

    #[tokio::test]
    async fn config_auth_method_enum_falls_back_to_none_with_no_schemes() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_enterprise_scaffolding(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.join("Core").join("Config.cs"))
            .await
            .unwrap();
        assert!(contents.contains("None,"));
    }
}
