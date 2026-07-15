use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use tokio::process::Command;
use tokio::time::{Duration, timeout};

use crate::add_version::{self, AddVersionRequest};
use crate::openapi::fetch::load_raw;
use crate::openapi::source::classify;
use crate::pipeline::run_shared_pipeline_with_settings;
use crate::project_config::{PreprocessCommand, ProjectManifest, VersionSpec, select_versions};
use crate::targets;

pub async fn run(manifest_path: &Path) -> Result<PathBuf> {
    let manifest = ProjectManifest::read(manifest_path).await?;
    let selected = select_versions(&manifest.versions, &manifest.version_policy)?;
    let default = selected
        .iter()
        .find(|version| version.default)
        .copied()
        .context("selected versions have no default")?;
    let prepared_default = prepare_source(default).await?;
    let settings = manifest.settings();

    let ctx = run_shared_pipeline_with_settings(
        &prepared_default,
        manifest.output.clone(),
        manifest.force,
        false,
        manifest.publish_registry,
        &default.version,
        &settings,
        &manifest.auth,
    )
    .await?;
    let registry = targets::build_registry();
    let target = registry
        .get(manifest.language.as_str())
        .with_context(|| format!("no generator registered for '{}'", manifest.language))?;
    target.execute(&ctx).await?;
    add_version::seed::seed_ledger_after_generate(&ctx, &manifest.language, &default.version)
        .await?;
    cleanup_prepared_source(&prepared_default, &default.source).await;

    for version in selected.into_iter().filter(|version| !version.default) {
        let prepared = prepare_source(version).await?;
        add_version::run(AddVersionRequest {
            project_dir: manifest.output.clone(),
            version_label: version.version.clone(),
            input: prepared.clone(),
            set_default: false,
            force: manifest.force,
        })
        .await?;
        cleanup_prepared_source(&prepared, &version.source).await;
    }

    let mut ledger = crate::add_version::ledger::read(&manifest.output).await?;
    for version in &manifest.versions {
        if let Some(entry) = ledger.versions.get_mut(&version.version) {
            entry.source = version.source.clone();
        }
    }
    crate::add_version::ledger::write(&manifest.output, &ledger).await?;
    crate::package_preflight::enforce_project_limit(&ctx)?;

    let canonical = serde_yaml::to_string(&manifest).context("failed to serialize manifest")?;
    tokio::fs::write(manifest.output.join("mcpify.yaml"), canonical)
        .await
        .context("failed to write generated project's mcpify.yaml")?;
    crate::add_version::ledger::write_source_documentation(&manifest.output).await?;
    Ok(manifest.output)
}

async fn prepare_source(version: &VersionSpec) -> Result<String> {
    if version.preprocess.is_empty() {
        return Ok(version.source.clone());
    }
    let mut raw = load_raw(&classify(&version.source)).await?;
    let work_file = temporary_path(&version.version);
    for hook in &version.preprocess {
        tokio::fs::write(&work_file, &raw).await?;
        raw = run_preprocessor(hook, &work_file).await?;
    }
    tokio::fs::write(&work_file, raw).await?;
    Ok(work_file.to_string_lossy().into_owned())
}

async fn run_preprocessor(hook: &PreprocessCommand, input: &Path) -> Result<String> {
    let input_text = input.to_string_lossy();
    let mut saw_placeholder = false;
    let args = hook
        .args
        .iter()
        .map(|arg| {
            if arg.contains("{input}") {
                saw_placeholder = true;
                arg.replace("{input}", &input_text)
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>();
    let mut command = Command::new(&hook.command);
    command.args(args);
    if !saw_placeholder {
        command.arg(input);
    }
    let output = timeout(Duration::from_secs(120), command.output())
        .await
        .with_context(|| format!("preprocessor '{}' timed out after 120s", hook.command))?
        .with_context(|| format!("failed to run preprocessor '{}'", hook.command))?;
    if !output.status.success() {
        bail!(
            "preprocessor '{}' failed: {}",
            hook.command,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    if output.stdout.is_empty() {
        tokio::fs::read_to_string(input)
            .await
            .with_context(|| format!("preprocessor '{}' produced no stdout", hook.command))
    } else {
        String::from_utf8(output.stdout).context("preprocessor output was not UTF-8")
    }
}

fn temporary_path(version: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let safe = version
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    std::env::temp_dir().join(format!(
        "mcpify-preprocessed-{}-{safe}-{timestamp}.yaml",
        std::process::id()
    ))
}

async fn cleanup_prepared_source(prepared: &str, source: &str) {
    if prepared != source {
        let _ = tokio::fs::remove_file(prepared).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn preprocessor_receives_input_without_using_a_shell() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.yaml");
        tokio::fs::write(&input, "title: Legacy API\n")
            .await
            .unwrap();
        let output = run_preprocessor(
            &PreprocessCommand {
                command: "sed".to_string(),
                args: vec!["s/Legacy/Modern/".to_string(), "{input}".to_string()],
            },
            &input,
        )
        .await
        .unwrap();
        assert_eq!(output, "title: Modern API\n");
    }
}
