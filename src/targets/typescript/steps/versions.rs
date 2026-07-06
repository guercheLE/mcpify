use std::path::Path;

use anyhow::Result;

use crate::add_version::ledger::Ledger;
use crate::add_version::marker_region::patch_marked_region;
use crate::add_version::sync::version_entries_from_ledger;
use crate::context::VersionEntryView;

const CONFIG_SCHEMA_PATH: &str = "src/core/config-schema.ts";
const STORE_REPOSITORY_PATH: &str = "src/data/store-repository.ts";
const VALIDATOR_PATH: &str = "src/validation/validator.ts";
const SETUP_WIZARD_PATH: &str = "src/cli/setup-wizard.ts";
const VERSIONS_COMMAND_PATH: &str = "src/cli/versions-command.ts";

/// Re-renders every version-aware, marker-delimited region in an
/// already-generated TypeScript project to reflect an updated ledger —
/// the TypeScript-specific half of `add_version::sync::sync_versions`.
/// Deliberately touches only these 5 files: auth strategies, enterprise
/// scaffolding, transports, and tests are all version-independent and are
/// never re-rendered by `add-version`.
pub async fn sync(project_dir: &Path, ledger: &Ledger) -> Result<()> {
    let entries = version_entries_from_ledger(ledger);
    let default_label = &ledger.default_version;

    patch_marked_region(
        &project_dir.join(CONFIG_SCHEMA_PATH),
        &config_schema_body(&entries, default_label),
    )
    .await?;
    patch_marked_region(
        &project_dir.join(STORE_REPOSITORY_PATH),
        &store_repository_body(&entries),
    )
    .await?;
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

fn config_schema_body(entries: &[VersionEntryView], default_label: &str) -> String {
    let mut body = String::from("export const ApiVersionSchema = z.enum([\n");
    for entry in entries {
        body.push_str(&format!("  '{}',\n", entry.label));
    }
    body.push_str("]);\n");
    body.push_str(&format!(
        "export const DEFAULT_API_VERSION = '{default_label}';\n"
    ));
    body
}

fn store_repository_body(entries: &[VersionEntryView]) -> String {
    let mut body = String::from("export const VERSION_STORE_FILES: Record<string, string> = {\n");
    for entry in entries {
        body.push_str(&format!("  '{}': '{}',\n", entry.label, entry.db_file));
    }
    body.push_str("};\n");
    body
}

fn validator_body(entries: &[VersionEntryView]) -> String {
    let mut body = String::from("const VERSION_SCHEMAS_FILES: Record<string, string> = {\n");
    for entry in entries {
        body.push_str(&format!("  '{}': '{}',\n", entry.label, entry.schemas_file));
    }
    body.push_str("};\n");
    body
}

/// Mirrors `setup-wizard.ts.tera`'s `{% if version_entries | length == 1 %}`
/// branch exactly: a single version silently returns its own label with no
/// prompt at all (so a Bamboo-style project never shows this step), and
/// multiple versions render an `inquirer` list with the default pre-selected.
fn setup_wizard_body(entries: &[VersionEntryView], default_label: &str) -> String {
    let mut body = String::from("async function promptApiVersion(): Promise<ApiVersion> {\n");
    if entries.len() == 1 {
        body.push_str(&format!("  return '{}';\n", entries[0].label));
    } else {
        body.push_str(
            "  const { apiVersion } = await inquirer.prompt<{ apiVersion: ApiVersion }>([\n",
        );
        body.push_str("    {\n");
        body.push_str("      type: 'list',\n");
        body.push_str("      name: 'apiVersion',\n");
        body.push_str("      message: 'API version to use:',\n");
        body.push_str(&format!("      default: '{default_label}',\n"));
        body.push_str("      choices: [\n");
        for entry in entries {
            let suffix = if entry.label == default_label {
                " (default/latest)"
            } else {
                ""
            };
            body.push_str(&format!(
                "        {{ name: '{}{}', value: '{}' }},\n",
                entry.label, suffix, entry.label
            ));
        }
        body.push_str("      ],\n");
        body.push_str("    },\n");
        body.push_str("  ]);\n");
        body.push_str("  return apiVersion;\n");
    }
    body.push_str("}\n");
    body
}

fn versions_command_body(entries: &[VersionEntryView], default_label: &str) -> String {
    let mut body = String::from("const KNOWN_VERSIONS: VersionRow[] = [\n");
    for entry in entries {
        let is_default = entry.label == default_label;
        body.push_str(&format!(
            "  {{ label: '{}', isDefault: {} }},\n",
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
        let mut ledger = Ledger::new("typescript", "Widget API", "widget-mcp");
        ledger.default_version = "11.3".to_string();
        ledger.versions.insert(
            "11.3".to_string(),
            VersionEntry {
                db_file: "mcp_store.db".to_string(),
                schemas_file: "src/validation/generated-schemas.json.zst".to_string(),
                source: "spec.yaml".to_string(),
                added_at: 0,
            },
        );
        ledger.versions.insert(
            "11.2".to_string(),
            VersionEntry {
                db_file: "mcp_store_v11.2.db".to_string(),
                schemas_file: "src/validation/generated-schemas_v11.2.json.zst".to_string(),
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
            STORE_REPOSITORY_PATH,
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
        assert!(config_schema.contains("'11.3',"));
        assert!(config_schema.contains("'11.2',"));
        assert!(config_schema.contains("DEFAULT_API_VERSION = '11.3'"));

        let store_repo = tokio::fs::read_to_string(dir.path().join(STORE_REPOSITORY_PATH))
            .await
            .unwrap();
        assert!(store_repo.contains("'11.3': 'mcp_store.db'"));
        assert!(store_repo.contains("'11.2': 'mcp_store_v11.2.db'"));

        let validator = tokio::fs::read_to_string(dir.path().join(VALIDATOR_PATH))
            .await
            .unwrap();
        assert!(validator.contains("'11.3': 'generated-schemas.json.zst'"));
        assert!(validator.contains("'11.2': 'generated-schemas_v11.2.json.zst'"));

        let setup_wizard = tokio::fs::read_to_string(dir.path().join(SETUP_WIZARD_PATH))
            .await
            .unwrap();
        assert!(setup_wizard.contains("inquirer.prompt"));
        assert!(setup_wizard.contains("default: '11.3'"));

        let versions_command = tokio::fs::read_to_string(dir.path().join(VERSIONS_COMMAND_PATH))
            .await
            .unwrap();
        assert!(versions_command.contains("{ label: '11.3', isDefault: true }"));
        assert!(versions_command.contains("{ label: '11.2', isDefault: false }"));
    }

    #[test]
    fn setup_wizard_body_skips_the_prompt_for_a_single_version() {
        let entries = vec![VersionEntryView::from_project_relative_paths(
            "default",
            "mcp_store.db",
            "src/validation/generated-schemas.json.zst",
        )];
        let body = setup_wizard_body(&entries, "default");
        assert!(body.contains("return 'default';"));
        assert!(!body.contains("inquirer.prompt"));
    }
}
