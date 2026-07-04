use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::ledger::{Ledger, VersionEntry};

/// Executes the file-level side of `add-version --set-default`: preserves
/// the outgoing default's data by moving it into its own suffixed files
/// (skipped entirely for a self-promotion, i.e. `new_label` is already the
/// default), and reports any of `new_label`'s *own* pre-existing suffixed
/// files that are now stale — because its data is about to be rewritten
/// straight to the canonical default paths — so the caller can remove them
/// only after the new data has been safely written at those canonical
/// paths (never before, so a failure partway through never loses data).
pub async fn demote_current_default_if_needed(
    project_dir: &Path,
    ledger: &mut Ledger,
    new_label: &str,
    force: bool,
) -> Result<Vec<PathBuf>> {
    let old_label = ledger.default_version.clone();
    let old_entry = ledger
        .versions
        .get(&old_label)
        .with_context(|| format!("ledger's default_version '{old_label}' has no matching entry"))?
        .clone();
    let canonical_db = old_entry.db_file.clone();
    let canonical_schemas = old_entry.schemas_file.clone();

    // If `new_label` already has its own ledger entry (it was
    // `add-version`'d earlier as a non-default version), its files sit at
    // suffixed paths distinct from the canonical ones — those become stale
    // once its data is rewritten straight to the canonical paths below.
    // (For a self-promotion, `new_label == old_label`, so this entry's
    // paths already *are* the canonical ones and nothing is reported.)
    let mut stale_files = Vec::new();
    if let Some(existing) = ledger.versions.get(new_label) {
        if existing.db_file != canonical_db {
            stale_files.push(project_dir.join(&existing.db_file));
        }
        if existing.schemas_file != canonical_schemas {
            stale_files.push(project_dir.join(&existing.schemas_file));
        }
    }

    if old_label != new_label {
        let demoted_db = super::sibling_path(&canonical_db, &old_label);
        let demoted_schemas = super::sibling_path(&canonical_schemas, &old_label);

        let db_target = project_dir.join(&demoted_db);
        let schemas_target = project_dir.join(&demoted_schemas);
        guard_against_clobber(&db_target, force).await?;
        guard_against_clobber(&schemas_target, force).await?;

        rename_if_source_exists(&project_dir.join(&canonical_db), &db_target).await?;
        rename_if_source_exists(&project_dir.join(&canonical_schemas), &schemas_target).await?;

        ledger.versions.insert(
            old_label,
            VersionEntry {
                db_file: demoted_db,
                schemas_file: demoted_schemas,
                source: old_entry.source,
                added_at: old_entry.added_at,
            },
        );
    }

    Ok(stale_files)
}

/// Refuses to overwrite a file already sitting at a demotion's target path
/// (leftover from an earlier, different `add-version` call) unless
/// `--force` is given — mirrors `dir_guard::check_output_dir`'s "non-empty
/// target requires --force" idiom, applied at file granularity.
async fn guard_against_clobber(target: &Path, force: bool) -> Result<()> {
    if !force && tokio::fs::try_exists(target).await.unwrap_or(false) {
        anyhow::bail!(
            "'{}' already exists and would be overwritten by this demotion — pass --force to overwrite it",
            target.display()
        );
    }
    Ok(())
}

async fn rename_if_source_exists(source: &Path, target: &Path) -> Result<()> {
    if tokio::fs::try_exists(source).await.unwrap_or(false) {
        tokio::fs::rename(source, target).await.with_context(|| {
            format!(
                "failed to move '{}' to '{}'",
                source.display(),
                target.display()
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::add_version::ledger::now_unix;

    fn ledger_with(default: &str, entries: &[(&str, &str, &str)]) -> Ledger {
        let mut ledger = Ledger::new("typescript", "Widget API", "widget-mcp");
        ledger.default_version = default.to_string();
        for (label, db_file, schemas_file) in entries {
            ledger.versions.insert(
                label.to_string(),
                VersionEntry {
                    db_file: db_file.to_string(),
                    schemas_file: schemas_file.to_string(),
                    source: "spec.yaml".to_string(),
                    added_at: now_unix(),
                },
            );
        }
        ledger
    }

    #[tokio::test]
    async fn demotes_the_old_default_to_suffixed_files() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("mcp_store.db"), b"v1 data")
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("schemas.json"), b"v1 schemas")
            .await
            .unwrap();
        let mut ledger = ledger_with("11.3", &[("11.3", "mcp_store.db", "schemas.json")]);

        let stale = demote_current_default_if_needed(dir.path(), &mut ledger, "11.2", false)
            .await
            .unwrap();

        assert!(stale.is_empty());
        assert!(dir.path().join("mcp_store_v11.3.db").is_file());
        assert!(dir.path().join("schemas_v11.3.json").is_file());
        assert!(!dir.path().join("mcp_store.db").exists());
        assert_eq!(ledger.versions["11.3"].db_file, "mcp_store_v11.3.db");
    }

    #[tokio::test]
    async fn self_promotion_demotes_nothing_and_reports_no_stale_files() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("mcp_store.db"), b"data")
            .await
            .unwrap();
        let mut ledger = ledger_with("11.3", &[("11.3", "mcp_store.db", "schemas.json")]);

        let stale = demote_current_default_if_needed(dir.path(), &mut ledger, "11.3", false)
            .await
            .unwrap();

        assert!(stale.is_empty());
        assert!(dir.path().join("mcp_store.db").is_file());
        assert_eq!(ledger.versions["11.3"].db_file, "mcp_store.db");
    }

    #[tokio::test]
    async fn repromoting_an_already_added_label_reports_its_old_suffixed_files_as_stale() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("mcp_store.db"), b"v11.3 data")
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("mcp_store_v11.2.db"), b"v11.2 data")
            .await
            .unwrap();
        let mut ledger = ledger_with(
            "11.3",
            &[
                ("11.3", "mcp_store.db", "schemas.json"),
                ("11.2", "mcp_store_v11.2.db", "schemas_v11.2.json"),
            ],
        );

        let stale = demote_current_default_if_needed(dir.path(), &mut ledger, "11.2", false)
            .await
            .unwrap();

        assert_eq!(stale.len(), 2);
        assert!(stale.contains(&dir.path().join("mcp_store_v11.2.db")));
        // The old default (11.3) is still demoted normally.
        assert!(dir.path().join("mcp_store_v11.3.db").is_file());
        assert_eq!(ledger.versions["11.3"].db_file, "mcp_store_v11.3.db");
    }

    #[tokio::test]
    async fn refuses_to_clobber_an_existing_demoted_file_without_force() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("mcp_store.db"), b"current default")
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("mcp_store_v11.3.db"), b"stale leftover")
            .await
            .unwrap();
        let mut ledger = ledger_with("11.3", &[("11.3", "mcp_store.db", "schemas.json")]);

        let err = demote_current_default_if_needed(dir.path(), &mut ledger, "11.2", false)
            .await
            .unwrap_err();

        assert!(err.to_string().contains("--force"));
    }

    #[tokio::test]
    async fn force_allows_clobbering_an_existing_demoted_file() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("mcp_store.db"), b"current default")
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("mcp_store_v11.3.db"), b"stale leftover")
            .await
            .unwrap();
        let mut ledger = ledger_with("11.3", &[("11.3", "mcp_store.db", "schemas.json")]);

        demote_current_default_if_needed(dir.path(), &mut ledger, "11.2", true)
            .await
            .unwrap();

        let contents = tokio::fs::read(dir.path().join("mcp_store_v11.3.db"))
            .await
            .unwrap();
        assert_eq!(contents, b"current default");
    }
}
