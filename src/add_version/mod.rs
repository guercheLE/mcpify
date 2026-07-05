pub mod demote;
pub mod ledger;
pub mod marker_region;
pub mod seed;
pub mod sync;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::openapi::{self, NormalizedOperation};
use ledger::{Ledger, VersionEntry};

/// Everything `add-version` needs beyond what's already recorded in the
/// project's ledger. Deliberately not `GeneratorContext`: that struct's
/// `output_dir_preexisted`/`force`/`publish_registry`/`auth_schemes` fields
/// describe the full `generate` lifecycle's preconditions, none of which
/// apply here — `add-version` never touches auth strategies, enterprise
/// scaffolding, transports, or tests, and has no `--language` flag of its
/// own (it trusts the ledger's recorded `language`).
pub struct AddVersionRequest {
    pub project_dir: PathBuf,
    pub version_label: String,
    pub input: String,
    pub set_default: bool,
    pub force: bool,
}

/// Adds another OpenAPI spec version to an already-generated project
/// (v8 multi-version support). Reuses `openapi::ingest` +
/// `openapi::normalize::normalize_operations` exactly as `generate` does,
/// but does *not* run the full 7-step per-target pipeline — only the
/// version-aware slice of it: store/schema assembly at a version-specific
/// path, and re-rendering the handful of version-aware code regions
/// (`sync::sync_versions`).
pub async fn run(request: AddVersionRequest) -> Result<()> {
    if crate::progress::enabled() {
        eprintln!(
            "==> Reading project ledger at {}",
            request.project_dir.display()
        );
    }
    let mut ledger = ledger::read(&request.project_dir).await?;

    if request.version_label == ledger.default_version && !request.set_default {
        anyhow::bail!(
            "'{}' is already this project's default version — pass --set-default to refresh it, \
             or choose a different --version label",
            request.version_label
        );
    }

    if crate::progress::enabled() {
        eprintln!("==> Fetching OpenAPI spec from {}", request.input);
    }
    let doc = openapi::ingest(&request.input).await?;

    if crate::progress::enabled() {
        eprintln!("==> Normalizing operations");
    }
    let operations = openapi::normalize::normalize_operations(&doc);
    if crate::progress::enabled() {
        eprintln!("==> Found {} operations", operations.len());
    }

    if request.set_default {
        promote_to_default(&request, &mut ledger, &operations).await?;
    } else {
        add_non_default_version(&request, &mut ledger, &operations).await?;
    }

    if crate::progress::enabled() {
        eprintln!("==> Syncing version-aware code regions");
    }
    sync::sync_versions(&request.project_dir, &ledger).await?;

    if crate::progress::enabled() {
        eprintln!("==> Writing project ledger");
    }
    ledger::write(&request.project_dir, &ledger).await?;

    Ok(())
}

async fn add_non_default_version(
    request: &AddVersionRequest,
    ledger: &mut Ledger,
    operations: &[NormalizedOperation],
) -> Result<()> {
    if ledger.versions.contains_key(&request.version_label) && !request.force {
        anyhow::bail!(
            "version '{}' already exists in this project — pass --force to overwrite it",
            request.version_label
        );
    }

    let (db_relative, schemas_relative) = sibling_version_paths(ledger, &request.version_label)?;

    if crate::progress::enabled() {
        eprintln!("==> Assembling {db_relative}");
    }
    crate::db::assemble_store_at(request.project_dir.join(&db_relative), true, operations).await?;
    if crate::progress::enabled() {
        eprintln!("==> Writing {schemas_relative}");
    }
    crate::schemas_asset::write_schemas_json_at(
        operations,
        &request.project_dir.join(&schemas_relative),
    )
    .await?;

    ledger.versions.insert(
        request.version_label.clone(),
        VersionEntry {
            db_file: db_relative,
            schemas_file: schemas_relative,
            source: request.input.clone(),
            added_at: ledger::now_unix(),
        },
    );

    Ok(())
}

async fn promote_to_default(
    request: &AddVersionRequest,
    ledger: &mut Ledger,
    operations: &[NormalizedOperation],
) -> Result<()> {
    let current_default = ledger
        .versions
        .get(&ledger.default_version)
        .with_context(|| {
            format!(
                "ledger's default_version '{}' has no matching entry",
                ledger.default_version
            )
        })?;
    let canonical_db = current_default.db_file.clone();
    let canonical_schemas = current_default.schemas_file.clone();

    // Demotes the outgoing default (if any) to its own suffixed files
    // *before* the new data is written, and reports any of the incoming
    // label's own pre-existing suffixed files that are now stale.
    let stale_files = demote::demote_current_default_if_needed(
        &request.project_dir,
        ledger,
        &request.version_label,
        request.force,
    )
    .await?;

    if crate::progress::enabled() {
        eprintln!("==> Assembling {canonical_db}");
    }
    crate::db::assemble_store_at(request.project_dir.join(&canonical_db), true, operations).await?;
    if crate::progress::enabled() {
        eprintln!("==> Writing {canonical_schemas}");
    }
    crate::schemas_asset::write_schemas_json_at(
        operations,
        &request.project_dir.join(&canonical_schemas),
    )
    .await?;

    // Only remove the stale files once the new data is safely on disk, so a
    // failure partway through this function never loses data.
    for stale in stale_files {
        let _ = tokio::fs::remove_file(stale).await;
    }

    ledger.versions.insert(
        request.version_label.clone(),
        VersionEntry {
            db_file: canonical_db,
            schemas_file: canonical_schemas,
            source: request.input.clone(),
            added_at: ledger::now_unix(),
        },
    );
    ledger.default_version = request.version_label.clone();

    Ok(())
}

/// Derives a new version's sibling file names from the *current default*'s
/// file names — e.g. default `mcp_store.db` + label `"11.2"` ->
/// `mcp_store_v11.2.db` — so `add-version` never needs any per-target or
/// per-project knowledge of naming/layout conventions; it only needs to
/// know where the current default's files live, which the ledger already
/// records.
fn sibling_version_paths(ledger: &Ledger, label: &str) -> Result<(String, String)> {
    let default_entry = ledger
        .versions
        .get(&ledger.default_version)
        .with_context(|| {
            format!(
                "ledger's default_version '{}' has no matching entry",
                ledger.default_version
            )
        })?;
    Ok((
        sibling_path(&default_entry.db_file, label),
        sibling_path(&default_entry.schemas_file, label),
    ))
}

pub(crate) fn sibling_path(canonical_relative_path: &str, label: &str) -> String {
    let path = Path::new(canonical_relative_path);
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    let sanitized_label = sanitize_label_for_filename(label);

    // Split at the *first* `.`, not the last — `Path::file_stem`/`extension`
    // only recognize the last one, which would turn a compound extension
    // like `generated_schemas.json.zst` into `generated_schemas.json_v<label>.zst`
    // instead of the intended `generated_schemas_v<label>.json.zst`.
    let new_file_name = match file_name.split_once('.') {
        Some((stem, ext)) => format!("{stem}_v{sanitized_label}.{ext}"),
        None => format!("{file_name}_v{sanitized_label}"),
    };
    match parent {
        Some(parent) => parent
            .join(new_file_name)
            .to_string_lossy()
            .replace('\\', "/"),
        None => new_file_name,
    }
}

/// Version labels are free-form operator input (e.g. `"11.2"`,
/// `"10.2.14"`) that end up as part of a filename — strip anything that
/// isn't alphanumeric/`.`/`-`/`_` so an unusual label can't escape the
/// project directory or produce an invalid path. The ledger's own map key
/// keeps the original, unsanitized label the operator typed; only the
/// derived *file name* is sanitized.
fn sanitize_label_for_filename(label: &str) -> String {
    label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_path_inserts_label_before_the_extension() {
        assert_eq!(sibling_path("mcp_store.db", "11.2"), "mcp_store_v11.2.db");
        assert_eq!(
            sibling_path("src/validation/generated-schemas.json", "11.2"),
            "src/validation/generated-schemas_v11.2.json"
        );
    }

    #[test]
    fn sibling_path_inserts_label_before_a_compound_extension() {
        // `Path::file_stem`/`extension` only recognize the last dot, which
        // would otherwise turn this into
        // `src/validation/generated_schemas.json_v11.2.zst`.
        assert_eq!(
            sibling_path("src/validation/generated_schemas.json.zst", "11.2"),
            "src/validation/generated_schemas_v11.2.json.zst"
        );
    }

    #[test]
    fn sibling_path_sanitizes_unusual_characters_in_the_label() {
        assert_eq!(
            sibling_path("mcp_store.db", "10.2.14/beta"),
            "mcp_store_v10.2.14_beta.db"
        );
    }

    #[tokio::test]
    async fn add_non_default_version_rejects_a_duplicate_label_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::new("typescript", "Widget API", "widget-mcp");
        ledger.default_version = "11.3".to_string();
        ledger.versions.insert(
            "11.3".to_string(),
            VersionEntry {
                db_file: "mcp_store.db".to_string(),
                schemas_file: "schemas.json".to_string(),
                source: "spec.yaml".to_string(),
                added_at: ledger::now_unix(),
            },
        );
        ledger.versions.insert(
            "11.2".to_string(),
            VersionEntry {
                db_file: "mcp_store_v11.2.db".to_string(),
                schemas_file: "schemas_v11.2.json".to_string(),
                source: "spec.yaml".to_string(),
                added_at: ledger::now_unix(),
            },
        );

        let request = AddVersionRequest {
            project_dir: dir.path().to_path_buf(),
            version_label: "11.2".to_string(),
            input: "spec.yaml".to_string(),
            set_default: false,
            force: false,
        };

        let err = add_non_default_version(&request, &mut ledger, &[])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("--force"));
    }

    /// The trickiest case flagged during design: promoting a label that
    /// was already `add-version`'d earlier as a non-default version (not
    /// a brand-new label) — its stale suffixed files must be cleaned up
    /// only *after* the new data lands at the canonical paths, and the
    /// outgoing default must still be demoted normally.
    #[tokio::test]
    async fn promote_to_default_repromotes_an_already_added_label_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("mcp_store.db"), b"11.3 data")
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("mcp_store_v11.2.db"), b"stale 11.2 data")
            .await
            .unwrap();

        let mut ledger = Ledger::new("typescript", "Widget API", "widget-mcp");
        ledger.default_version = "11.3".to_string();
        ledger.versions.insert(
            "11.3".to_string(),
            VersionEntry {
                db_file: "mcp_store.db".to_string(),
                schemas_file: "schemas.json".to_string(),
                source: "spec.yaml".to_string(),
                added_at: ledger::now_unix(),
            },
        );
        ledger.versions.insert(
            "11.2".to_string(),
            VersionEntry {
                db_file: "mcp_store_v11.2.db".to_string(),
                schemas_file: "schemas_v11.2.json".to_string(),
                source: "spec.yaml".to_string(),
                added_at: ledger::now_unix(),
            },
        );

        let request = AddVersionRequest {
            project_dir: dir.path().to_path_buf(),
            version_label: "11.2".to_string(),
            input: "tests/fixtures/openapi/minimal.yaml".to_string(),
            set_default: true,
            force: false,
        };
        let operations = Vec::new();

        promote_to_default(&request, &mut ledger, &operations)
            .await
            .unwrap();

        // 11.2 is now default, sitting at the canonical paths.
        assert_eq!(ledger.default_version, "11.2");
        assert_eq!(ledger.versions["11.2"].db_file, "mcp_store.db");
        // 11.3 (the outgoing default) was demoted normally.
        assert_eq!(ledger.versions["11.3"].db_file, "mcp_store_v11.3.db");
        assert!(dir.path().join("mcp_store_v11.3.db").is_file());
        // The old, now-superseded mcp_store_v11.2.db is gone — nothing in
        // the ledger points at it anymore, so it must not linger orphaned.
        assert!(!dir.path().join("mcp_store_v11.2.db").exists());
    }
}
