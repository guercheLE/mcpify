use anyhow::Result;

use crate::context::GeneratorContext;
use crate::targets::csharp::context::CsTemplateContext;
use crate::targets::csharp::emit::render_and_write;
use crate::targets::csharp::render::render_engine;

/// Dual-role entry point (architecture.md §1, step 8): re-renders
/// `Program.cs` from the (by now dual-role-dispatch-aware) shared
/// template, plus the `Http/` middleware and `Cli/Roles.cs` it depends
/// on. mcpify always emits both stdio and HTTP capability; transport
/// *selection* is runtime config (REQ-2.2's cascade), not a
/// generation-time decision. Mirrors
/// `targets::python::steps::transports`'s file list, scoped to what C5
/// alone can deliver — the Terminal Client's `search`/`get`/`call`/
/// `test-connection`/`setup` subcommands are Stories C6/C7's job (they
/// depend on tools/setup-wizard code this story doesn't have yet), added
/// to this same `Program.cs`/`RootCommand` incrementally.
const FILES: &[(&str, &str)] = &[
    (
        "Http/LocalhostDetector.cs.tera",
        "Http/LocalhostDetector.cs",
    ),
    ("Http/AuthExtractor.cs.tera", "Http/AuthExtractor.cs"),
    ("Http/Metrics.cs.tera", "Http/Metrics.cs"),
    (
        "Http/RequestCredentialProvider.cs.tera",
        "Http/RequestCredentialProvider.cs",
    ),
    (
        "Http/AuthGateMiddleware.cs.tera",
        "Http/AuthGateMiddleware.cs",
    ),
    (
        "Http/CorsHeaderMiddleware.cs.tera",
        "Http/CorsHeaderMiddleware.cs",
    ),
    ("Http/HttpServer.cs.tera", "Http/HttpServer.cs"),
    ("Cli/Roles.cs.tera", "Cli/Roles.cs"),
];

pub async fn generate_transports_and_roles(ctx: &GeneratorContext) -> Result<()> {
    let view = CsTemplateContext::from_context(ctx);
    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in FILES {
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

    fn ctx(output_dir: PathBuf) -> GeneratorContext {
        GeneratorContext {
            publish_registry: false,
            openapi_input: "spec.yaml".to_string(),
            output_dir,
            force: false,
            output_dir_preexisted: false,
            auth_schemes: Vec::new(),
            normalized_operations: Vec::new(),
            api_title: "Widget API".to_string(),
            version_label: "default".to_string(),
        }
    }

    fn output_dir(parent: &tempfile::TempDir) -> PathBuf {
        parent.path().join("output")
    }

    #[tokio::test]
    async fn writes_every_file() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        generate_transports_and_roles(&ctx(dir.clone()))
            .await
            .unwrap();

        for (_, out_name) in FILES {
            assert!(dir.join(out_name).is_file(), "missing {out_name}");
        }
        assert!(dir.join("Program.cs").is_file());
    }

    #[tokio::test]
    async fn program_cs_dispatches_the_four_c5_subcommands() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        generate_transports_and_roles(&ctx(dir.clone()))
            .await
            .unwrap();

        let contents = tokio::fs::read_to_string(dir.join("Program.cs"))
            .await
            .unwrap();
        for command in ["\"start\"", "\"http\"", "\"version\"", "\"config\""] {
            assert!(contents.contains(command), "missing {command} subcommand");
        }
    }

    #[tokio::test]
    async fn roles_wires_the_configuration_cascade_for_both_harness_roles() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        generate_transports_and_roles(&ctx(dir.clone()))
            .await
            .unwrap();

        let contents = tokio::fs::read_to_string(dir.join("Cli").join("Roles.cs"))
            .await
            .unwrap();
        assert!(contents.contains("AddMcpifyConfiguration(\"output\", args)"));
        assert!(contents.contains("WithStdioServerTransport"));
        assert!(contents.contains("HttpServer.Configure"));
    }
}
