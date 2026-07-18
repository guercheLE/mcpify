use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::auth_profile::AuthSchemeDescriptor;
use crate::openapi::NormalizedOperation;

/// Shared state threaded through every step of the compile-time lifecycle
/// (architecture.md §1). Populated by the shared pipeline (Story 6) before
/// any `McpServerTargetGenerator` method runs.
#[derive(Debug)]
#[allow(dead_code)] // ponytail: fields read once the shared pipeline (Story 6) populates them
pub struct GeneratorContext {
    /// Local path or URL to the source OpenAPI spec.
    pub openapi_input: String,
    pub output_dir: PathBuf,
    pub force: bool,
    /// True if `output_dir` already had content before this run (via `--force`).
    pub output_dir_preexisted: bool,
    /// v6 Part PUB: opt-in via `--publish-registry`. Gates whether
    /// Rust/Python/C#'s `release.yml`/manifest templates emit a real
    /// registry-publish step (`cargo publish`/`uv publish`/`dotnet nuget
    /// push`) instead of the default GitHub-Release-only behavior.
    pub publish_registry: bool,
    /// Discovered from `components.securitySchemes`.
    pub auth_schemes: Vec<AuthSchemeDescriptor>,
    /// Flattened out of the parsed OpenAPI doc once by the shared pipeline
    /// (Story 6) and reused by every later target step (Stories 8-13)
    /// instead of each one re-deriving it or re-querying `mcp_store.db`.
    /// A deliberate, minor extension beyond architecture.md's literal
    /// `GeneratorContext` listing — justified by avoiding repeated work.
    pub normalized_operations: Vec<NormalizedOperation>,
    /// `info.title` from the parsed OpenAPI doc, kept on the context (rather
    /// than threading the whole raw document through the target trait) so
    /// template contexts (Story 7) can derive a human-readable display name.
    pub api_title: String,
    /// v8 multi-version support: the label this `generate` run's spec is
    /// recorded under (`cli::DEFAULT_VERSION_LABEL` unless `--api-version`
    /// was passed). At `generate` time there is always exactly one version,
    /// so every target's template context renders `version_labels` as a
    /// single-element list from this one field — `add-version` later
    /// re-renders that same small region with the full, updated list
    /// without needing a `GeneratorContext` of its own.
    pub version_label: String,
}

impl GeneratorContext {
    /// `output_dir`'s last path component, for targets to slug into a
    /// project/crate/package name. Delegates to [`resolve_dir_name`] so
    /// `output: .` in a `sync`-driven manifest (the shape every previously
    /// generated project's `mcpify.yaml` uses) still resolves to the actual
    /// working directory's name rather than silently losing it.
    pub fn output_dir_name(&self) -> Option<String> {
        resolve_dir_name(&self.output_dir)
    }
}

/// The last path component of `dir`, falling back to canonicalizing it first
/// when `dir` has none of its own — notably `.` and `..`, whose only
/// component (`CurDir`/`ParentDir`) isn't a `Normal` component, so
/// `Path::file_name` returns `None` for them even though they plainly
/// resolve to a real, named directory (canonicalizing `.` yields the actual
/// working directory, e.g. `/repos/bamboo-mcp-rs`, whose basename is what
/// callers actually want). Still returns `None` for paths with no
/// meaningful basename even once resolved, e.g. `/`.
pub(crate) fn resolve_dir_name(dir: &Path) -> Option<String> {
    dir.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .or_else(|| {
            dir.canonicalize()
                .ok()?
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
}

/// One version's on-disk artifact file names, in the shape every target's
/// template context needs to render its version-aware code regions (the
/// `ApiVersion` enum, the store/schemas file-lookup maps, the setup
/// wizard's version prompt, the `versions` command's listing). Mirrors
/// `add_version::ledger::VersionEntry` plus the `label` that's the
/// ledger's map key rather than a field on `VersionEntry` itself —
/// templates need both together.
#[derive(Debug, Clone, Serialize)]
pub struct VersionEntryView {
    pub label: String,
    pub db_file: String,
    pub schemas_file: String,
    /// A version label sanitized into a valid identifier suffix (e.g.
    /// `"10.2.14"` -> `"v10_2_14"`) — only Go's target needs this (each
    /// version's `//go:embed`-backed schemas asset needs its own uniquely
    /// named package-level variable, since `go:embed` directives bind to a
    /// single var declaration each and labels like `"11.2"` aren't valid Go
    /// identifiers), but it's computed here rather than duplicated in
    /// `targets::go` so generate-time Tera rendering and `add-version`'s
    /// Rust-side re-rendering always agree on the same suffix.
    pub var_suffix: String,
}

impl VersionEntryView {
    /// Builds the template-facing view of one version. `db_file` is passed
    /// through as-is (db files always live at the project root, so it's
    /// already a bare filename). `schemas_file` is reduced to its basename:
    /// `add_version::ledger::VersionEntry::schemas_file` is project-root-relative
    /// (what Rust-side file operations need), but every target's validator
    /// resolves its schemas asset relative to *its own* directory, which is
    /// always the same directory the asset is written into alongside it.
    pub fn from_project_relative_paths(label: &str, db_file: &str, schemas_file: &str) -> Self {
        Self {
            label: label.to_string(),
            db_file: db_file.to_string(),
            schemas_file: basename(schemas_file),
            var_suffix: identifier_suffix(label),
        }
    }
}

fn basename(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string()
}

/// Sanitizes an arbitrary version label into a valid identifier suffix:
/// non-alphanumeric characters become `_`, and a leading digit (identifiers
/// can't start with one) gets a `v` prefix.
fn identifier_suffix(label: &str) -> String {
    let sanitized: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    match sanitized.chars().next() {
        Some(c) if c.is_ascii_digit() => format!("v{sanitized}"),
        _ => sanitized,
    }
}

#[cfg(test)]
mod resolve_dir_name_tests {
    use super::resolve_dir_name;
    use std::path::Path;

    #[test]
    fn returns_the_basename_of_a_named_path() {
        assert_eq!(
            resolve_dir_name(Path::new("/repos/bamboo-mcp-rs")),
            Some("bamboo-mcp-rs".to_string())
        );
    }

    #[test]
    fn resolves_a_bare_current_dir_marker_to_the_working_directory_name() {
        let expected = std::env::current_dir()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(resolve_dir_name(Path::new(".")), Some(expected));
    }

    #[test]
    fn falls_back_to_none_for_a_path_with_no_meaningful_basename() {
        assert_eq!(resolve_dir_name(Path::new("/")), None);
    }
}
