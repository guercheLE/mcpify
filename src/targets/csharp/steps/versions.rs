use std::path::Path;

use anyhow::Result;

use crate::add_version::ledger::Ledger;
use crate::add_version::marker_region::patch_marked_region;
use crate::add_version::sync::version_entries_from_ledger;
use crate::context::VersionEntryView;

const CONFIG_PATH: &str = "Core/Config.cs";
const STORE_PATH: &str = "Data/SqliteVecStore.cs";
const VALIDATOR_PATH: &str = "Validation/Validator.cs";
const SETUP_WIZARD_PATH: &str = "Cli/SetupWizard.cs";
const VERSIONS_COMMAND_PATH: &str = "Cli/VersionsCommand.cs";

/// Re-renders every version-aware, marker-delimited region in an
/// already-generated C# project to reflect an updated ledger — the
/// C#-specific half of `add_version::sync::sync_versions`. Deliberately
/// touches only these 5 files: auth strategies, enterprise scaffolding,
/// transports, and tests are all version-independent and are never
/// re-rendered by `add-version`.
///
/// `Project.csproj`'s `<EmbeddedResource Include="Validation\GeneratedSchemas*.json.zst" />`
/// glob picks up a newly added version's schemas file on the next build
/// without needing this project file re-rendered — but because
/// `Validator.cs`'s schemas are still baked into the compiled assembly as
/// embedded resources, a `dotnet build` is required after `add-version`
/// for a new version to actually take effect, same as Rust/Go.
pub async fn sync(project_dir: &Path, ledger: &Ledger) -> Result<()> {
    let entries = version_entries_from_ledger(ledger);
    let default_label = &ledger.default_version;

    patch_marked_region(&project_dir.join(CONFIG_PATH), &config_body(default_label)).await?;
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

fn config_body(default_label: &str) -> String {
    format!("public string ApiVersion {{ get; set; }} = \"{default_label}\";\n")
}

fn store_body(entries: &[VersionEntryView]) -> String {
    let mut body = String::from(
        "private static readonly Dictionary<string, string> VersionStoreFiles = new()\n{\n",
    );
    for entry in entries {
        body.push_str(&format!(
            "    [\"{}\"] = \"{}\",\n",
            entry.label, entry.db_file
        ));
    }
    body.push_str("};\n");
    body
}

fn validator_body(entries: &[VersionEntryView]) -> String {
    let mut body = String::from(
        "private static readonly Dictionary<string, string> VersionSchemasFiles = new()\n{\n",
    );
    for entry in entries {
        body.push_str(&format!(
            "    [\"{}\"] = \"{}\",\n",
            entry.label, entry.schemas_file
        ));
    }
    body.push_str("};\n");
    body
}

/// Mirrors `SetupWizard.cs.tera`'s `{% if version_entries | length <= 1 %}`
/// branch exactly: a single version silently returns its own label with no
/// prompt at all, and multiple versions render a `SelectionPrompt` with
/// the default annotated in its display label.
fn setup_wizard_body(entries: &[VersionEntryView], default_label: &str) -> String {
    let mut body = String::from("private static string PromptApiVersion()\n{\n");
    if entries.len() <= 1 {
        let label = entries
            .first()
            .map(|e| e.label.as_str())
            .unwrap_or(default_label);
        body.push_str(&format!(
            "    // Only one version is available — nothing to choose between.\n    return \"{label}\";\n"
        ));
    } else {
        body.push_str("    var choices = new Dictionary<string, string>\n    {\n");
        for entry in entries {
            let display = if entry.label == default_label {
                format!("{} (default/latest)", entry.label)
            } else {
                entry.label.clone()
            };
            body.push_str(&format!(
                "        [\"{}\"] = \"{}\",\n",
                display, entry.label
            ));
        }
        body.push_str("    };\n");
        body.push_str("    var selection = AnsiConsole.Prompt(\n");
        body.push_str(
            "        new SelectionPrompt<string>().Title(\"API version to use:\").AddChoices(choices.Keys));\n",
        );
        body.push_str("    return choices[selection];\n");
    }
    body.push_str("}\n");
    body
}

fn versions_command_body(entries: &[VersionEntryView], default_label: &str) -> String {
    let mut body =
        String::from("public static readonly IReadOnlyList<VersionRow> KnownVersions =\n[\n");
    for entry in entries {
        let is_default = if entry.label == default_label {
            "true"
        } else {
            "false"
        };
        body.push_str(&format!(
            "    new VersionRow(\"{}\", {}),\n",
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
        let mut ledger = Ledger::new("csharp", "Widget API", "widget-mcp");
        ledger.default_version = "11.3".to_string();
        ledger.versions.insert(
            "11.3".to_string(),
            VersionEntry {
                db_file: "mcp_store.db".to_string(),
                schemas_file: "Validation/GeneratedSchemas.json.zst".to_string(),
                source: "spec.yaml".to_string(),
                added_at: 0,
            },
        );
        ledger.versions.insert(
            "11.2".to_string(),
            VersionEntry {
                db_file: "mcp_store_v11.2.db".to_string(),
                schemas_file: "Validation/GeneratedSchemas_v11.2.json.zst".to_string(),
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
            CONFIG_PATH,
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

        let config = tokio::fs::read_to_string(dir.path().join(CONFIG_PATH))
            .await
            .unwrap();
        assert!(config.contains("ApiVersion { get; set; } = \"11.3\""));

        let store = tokio::fs::read_to_string(dir.path().join(STORE_PATH))
            .await
            .unwrap();
        assert!(store.contains("[\"11.3\"] = \"mcp_store.db\""));
        assert!(store.contains("[\"11.2\"] = \"mcp_store_v11.2.db\""));

        let validator = tokio::fs::read_to_string(dir.path().join(VALIDATOR_PATH))
            .await
            .unwrap();
        assert!(validator.contains("[\"11.3\"] = \"GeneratedSchemas.json.zst\""));
        assert!(validator.contains("[\"11.2\"] = \"GeneratedSchemas_v11.2.json.zst\""));

        let setup_wizard = tokio::fs::read_to_string(dir.path().join(SETUP_WIZARD_PATH))
            .await
            .unwrap();
        assert!(setup_wizard.contains("SelectionPrompt"));
        assert!(setup_wizard.contains("11.3 (default/latest)"));

        let versions_command = tokio::fs::read_to_string(dir.path().join(VERSIONS_COMMAND_PATH))
            .await
            .unwrap();
        assert!(versions_command.contains("new VersionRow(\"11.3\", true)"));
        assert!(versions_command.contains("new VersionRow(\"11.2\", false)"));
    }

    #[test]
    fn setup_wizard_body_skips_the_prompt_for_a_single_version() {
        let entries = vec![VersionEntryView::from_project_relative_paths(
            "default",
            "mcp_store.db",
            "Validation/GeneratedSchemas.json.zst",
        )];
        let body = setup_wizard_body(&entries, "default");
        assert!(body.contains("return \"default\";"));
        assert!(!body.contains("SelectionPrompt"));
    }
}
