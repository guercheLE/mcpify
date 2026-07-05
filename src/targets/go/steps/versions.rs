use std::path::Path;

use anyhow::Result;

use crate::add_version::ledger::Ledger;
use crate::add_version::marker_region::patch_marked_region;
use crate::add_version::sync::version_entries_from_ledger;
use crate::context::VersionEntryView;

const CONFIG_PATH: &str = "internal/core/config.go";
const STORE_PATH: &str = "internal/data/store.go";
const VALIDATOR_PATH: &str = "internal/validation/validator.go";
const SETUP_PATH: &str = "internal/cli/setup.go";
const VERSIONS_PATH: &str = "internal/cli/versions.go";

/// Re-renders every version-aware, marker-delimited region in an
/// already-generated Go project to reflect an updated ledger — the
/// Go-specific half of `add_version::sync::sync_versions`. Deliberately
/// touches only these 5 files: auth strategies, enterprise scaffolding,
/// transports, and tests are all version-independent and are never
/// re-rendered by `add-version`.
///
/// `validator.go`'s schemas are baked into the compiled binary via one
/// `//go:embed` directive per version, so a `go build` is required after
/// `add-version` for a newly added version's schemas to actually take
/// effect — same as Rust/C#.
pub async fn sync(project_dir: &Path, ledger: &Ledger) -> Result<()> {
    let entries = version_entries_from_ledger(ledger);
    let default_label = &ledger.default_version;

    patch_marked_region(&project_dir.join(CONFIG_PATH), &config_body(default_label)).await?;
    patch_marked_region(&project_dir.join(STORE_PATH), &store_body(&entries)).await?;
    patch_marked_region(&project_dir.join(VALIDATOR_PATH), &validator_body(&entries)).await?;
    patch_marked_region(
        &project_dir.join(SETUP_PATH),
        &setup_body(&entries, default_label),
    )
    .await?;
    patch_marked_region(
        &project_dir.join(VERSIONS_PATH),
        &versions_body(&entries, default_label),
    )
    .await?;

    Ok(())
}

// A trailing blank line on every body below (in addition to whatever
// `patch_marked_region` itself adds after the body) matches what gofmt
// wants immediately before the `mcpify:versions:end` marker comment that
// follows — without it, `gofmt -l` flags the file as needing reformatting
// even though the code is semantically identical.

fn config_body(default_label: &str) -> String {
    format!("func defaultAPIVersion() string {{ return \"{default_label}\" }}\n\n")
}

fn store_body(entries: &[VersionEntryView]) -> String {
    let mut body = String::from("var versionStoreFiles = map[string]string{\n");
    for entry in entries {
        body.push_str(&format!("\t\"{}\": \"{}\",\n", entry.label, entry.db_file));
    }
    body.push_str("}\n\n");
    body
}

/// Mirrors `validator.go.tera`'s marker region: one `//go:embed` directive
/// (each binds to exactly one var, so multiple versions need multiple
/// embed+var pairs, not a shared one) per version, followed by the map
/// tying each version label back to its embedded bytes. Starts with a
/// blank line so the `//go:embed` directive isn't directly adjacent to the
/// `mcpify:versions:begin` marker comment above it — gofmt otherwise
/// treats them as one comment group and rewrites it to force that
/// separation itself.
fn validator_body(entries: &[VersionEntryView]) -> String {
    let mut body = String::from("\n");
    for entry in entries {
        body.push_str(&format!(
            "//go:embed {}\nvar embeddedSchemas_{} []byte\n\n",
            entry.schemas_file, entry.var_suffix
        ));
    }
    body.push_str("var generatedSchemasByVersion = map[string][]byte{\n");
    for entry in entries {
        body.push_str(&format!(
            "\t\"{}\": embeddedSchemas_{},\n",
            entry.label, entry.var_suffix
        ));
    }
    body.push_str("}\n\n");
    body
}

/// Mirrors `setup.go.tera`'s `{% if version_entries | length <= 1 %}`
/// branch exactly: a single version silently returns its own label with no
/// prompt at all, and multiple versions render a `survey.Select` with the
/// default annotated in its display label.
fn setup_body(entries: &[VersionEntryView], default_label: &str) -> String {
    let mut body = String::from("func promptApiVersion() string {\n");
    if entries.len() <= 1 {
        let label = entries
            .first()
            .map(|e| e.label.as_str())
            .unwrap_or(default_label);
        body.push_str(&format!(
            "\t// Only one version is available — nothing to choose between.\n\treturn \"{label}\"\n"
        ));
    } else {
        body.push_str("\tchoices := map[string]string{}\n");
        for entry in entries {
            let display = if entry.label == default_label {
                format!("{} (default/latest)", entry.label)
            } else {
                entry.label.clone()
            };
            body.push_str(&format!(
                "\tchoices[\"{}\"] = \"{}\"\n",
                display, entry.label
            ));
        }
        body.push_str("\toptions := make([]string, 0, len(choices))\n");
        body.push_str("\tfor key := range choices {\n\t\toptions = append(options, key)\n\t}\n");
        body.push_str("\tsort.Strings(options)\n\n");
        body.push_str("\tvar selection string\n");
        body.push_str(
            "\t_ = survey.AskOne(&survey.Select{Message: \"API version to use:\", Options: options}, &selection)\n",
        );
        body.push_str("\treturn choices[selection]\n");
    }
    body.push_str("}\n\n");
    body
}

fn versions_body(entries: &[VersionEntryView], default_label: &str) -> String {
    let mut body = String::from("var KnownVersions = []VersionRow{\n");
    for entry in entries {
        let is_default = if entry.label == default_label {
            "true"
        } else {
            "false"
        };
        body.push_str(&format!(
            "\t{{Label: \"{}\", IsDefault: {}}},\n",
            entry.label, is_default
        ));
    }
    body.push_str("}\n\n");
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::add_version::ledger::VersionEntry;

    fn ledger_with_two_versions() -> Ledger {
        let mut ledger = Ledger::new("go", "Widget API", "widget-mcp");
        ledger.default_version = "11.3".to_string();
        ledger.versions.insert(
            "11.3".to_string(),
            VersionEntry {
                db_file: "mcp_store.db".to_string(),
                schemas_file: "internal/validation/generated_schemas.json.zst".to_string(),
                source: "spec.yaml".to_string(),
                added_at: 0,
            },
        );
        ledger.versions.insert(
            "11.2".to_string(),
            VersionEntry {
                db_file: "mcp_store_v11.2.db".to_string(),
                schemas_file: "internal/validation/generated_schemas_v11.2.json.zst".to_string(),
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
            SETUP_PATH,
            VERSIONS_PATH,
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
        assert!(config.contains("defaultAPIVersion() string { return \"11.3\" }"));

        let store = tokio::fs::read_to_string(dir.path().join(STORE_PATH))
            .await
            .unwrap();
        assert!(store.contains("\"11.3\": \"mcp_store.db\""));
        assert!(store.contains("\"11.2\": \"mcp_store_v11.2.db\""));

        let validator = tokio::fs::read_to_string(dir.path().join(VALIDATOR_PATH))
            .await
            .unwrap();
        assert!(validator.contains("//go:embed generated_schemas.json.zst"));
        assert!(validator.contains("var embeddedSchemas_v11_3 []byte"));
        assert!(validator.contains("//go:embed generated_schemas_v11.2.json.zst"));
        assert!(validator.contains("var embeddedSchemas_v11_2 []byte"));
        assert!(validator.contains("\"11.3\": embeddedSchemas_v11_3"));
        assert!(validator.contains("\"11.2\": embeddedSchemas_v11_2"));

        let setup = tokio::fs::read_to_string(dir.path().join(SETUP_PATH))
            .await
            .unwrap();
        assert!(setup.contains("survey.Select"));
        assert!(setup.contains("11.3 (default/latest)"));

        let versions = tokio::fs::read_to_string(dir.path().join(VERSIONS_PATH))
            .await
            .unwrap();
        assert!(versions.contains("{Label: \"11.3\", IsDefault: true}"));
        assert!(versions.contains("{Label: \"11.2\", IsDefault: false}"));
    }

    #[test]
    fn setup_body_skips_the_prompt_for_a_single_version() {
        let entries = vec![VersionEntryView::from_project_relative_paths(
            "default",
            "mcp_store.db",
            "internal/validation/generated_schemas.json.zst",
        )];
        let body = setup_body(&entries, "default");
        assert!(body.contains("return \"default\""));
        assert!(!body.contains("survey.Select"));
    }
}
