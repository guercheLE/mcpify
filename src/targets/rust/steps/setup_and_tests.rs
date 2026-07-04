use anyhow::Result;

use crate::context::GeneratorContext;
use crate::targets::rust::context::RsTemplateContext;
use crate::targets::rust::emit::render_and_write;
use crate::targets::rust::render::render_engine;

const FILES: &[(&str, &str)] = &[
    ("cli/setup_wizard.rs.tera", "src/cli/setup_wizard.rs"),
    ("scripts/coverage.sh.tera", "scripts/coverage.sh"),
    ("scripts/profile.sh.tera", "scripts/profile.sh"),
    (
        "scripts/samply_to_text.py.tera",
        "scripts/samply_to_text.py",
    ),
];

/// `generate_setup_wizard_and_tests` (architecture.md §1, step 10): the
/// interactive `setup` command. Unlike `targets::typescript`'s Story 13
/// (which conditionally emits separate `tests/unit/auth/*.test.ts` files
/// per discovered scheme, since vitest needs standalone test files), this
/// target's "generated test suite" is the inline `#[cfg(test)] mod tests`
/// block every core/auth/data/services/tools module already carries —
/// this whole codebase's own convention (see e.g. `steps::auth`'s doc
/// comment). Those are already scoped to discovered schemes by
/// construction (Story R4 only emits a strategy's `.rs` file — inline
/// tests included — for kinds actually present in the spec), so there's
/// nothing left for this step to conditionally emit.
pub async fn generate_setup_wizard_and_tests(ctx: &GeneratorContext) -> Result<()> {
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

    #[tokio::test]
    async fn writes_the_setup_wizard() {
        let dir = tempfile::tempdir().unwrap();
        generate_setup_wizard_and_tests(&ctx(dir.path().to_path_buf()))
            .await
            .unwrap();

        for (_, out_name) in FILES {
            assert!(dir.path().join(out_name).is_file(), "missing {out_name}");
        }
    }
}
