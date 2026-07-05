use std::path::Path;

use anyhow::Result;

use crate::add_version::ledger::Ledger;
use crate::add_version::marker_region::patch_marked_region;
use crate::add_version::sync::version_entries_from_ledger;
use crate::context::VersionEntryView;

/// Re-renders every version-aware, marker-delimited region in an
/// already-generated Python project to reflect an updated ledger — the
/// Python-specific half of `add_version::sync::sync_versions`. Deliberately
/// touches only these 5 files: auth strategies, enterprise scaffolding,
/// transports, and tests are all version-independent and are never
/// re-rendered by `add-version`.
pub async fn sync(project_dir: &Path, ledger: &Ledger) -> Result<()> {
    let entries = version_entries_from_ledger(ledger);
    let default_label = &ledger.default_version;

    // `module_name` isn't stored on the ledger directly (only
    // `project_name`, which is pinned at `generate` time) — it's rederived
    // deterministically the same way `PyTemplateContext::from_context`
    // derives it the first time, so no ledger schema change is needed.
    let module_name = crate::targets::python::naming::snake_case(&ledger.project_name);
    let package_root = project_dir.join("src").join(module_name);

    patch_marked_region(
        &package_root.join("core/config.py"),
        &config_body(default_label),
    )
    .await?;
    patch_marked_region(&package_root.join("data/store.py"), &store_body(&entries)).await?;
    patch_marked_region(
        &package_root.join("validation/validator.py"),
        &validator_body(&entries),
    )
    .await?;
    patch_marked_region(
        &package_root.join("cli/setup_wizard.py"),
        &setup_wizard_body(&entries, default_label),
    )
    .await?;
    patch_marked_region(
        &package_root.join("cli/versions.py"),
        &versions_command_body(&entries, default_label),
    )
    .await?;

    Ok(())
}

fn config_body(default_label: &str) -> String {
    format!("api_version: str = \"{default_label}\"\n")
}

fn store_body(entries: &[VersionEntryView]) -> String {
    let mut body = String::from("_VERSION_STORE_FILES: dict[str, str] = {\n");
    for entry in entries {
        body.push_str(&format!(
            "    \"{}\": \"{}\",\n",
            entry.label, entry.db_file
        ));
    }
    body.push_str("}\n");
    body
}

fn validator_body(entries: &[VersionEntryView]) -> String {
    let mut body = String::from("_VERSION_SCHEMAS_FILES: dict[str, str] = {\n");
    for entry in entries {
        body.push_str(&format!(
            "    \"{}\": \"{}\",\n",
            entry.label, entry.schemas_file
        ));
    }
    body.push_str("}\n");
    body
}

/// Mirrors `setup_wizard.py.tera`'s `{% if version_entries | length == 1 %}`
/// branch exactly: a single version silently returns its own label with no
/// prompt at all, and multiple versions render a `questionary.select` with
/// the default pre-marked.
fn setup_wizard_body(entries: &[VersionEntryView], default_label: &str) -> String {
    let mut body = String::from("async def _prompt_api_version() -> str:\n");
    if entries.len() == 1 {
        body.push_str(&format!(
            "    # Only one version is available — nothing to choose between.\n    return \"{}\"\n",
            entries[0].label
        ));
    } else {
        body.push_str("    choices = [\n");
        for entry in entries {
            let suffix = if entry.label == default_label {
                " (default/latest)"
            } else {
                ""
            };
            body.push_str(&format!("        \"{}{}\",\n", entry.label, suffix));
        }
        body.push_str("    ]\n");
        body.push_str("    selection = await questionary.select(\"API version to use:\", choices).ask_async()\n");
        body.push_str("    return selection.split(\" \")[0]\n");
    }
    body
}

fn versions_command_body(entries: &[VersionEntryView], default_label: &str) -> String {
    let mut body = String::from("KNOWN_VERSIONS: list[VersionRow] = [\n");
    for entry in entries {
        let is_default = if entry.label == default_label {
            "True"
        } else {
            "False"
        };
        body.push_str(&format!(
            "    VersionRow(label=\"{}\", is_default={}),\n",
            entry.label, is_default
        ));
    }
    body.push_str("]\n");
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::add_version::ledger::VersionEntry;

    fn ledger_with_two_versions() -> Ledger {
        let mut ledger = Ledger::new("python", "Widget API", "widget-mcp");
        ledger.default_version = "11.3".to_string();
        ledger.versions.insert(
            "11.3".to_string(),
            VersionEntry {
                db_file: "mcp_store.db".to_string(),
                schemas_file: "src/widget_mcp/validation/generated_schemas.json.zst".to_string(),
                source: "spec.yaml".to_string(),
                added_at: 0,
            },
        );
        ledger.versions.insert(
            "11.2".to_string(),
            VersionEntry {
                db_file: "mcp_store_v11.2.db".to_string(),
                schemas_file: "src/widget_mcp/validation/generated_schemas_v11.2.json.zst"
                    .to_string(),
                source: "spec.yaml".to_string(),
                added_at: 1,
            },
        );
        ledger
    }

    async fn write_marked_file(path: &Path, initial_body: &str) {
        tokio::fs::write(
            path,
            format!(
                "# header\n# mcpify:versions:begin\n{initial_body}# mcpify:versions:end\n# footer\n"
            ),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn sync_patches_all_five_version_aware_files() {
        let dir = tempfile::tempdir().unwrap();
        let package_root = dir.path().join("src").join("widget_mcp");
        for path in [
            "core/config.py",
            "data/store.py",
            "validation/validator.py",
            "cli/setup_wizard.py",
            "cli/versions.py",
        ] {
            let full_path = package_root.join(path);
            tokio::fs::create_dir_all(full_path.parent().unwrap())
                .await
                .unwrap();
            write_marked_file(&full_path, "old body\n").await;
        }
        let ledger = ledger_with_two_versions();

        sync(dir.path(), &ledger).await.unwrap();

        let config = tokio::fs::read_to_string(package_root.join("core/config.py"))
            .await
            .unwrap();
        assert!(config.contains("api_version: str = \"11.3\""));

        let store = tokio::fs::read_to_string(package_root.join("data/store.py"))
            .await
            .unwrap();
        assert!(store.contains("\"11.3\": \"mcp_store.db\""));
        assert!(store.contains("\"11.2\": \"mcp_store_v11.2.db\""));

        let validator = tokio::fs::read_to_string(package_root.join("validation/validator.py"))
            .await
            .unwrap();
        assert!(validator.contains("\"11.3\": \"generated_schemas.json.zst\""));
        assert!(validator.contains("\"11.2\": \"generated_schemas_v11.2.json.zst\""));

        let setup_wizard = tokio::fs::read_to_string(package_root.join("cli/setup_wizard.py"))
            .await
            .unwrap();
        assert!(setup_wizard.contains("questionary.select"));
        assert!(setup_wizard.contains("11.3 (default/latest)"));

        let versions_command = tokio::fs::read_to_string(package_root.join("cli/versions.py"))
            .await
            .unwrap();
        assert!(versions_command.contains("label=\"11.3\", is_default=True"));
        assert!(versions_command.contains("label=\"11.2\", is_default=False"));
    }

    #[test]
    fn setup_wizard_body_skips_the_prompt_for_a_single_version() {
        let entries = vec![VersionEntryView::from_project_relative_paths(
            "default",
            "mcp_store.db",
            "src/widget_mcp/validation/generated_schemas.json.zst",
        )];
        let body = setup_wizard_body(&entries, "default");
        assert!(body.contains("return \"default\""));
        assert!(!body.contains("questionary.select"));
    }
}
