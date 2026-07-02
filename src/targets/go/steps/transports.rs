use anyhow::Result;

use crate::context::GeneratorContext;
use crate::targets::go::context::GoTemplateContext;
use crate::targets::go::emit::render_and_write;
use crate::targets::go::render::render_engine;

/// Files whose output path is static regardless of the spec. The dual-role
/// entry point (`cmd/<binary>/main.go`) is rendered separately below since
/// its output path depends on `view.project_name`.
const FILES: &[(&str, &str)] = &[
    (
        "internal/http/localhost.go.tera",
        "internal/http/localhost.go",
    ),
    (
        "internal/http/middleware.go.tera",
        "internal/http/middleware.go",
    ),
    ("internal/http/server.go.tera", "internal/http/server.go"),
    (
        "internal/tools/register.go.tera",
        "internal/tools/register.go",
    ),
    ("internal/cli/roles.go.tera", "internal/cli/roles.go"),
];

/// `generate_transports_and_roles` (architecture.md §1, step 8): the
/// dual-role entry point (`cmd/<binary>/main.go`, via `spf13/cobra`,
/// dispatching between Terminal Client and Harness Server), the
/// `net/http`-based HTTP transport translating v1's
/// localhost-detector/auth-extractor/metrics concerns into Go middleware
/// (open decision #4 resolved: stdlib `net/http`'s `ServeMux` handled the
/// auth-gate/CORS/metrics chain cleanly, no `chi` needed), and
/// `internal/tools/register.go` — the explicit, imperative MCP-protocol
/// tool registration Go's `mcp-go` SDK needs in place of C#'s
/// attribute/reflection-based auto-registration (Go has no attributes).
pub async fn generate_transports_and_roles(ctx: &GeneratorContext) -> Result<()> {
    let view = GoTemplateContext::from_context(ctx);
    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in FILES {
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }

    render_and_write(
        &tera,
        "cmd/main.go.tera",
        &tera_ctx,
        &ctx.output_dir
            .join("cmd")
            .join(&view.project_name)
            .join("main.go"),
    )
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::auth_profile::AuthSchemeDescriptor;

    fn ctx_with_schemes(
        output_dir: PathBuf,
        auth_schemes: Vec<AuthSchemeDescriptor>,
    ) -> GeneratorContext {
        GeneratorContext {
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
    async fn writes_every_static_file() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_transports_and_roles(&ctx).await.unwrap();

        for (_, out_name) in FILES {
            assert!(dir.join(out_name).is_file(), "missing {out_name}");
        }
    }

    #[tokio::test]
    async fn writes_main_go_under_cmd_project_name() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_transports_and_roles(&ctx).await.unwrap();

        let main_go = dir.join("cmd").join("output").join("main.go");
        assert!(main_go.is_file());
        let contents = tokio::fs::read_to_string(main_go).await.unwrap();
        assert!(contents.contains(r#"Use:   "output","#));
        assert!(contents.contains("Widget API MCP server"));
        assert!(contents.contains("cli.RunStdioHarness"));
        assert!(contents.contains("cli.RunHTTPHarness"));
    }

    #[tokio::test]
    async fn roles_go_wires_every_subcommand_entry_point() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_transports_and_roles(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.join("internal/cli/roles.go"))
            .await
            .unwrap();
        for expected in [
            "func RunStdioHarness",
            "func RunHTTPHarness",
            "func RunSearch",
            "func RunGet",
            "func RunCall",
            "func RunPopulateEmbeddings",
            "func RunSetup",
            "func RunTestConnection",
            "func PrintVersion",
            "func PrintConfig",
        ] {
            assert!(contents.contains(expected), "missing {expected}");
        }
    }
}
