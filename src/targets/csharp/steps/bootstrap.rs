use anyhow::Result;

use crate::context::GeneratorContext;
use crate::targets::csharp::context::CsTemplateContext;
use crate::targets::csharp::emit::render_and_write;
use crate::targets::csharp::render::render_engine;

const SOURCE_SUBDIRS: &[&str] = &[
    "Auth",
    "Cli",
    "Core",
    "Data",
    "Http",
    "Services",
    "Tools",
    "Validation",
];

const STATIC_FILES: &[(&str, &str)] = &[
    (".gitignore.tera", ".gitignore"),
    (".editorconfig.tera", ".editorconfig"),
    (".env.example.tera", ".env.example"),
    ("README.md.tera", "README.md"),
];

/// `bootstrap_project` (architecture.md §1, step 5): project skeleton,
/// `.csproj` manifest, and config files — everything before enterprise
/// scaffolding (Story C3). `mcp_store.db` is already written directly into
/// `ctx.output_dir` by the shared pipeline (Story 5/6), so this step only
/// lays out the C#-convention PascalCase folder skeleton and the
/// root-level project files around it, plus the skeleton
/// `AddMcpifyServices` DI registration extension every later step (C3-C6)
/// extends. Mirrors
/// `targets::python::steps::bootstrap::bootstrap_project`'s
/// folder-per-concern shape, adapted to C#'s capitalized folder/namespace
/// convention — no `__init__.py`-equivalent marker file is needed since
/// C# namespaces don't require one.
pub async fn bootstrap_project(ctx: &GeneratorContext) -> Result<()> {
    let view = CsTemplateContext::from_context(ctx);

    for subdir in SOURCE_SUBDIRS {
        tokio::fs::create_dir_all(ctx.output_dir.join(subdir)).await?;
    }

    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in STATIC_FILES {
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }

    render_and_write(
        &tera,
        "Project.csproj.tera",
        &tera_ctx,
        &ctx.output_dir.join(format!("{}.csproj", view.namespace)),
    )
    .await?;

    render_and_write(
        &tera,
        "Program.cs.tera",
        &tera_ctx,
        &ctx.output_dir.join("Program.cs"),
    )
    .await?;

    render_and_write(
        &tera,
        "Core/ServiceCollectionExtensions.cs.tera",
        &tera_ctx,
        &ctx.output_dir
            .join("Core")
            .join("ServiceCollectionExtensions.cs"),
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

    // A named subdirectory (rather than the tempdir root, whose name is
    // random) so `project_name`/`namespace`/`tool_prefix_env` are
    // deterministic.
    fn output_dir(parent: &tempfile::TempDir) -> PathBuf {
        parent.path().join("output")
    }

    #[tokio::test]
    async fn creates_every_source_subdirectory() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        bootstrap_project(&ctx).await.unwrap();

        for subdir in SOURCE_SUBDIRS {
            assert!(dir.join(subdir).is_dir(), "missing {subdir}/");
        }
    }

    #[tokio::test]
    async fn writes_every_static_file() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        bootstrap_project(&ctx).await.unwrap();

        for (_, out_name) in STATIC_FILES {
            assert!(dir.join(out_name).is_file(), "missing {out_name}");
        }
    }

    #[tokio::test]
    async fn writes_the_csproj_named_after_the_pascal_case_namespace() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        bootstrap_project(&ctx).await.unwrap();

        let csproj_path = dir.join("Output.csproj");
        assert!(csproj_path.is_file());
        let contents = tokio::fs::read_to_string(csproj_path).await.unwrap();
        assert!(contents.contains("<RootNamespace>Output</RootNamespace>"));
        assert!(contents.contains("<TargetFramework>net10.0</TargetFramework>"));
        assert!(contents.contains("ModelContextProtocol"));
    }

    #[tokio::test]
    async fn writes_program_cs_wiring_the_root_command() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        bootstrap_project(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.join("Program.cs"))
            .await
            .unwrap();
        assert!(contents.contains("using Output.Cli;"));
        assert!(contents.contains("new RootCommand("));
    }

    #[tokio::test]
    async fn writes_the_service_collection_extensions_skeleton() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        bootstrap_project(&ctx).await.unwrap();

        let path = dir.join("Core").join("ServiceCollectionExtensions.cs");
        assert!(path.is_file());
        let contents = tokio::fs::read_to_string(path).await.unwrap();
        assert!(contents.contains("namespace Output.Core;"));
        assert!(contents.contains("public static IServiceCollection AddMcpifyServices"));
        assert!(contents.contains("static partial void AddCircuitBreaker"));
    }

    #[tokio::test]
    async fn env_example_only_lists_vars_for_discovered_schemes() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(
            dir.clone(),
            vec![AuthSchemeDescriptor {
                name: "oauth2".to_string(),
                kind: AuthSchemeKind::OAuth2,
            }],
        );

        bootstrap_project(&ctx).await.unwrap();

        let env_example = tokio::fs::read_to_string(dir.join(".env.example"))
            .await
            .unwrap();
        assert!(env_example.contains("OUTPUT_CLIENT_ID="));
        assert!(env_example.contains("OUTPUT_CLIENT_SECRET="));
        assert!(!env_example.contains("_USERNAME="));
        assert!(!env_example.contains("_CONSUMER_KEY="));
    }
}
