use std::path::Path;

use anyhow::Result;

use crate::add_version::ledger::Ledger;
use crate::add_version::marker_region::patch_marked_region;
use crate::add_version::sync::version_entries_from_ledger;
use crate::context::VersionEntryView;

const CONFIG_SCHEMA_PATH: &str = "src/core/config_schema.rs";
const STORE_PATH: &str = "src/data/store.rs";
const VALIDATOR_PATH: &str = "src/validation/validator.rs";
const SETUP_WIZARD_PATH: &str = "src/cli/setup_wizard.rs";
const VERSIONS_COMMAND_PATH: &str = "src/cli/versions.rs";

/// Re-renders every version-aware, marker-delimited region in an
/// already-generated Rust project to reflect an updated ledger — the
/// Rust-specific half of `add_version::sync::sync_versions`. Deliberately
/// touches only these 5 files: auth strategies, enterprise scaffolding,
/// transports, and tests are all version-independent and are never
/// re-rendered by `add-version`.
///
/// Because `validator.rs`'s schemas are baked in at compile time
/// (`include_str!` per version), a `cargo build` is required after
/// `add-version` for a newly added version's schemas to actually take
/// effect — unlike TypeScript/Python, which read their schemas asset from
/// disk at runtime.
pub async fn sync(project_dir: &Path, ledger: &Ledger) -> Result<()> {
    let entries = version_entries_from_ledger(ledger);
    let default_label = &ledger.default_version;

    patch_marked_region(
        &project_dir.join(CONFIG_SCHEMA_PATH),
        &config_schema_body(default_label),
    )
    .await?;
    patch_marked_region(&project_dir.join(STORE_PATH), &store_body(&entries)).await?;
    patch_marked_region(&project_dir.join(VALIDATOR_PATH), &validator_body(&entries)).await?;
    patch_marked_region(
        &project_dir.join(SETUP_WIZARD_PATH),
        &setup_wizard_body(&entries, default_label),
    )
    .await?;
    patch_marked_region(
        &project_dir.join(VERSIONS_COMMAND_PATH),
        &versions_command_body(&entries, default_label),
    )
    .await?;

    Ok(())
}

fn config_schema_body(default_label: &str) -> String {
    format!("fn default_api_version() -> String {{\n    \"{default_label}\".to_string()\n}}\n")
}

fn store_body(entries: &[VersionEntryView]) -> String {
    let mut body = String::from("const VERSION_STORE_FILES: &[(&str, &str)] = &[\n");
    for entry in entries {
        body.push_str(&format!(
            "    (\"{}\", \"{}\"),\n",
            entry.label, entry.db_file
        ));
    }
    body.push_str("];\n");
    body
}

fn validator_body(entries: &[VersionEntryView]) -> String {
    let mut body = String::from(
        "fn schemas_zst_for(api_version: &str) -> Option<&'static [u8]> {\n    match api_version {\n",
    );
    for entry in entries {
        body.push_str(&format!(
            "        \"{}\" => Some(include_bytes!(\"{}\")),\n",
            entry.label, entry.schemas_file
        ));
    }
    body.push_str("        _ => None,\n    }\n}\n");
    body
}

/// Mirrors `setup_wizard.rs.tera`'s `{% if version_entries | length == 1 %}`
/// branch exactly: a single version silently returns its own label with no
/// prompt at all, and multiple versions render an `inquire::Select` with
/// the default pre-marked, matched back to its bare label.
fn setup_wizard_body(entries: &[VersionEntryView], default_label: &str) -> String {
    let mut body = String::from("async fn prompt_api_version() -> anyhow::Result<String> {\n");
    if entries.len() == 1 {
        body.push_str(&format!("    Ok(\"{}\".to_string())\n", entries[0].label));
    } else {
        let choice_literal = |entry: &VersionEntryView| -> String {
            if entry.label == default_label {
                format!("{} (default/latest)", entry.label)
            } else {
                entry.label.clone()
            }
        };

        body.push_str("    let choices = vec![\n");
        for entry in entries {
            body.push_str(&format!("        \"{}\",\n", choice_literal(entry)));
        }
        body.push_str("    ];\n");
        body.push_str("    let selection = tokio::task::spawn_blocking(move || {\n");
        body.push_str("        inquire::Select::new(\"API version to use:\", choices).prompt()\n");
        body.push_str("    })\n    .await??;\n\n");
        body.push_str("    Ok(match selection {\n");
        for entry in entries {
            body.push_str(&format!(
                "        \"{}\" => \"{}\".to_string(),\n",
                choice_literal(entry),
                entry.label
            ));
        }
        body.push_str("        other => other.to_string(),\n    })\n");
    }
    body.push_str("}\n");
    body
}

fn versions_command_body(entries: &[VersionEntryView], default_label: &str) -> String {
    let mut body = String::from("const KNOWN_VERSIONS: &[VersionRow] = &[\n");
    for entry in entries {
        let is_default = entry.label == default_label;
        body.push_str(&format!(
            "    VersionRow {{\n        label: \"{}\",\n        is_default: {},\n    }},\n",
            entry.label, is_default
        ));
    }
    body.push_str("];\n");
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::add_version::ledger::VersionEntry;

    fn ledger_with_two_versions() -> Ledger {
        let mut ledger = Ledger::new("rust", "Widget API", "widget-mcp");
        ledger.default_version = "11.3".to_string();
        ledger.versions.insert(
            "11.3".to_string(),
            VersionEntry {
                db_file: "mcp_store.db".to_string(),
                schemas_file: "src/validation/generated_schemas.json.zst".to_string(),
                source: "spec.yaml".to_string(),
                added_at: 0,
            },
        );
        ledger.versions.insert(
            "11.2".to_string(),
            VersionEntry {
                db_file: "mcp_store_v11.2.db".to_string(),
                schemas_file: "src/validation/generated_schemas_v11.2.json.zst".to_string(),
                source: "spec.yaml".to_string(),
                added_at: 1,
            },
        );
        ledger
    }

    async fn write_marked_file(path: &Path, initial_body: &str) {
        tokio::fs::write(
            path,
            format!("// header\n// mcpify:versions:begin\n{initial_body}// mcpify:versions:end\n// footer\n"),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn sync_patches_all_five_version_aware_files() {
        let dir = tempfile::tempdir().unwrap();
        for path in [
            CONFIG_SCHEMA_PATH,
            STORE_PATH,
            VALIDATOR_PATH,
            SETUP_WIZARD_PATH,
            VERSIONS_COMMAND_PATH,
        ] {
            let full_path = dir.path().join(path);
            tokio::fs::create_dir_all(full_path.parent().unwrap())
                .await
                .unwrap();
            write_marked_file(&full_path, "old body\n").await;
        }
        let ledger = ledger_with_two_versions();

        sync(dir.path(), &ledger).await.unwrap();

        let config_schema = tokio::fs::read_to_string(dir.path().join(CONFIG_SCHEMA_PATH))
            .await
            .unwrap();
        assert!(config_schema.contains("\"11.3\".to_string()"));

        let store = tokio::fs::read_to_string(dir.path().join(STORE_PATH))
            .await
            .unwrap();
        assert!(store.contains("(\"11.3\", \"mcp_store.db\")"));
        assert!(store.contains("(\"11.2\", \"mcp_store_v11.2.db\")"));

        let validator = tokio::fs::read_to_string(dir.path().join(VALIDATOR_PATH))
            .await
            .unwrap();
        assert!(
            validator.contains("\"11.3\" => Some(include_bytes!(\"generated_schemas.json.zst\"))")
        );
        assert!(validator.contains(
            "\"11.2\" => Some(include_bytes!(\"generated_schemas_v11.2.json.zst\"))"
        ));

        let setup_wizard = tokio::fs::read_to_string(dir.path().join(SETUP_WIZARD_PATH))
            .await
            .unwrap();
        assert!(setup_wizard.contains("inquire::Select"));
        assert!(setup_wizard.contains("11.3 (default/latest)"));

        let versions_command = tokio::fs::read_to_string(dir.path().join(VERSIONS_COMMAND_PATH))
            .await
            .unwrap();
        assert!(versions_command.contains("label: \"11.3\""));
        assert!(versions_command.contains("is_default: true"));
        assert!(versions_command.contains("label: \"11.2\""));
        assert!(versions_command.contains("is_default: false"));
    }

    #[test]
    fn setup_wizard_body_skips_the_prompt_for_a_single_version() {
        let entries = vec![VersionEntryView::from_project_relative_paths(
            "default",
            "mcp_store.db",
            "src/validation/generated_schemas.json",
        )];
        let body = setup_wizard_body(&entries, "default");
        assert!(body.contains("Ok(\"default\".to_string())"));
        assert!(!body.contains("inquire::Select"));
    }
}
