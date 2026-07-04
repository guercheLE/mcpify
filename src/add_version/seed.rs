use anyhow::Result;

use super::ledger::{self, Ledger, VersionEntry};
use crate::context::GeneratorContext;
use crate::db::STORE_FILE_NAME;

/// Writes a project's initial version ledger right after a successful
/// `generate` run, with one entry for the spec just ingested (labeled
/// `version_label`, which defaults to `cli::DEFAULT_VERSION_LABEL` unless
/// `--version` was passed) — so `add-version` has something to extend
/// later, and a Bamboo-style project that never calls `add-version` simply
/// carries this one small, harmless extra file.
///
/// The per-target "generated schemas" relative path is derived from that
/// target's own template-context struct (already computed once during
/// `execute()`) rather than duplicated here, so it can never drift out of
/// sync with what each target's own generation step actually wrote.
pub async fn seed_ledger_after_generate(
    ctx: &GeneratorContext,
    language: &str,
    version_label: &str,
) -> Result<()> {
    let (project_name, schemas_relative) = target_naming(ctx, language)?;

    let mut ledger = Ledger::new(language, ctx.api_title.clone(), project_name);
    ledger.default_version = version_label.to_string();
    ledger.versions.insert(
        version_label.to_string(),
        VersionEntry {
            db_file: STORE_FILE_NAME.to_string(),
            schemas_file: schemas_relative,
            source: ctx.openapi_input.clone(),
            added_at: ledger::now_unix(),
        },
    );

    ledger::write(&ctx.output_dir, &ledger).await
}

fn target_naming(ctx: &GeneratorContext, language: &str) -> Result<(String, String)> {
    Ok(match language {
        "typescript" => {
            let view = crate::targets::typescript::context::TsTemplateContext::from_context(ctx);
            (
                view.project_name,
                crate::targets::typescript::steps::tools::GENERATED_SCHEMAS_PATH.to_string(),
            )
        }
        "rust" => {
            let view = crate::targets::rust::context::RsTemplateContext::from_context(ctx);
            (
                view.project_name,
                crate::targets::rust::steps::tools::GENERATED_SCHEMAS_PATH.to_string(),
            )
        }
        "python" => {
            let view = crate::targets::python::context::PyTemplateContext::from_context(ctx);
            let schemas_relative = format!(
                "src/{}/{}",
                view.module_name,
                crate::targets::python::steps::tools::GENERATED_SCHEMAS_RELATIVE_PATH
            );
            (view.project_name, schemas_relative)
        }
        "csharp" => {
            let view = crate::targets::csharp::context::CsTemplateContext::from_context(ctx);
            (
                view.project_name,
                crate::targets::csharp::steps::tools::GENERATED_SCHEMAS_PATH.to_string(),
            )
        }
        "go" => {
            let view = crate::targets::go::context::GoTemplateContext::from_context(ctx);
            (
                view.project_name,
                crate::targets::go::steps::tools::GENERATED_SCHEMAS_RELATIVE_PATH.to_string(),
            )
        }
        other => {
            anyhow::bail!("no version-ledger seeding support registered for language '{other}'")
        }
    })
}
