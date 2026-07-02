use anyhow::Result;

use crate::context::GeneratorContext;
use crate::targets::python::context::PyTemplateContext;
use crate::targets::python::emit::render_and_write;
use crate::targets::python::render::render_engine;

/// Rendered directly into `ctx.output_dir` rather than under the package
/// tree — the `python main.py` convenience entry point the toolchain
/// table calls out, alongside the `[project.scripts]` console script
/// (Story P2).
const ROOT_FILES: &[(&str, &str)] = &[("main.py.tera", "main.py")];

/// Terminal Client + Harness Server entry points and stdio/HTTP transport
/// wiring (architecture.md §1, step 8), rendered under
/// `src/<module_name>/`. mcpify always emits both stdio and HTTP
/// capability; transport *selection* is runtime config (§2.2's cascade),
/// not a generation-time decision. Mirrors
/// `targets::rust::steps::transports`'s `FILES` list, minus
/// `cli/setup_wizard.py`'s equivalent — Story P7 owns that file; this step
/// only imports it from `cli/setup.py`/`cli/__init__.py`.
const PACKAGE_FILES: &[(&str, &str)] = &[
    ("cli/__init__.py.tera", "cli/__init__.py"),
    ("cli/setup.py.tera", "cli/setup.py"),
    ("cli/search.py.tera", "cli/search.py"),
    ("cli/get.py.tera", "cli/get.py"),
    ("cli/call.py.tera", "cli/call.py"),
    ("cli/test_connection.py.tera", "cli/test_connection.py"),
    ("cli/config.py.tera", "cli/config.py"),
    ("cli/version.py.tera", "cli/version.py"),
    ("http/__init__.py.tera", "http/__init__.py"),
    ("http/types.py.tera", "http/types.py"),
    (
        "http/localhost_detector.py.tera",
        "http/localhost_detector.py",
    ),
    ("http/auth_extractor.py.tera", "http/auth_extractor.py"),
    ("http/metrics.py.tera", "http/metrics.py"),
    ("http/server.py.tera", "http/server.py"),
];

/// `generate_transports_and_roles` (architecture.md §1, step 8).
pub async fn generate_transports_and_roles(ctx: &GeneratorContext) -> Result<()> {
    let view = PyTemplateContext::from_context(ctx);
    let package_root = ctx.output_dir.join("src").join(&view.module_name);
    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in ROOT_FILES {
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }

    for (template, out_name) in PACKAGE_FILES {
        render_and_write(&tera, template, &tera_ctx, &package_root.join(out_name)).await?;
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

    // A named subdirectory (rather than the tempdir root, whose name is
    // random) so `module_name` — and therefore `package_root` below — is
    // deterministic.
    fn output_dir(parent: &tempfile::TempDir) -> PathBuf {
        parent.path().join("output")
    }

    #[tokio::test]
    async fn writes_every_root_file() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        generate_transports_and_roles(&ctx(dir.clone()))
            .await
            .unwrap();

        for (_, out_name) in ROOT_FILES {
            assert!(dir.join(out_name).is_file(), "missing {out_name}");
        }
    }

    #[tokio::test]
    async fn writes_every_package_file() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        generate_transports_and_roles(&ctx(dir.clone()))
            .await
            .unwrap();

        let package_root = dir.join("src").join("output");
        for (_, out_name) in PACKAGE_FILES {
            assert!(
                package_root.join(out_name).is_file(),
                "missing src/output/{out_name}"
            );
        }
    }

    #[tokio::test]
    async fn cli_dispatches_all_nine_subcommands() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        generate_transports_and_roles(&ctx(dir.clone()))
            .await
            .unwrap();

        let cli_init = tokio::fs::read_to_string(
            dir.join("src")
                .join("output")
                .join("cli")
                .join("__init__.py"),
        )
        .await
        .unwrap();
        for command in [
            "def setup",
            "def search",
            "def get",
            "def call",
            "def start",
            "def http_command",
            "def test_connection",
            "def config",
            "def version",
        ] {
            assert!(
                cli_init.contains(command),
                "cli/__init__.py must declare {command}"
            );
        }
    }
}
