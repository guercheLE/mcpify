use anyhow::Result;

use crate::context::GeneratorContext;
use crate::targets::typescript::context::TsTemplateContext;
use crate::targets::typescript::emit::render_and_write;
use crate::targets::typescript::render::render_engine;

/// Terminal Client + Harness Server entry points and stdio/HTTP transport
/// wiring (architecture.md §1, step 8). mcpify always emits both stdio and
/// HTTP capability; transport *selection* is runtime config (§2.2's
/// cascade), not a generation-time decision.
const FILES: &[(&str, &str)] = &[
    ("index.ts.tera", "src/index.ts"),
    ("cli.ts.tera", "src/cli.ts"),
    ("cli/setup-command.ts.tera", "src/cli/setup-command.ts"),
    ("cli/search-command.ts.tera", "src/cli/search-command.ts"),
    ("cli/get-command.ts.tera", "src/cli/get-command.ts"),
    ("cli/call-command.ts.tera", "src/cli/call-command.ts"),
    ("cli/start-command.ts.tera", "src/cli/start-command.ts"),
    ("cli/http-command.ts.tera", "src/cli/http-command.ts"),
    (
        "cli/test-connection-command.ts.tera",
        "src/cli/test-connection-command.ts",
    ),
    ("cli/config-command.ts.tera", "src/cli/config-command.ts"),
    ("cli/version-command.ts.tera", "src/cli/version-command.ts"),
    ("http/types.ts.tera", "src/http/types.ts"),
    (
        "http/localhost-detector.ts.tera",
        "src/http/localhost-detector.ts",
    ),
    ("http/auth-extractor.ts.tera", "src/http/auth-extractor.ts"),
    ("http/metrics.ts.tera", "src/http/metrics.ts"),
    ("http/http-server.ts.tera", "src/http/http-server.ts"),
];

/// `generate_transports_and_roles` (architecture.md §1, step 8).
pub async fn generate_transports_and_roles(ctx: &GeneratorContext) -> Result<()> {
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
        }
    }

    #[tokio::test]
    async fn writes_every_transport_and_role_file() {
        let dir = tempfile::tempdir().unwrap();
        generate_transports_and_roles(&ctx(dir.path().to_path_buf()))
            .await
            .unwrap();

        for (_, out_name) in FILES {
            assert!(dir.path().join(out_name).is_file(), "missing {out_name}");
        }
    }

    #[tokio::test]
    async fn cli_registers_all_nine_subcommands() {
        let dir = tempfile::tempdir().unwrap();
        generate_transports_and_roles(&ctx(dir.path().to_path_buf()))
            .await
            .unwrap();

        let cli = tokio::fs::read_to_string(dir.path().join("src/cli.ts"))
            .await
            .unwrap();
        for command in [
            "registerSetupCommand",
            "registerSearchCommand",
            "registerGetCommand",
            "registerCallCommand",
            "registerStartCommand",
            "registerHttpCommand",
            "registerTestConnectionCommand",
            "registerConfigCommand",
            "registerVersionCommand",
        ] {
            assert!(cli.contains(command), "cli.ts must register {command}");
        }
    }
}
