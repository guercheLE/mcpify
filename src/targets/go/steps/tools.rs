use anyhow::Result;

use crate::context::GeneratorContext;

/// Story G6: `data/store.go`, `services/embedding.go` (wrapping
/// `all-minilm-l6-v2-go`), `services/vectorstore.go` (wrapping
/// `chromem-go`), `services/apiclient.go`, `tools/{search,get,call}.go`,
/// and `validation/validator.go`. Stubbed in G1.
pub async fn generate_mcp_tools(_ctx: &GeneratorContext) -> Result<()> {
    Ok(())
}
