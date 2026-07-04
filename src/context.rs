use std::path::PathBuf;

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
}
