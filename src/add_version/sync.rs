use std::path::Path;

use anyhow::Result;

use super::ledger::Ledger;
use crate::context::VersionEntryView;

/// Re-renders each target's small set of version-aware code regions
/// (marker-delimited, see [`super::marker_region`]) so they reflect an
/// updated ledger — called once per `add-version` invocation, after the
/// ledger itself has been updated in memory but before it's written to
/// disk. Dispatches on the ledger's own recorded `language` (not a CLI
/// flag — `add-version` never asks for `--language`, since the project
/// was already generated in one language and must not risk a mismatch).
pub async fn sync_versions(project_dir: &Path, ledger: &Ledger) -> Result<()> {
    match ledger.language.as_str() {
        "typescript" => {
            crate::targets::typescript::steps::versions::sync(project_dir, ledger).await
        }
        "rust" => {
            crate::targets::rust::steps::versions::sync(project_dir, ledger).await?;
            // Unlike Go's hand-rendered bodies (already gofmt-compliant
            // by construction), Rust's rewrap long tuple entries once
            // they cross rustfmt's line-length heuristics — replicating
            // that reflow logic by hand would be fragile, so just run
            // the formatter for real, exactly like `run_tests.rs` does
            // after a full `generate`. Otherwise a long version label or
            // `.db` filename left the project's own `cargo fmt --check`
            // red in CI after every `add-version`/`remove-version`.
            crate::targets::rust::steps::run_tests::run_cargo_command(
                project_dir,
                &["fmt"],
                "cargo fmt",
            )
            .await
        }
        "python" => crate::targets::python::steps::versions::sync(project_dir, ledger).await,
        "csharp" => crate::targets::csharp::steps::versions::sync(project_dir, ledger).await,
        "go" => crate::targets::go::steps::versions::sync(project_dir, ledger).await,
        other => anyhow::bail!("no version-sync support registered for language '{other}'"),
    }
}

/// Builds the template-facing version list every target's `sync` needs,
/// straight from the ledger — in the ledger's own insertion order, matching
/// the order these versions were added over time.
pub fn version_entries_from_ledger(ledger: &Ledger) -> Vec<VersionEntryView> {
    ledger
        .versions
        .iter()
        .map(|(label, entry)| {
            VersionEntryView::from_project_relative_paths(
                label,
                &entry.db_file,
                &entry.schemas_file,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::add_version::ledger::{Ledger as TestLedger, VersionEntry};

    #[test]
    fn builds_version_entries_in_ledger_insertion_order() {
        let mut ledger = TestLedger::new("typescript", "Widget API", "widget-mcp");
        ledger.default_version = "11.3".to_string();
        ledger.versions.insert(
            "11.3".to_string(),
            VersionEntry {
                db_file: "mcp_store.db".to_string(),
                schemas_file: "src/validation/generated-schemas.json".to_string(),
                source: "spec.yaml".to_string(),
                added_at: 0,
            },
        );
        ledger.versions.insert(
            "11.2".to_string(),
            VersionEntry {
                db_file: "mcp_store_v11.2.db".to_string(),
                schemas_file: "src/validation/generated-schemas_v11.2.json".to_string(),
                source: "spec.yaml".to_string(),
                added_at: 1,
            },
        );

        let entries = version_entries_from_ledger(&ledger);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].label, "11.3");
        assert_eq!(entries[0].db_file, "mcp_store.db");
        assert_eq!(entries[0].schemas_file, "generated-schemas.json");
        assert_eq!(entries[1].label, "11.2");
        assert_eq!(entries[1].schemas_file, "generated-schemas_v11.2.json");
    }
}
