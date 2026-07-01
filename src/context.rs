use std::path::PathBuf;

use crate::auth_profile::AuthSchemeDescriptor;

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
    /// Discovered from `components.securitySchemes`.
    pub auth_schemes: Vec<AuthSchemeDescriptor>,
}
