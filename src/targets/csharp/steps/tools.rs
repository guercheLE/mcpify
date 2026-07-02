use anyhow::Result;

use crate::context::GeneratorContext;

/// Story C6: emits `search`/`get`/`call`, the `Microsoft.Data.Sqlite` +
/// sqlite-vec repository service, the `HttpClient`-based API client, the
/// `JsonSchema.Net`-based validation, and the embedding service (DI-shared
/// between the `search` tool and the populate-embeddings step). Stubbed in
/// Story C1 pending that story.
pub async fn generate_mcp_tools(_ctx: &GeneratorContext) -> Result<()> {
    Ok(())
}
