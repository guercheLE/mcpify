use anyhow::Result;

use crate::context::GeneratorContext;
use crate::targets::rust::context::RsTemplateContext;
use crate::targets::rust::emit::render_and_write;
use crate::targets::rust::render::render_engine;

/// Terminal Client + Harness Server entry points and stdio/HTTP transport
/// wiring (architecture.md §1, step 8). mcpify always emits both stdio and
/// HTTP capability; transport *selection* is runtime config (§2.2's
/// cascade), not a generation-time decision. Mirrors
/// `targets::typescript::steps::transports`'s `FILES` list, minus
/// `cli/setup-wizard`'s equivalent (`cli/setup_wizard.rs` — Story R7 owns
/// that file; this step only declares the module in `cli/mod.rs.tera`).
const FILES: &[(&str, &str)] = &[
    ("main.rs.tera", "src/main.rs"),
    ("cli/mod.rs.tera", "src/cli/mod.rs"),
    ("cli/setup.rs.tera", "src/cli/setup.rs"),
    ("cli/search.rs.tera", "src/cli/search.rs"),
    ("cli/get.rs.tera", "src/cli/get.rs"),
    ("cli/call.rs.tera", "src/cli/call.rs"),
    ("cli/test_connection.rs.tera", "src/cli/test_connection.rs"),
    ("cli/config.rs.tera", "src/cli/config.rs"),
    ("cli/version.rs.tera", "src/cli/version.rs"),
    ("http/mod.rs.tera", "src/http/mod.rs"),
    ("http/types.rs.tera", "src/http/types.rs"),
    (
        "http/localhost_detector.rs.tera",
        "src/http/localhost_detector.rs",
    ),
    ("http/auth_extractor.rs.tera", "src/http/auth_extractor.rs"),
    ("http/metrics.rs.tera", "src/http/metrics.rs"),
    ("http/server.rs.tera", "src/http/server.rs"),
];

/// `generate_transports_and_roles` (architecture.md §1, step 8).
pub async fn generate_transports_and_roles(ctx: &GeneratorContext) -> Result<()> {
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

    fn ctx(output_dir: PathBuf) -> GeneratorContext {
        GeneratorContext {
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
    async fn main_rs_dispatches_all_eight_subcommands() {
        let dir = tempfile::tempdir().unwrap();
        generate_transports_and_roles(&ctx(dir.path().to_path_buf()))
            .await
            .unwrap();

        let main_rs = tokio::fs::read_to_string(dir.path().join("src/main.rs"))
            .await
            .unwrap();
        for variant in [
            "Setup",
            "Search {",
            "Get {",
            "Call {",
            "Start",
            "Http {",
            "TestConnection",
            "Config",
            "Version",
        ] {
            assert!(main_rs.contains(variant), "main.rs must declare {variant}");
        }
    }
}
