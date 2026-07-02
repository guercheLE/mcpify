//! The real, slow acceptance test (Story 14 / PRD REQ-2.5.1): runs the full
//! `TypeScriptTargetGenerator::execute()` lifecycle — including
//! `run_generated_tests`' actual `npm install` (with the
//! `@xenova/transformers` model download) and `npm test` — against a
//! fixture spec. Requires Node.js/npm and network access, so it's ignored
//! by default; run explicitly with:
//!
//! ```sh
//! cargo test --test e2e_generation -- --ignored
//! ```

use mcpify::pipeline::run_shared_pipeline;
use mcpify::targets::McpServerTargetGenerator;
use mcpify::targets::typescript::TypeScriptTargetGenerator;

#[tokio::test]
#[ignore]
async fn generates_a_project_and_passes_its_own_test_suite() {
    let dir = tempfile::tempdir().unwrap();
    let output_dir = dir.path().join("generated");

    let ctx = run_shared_pipeline(
        "tests/fixtures/openapi/minimal-with-auth.yaml",
        output_dir,
        false,
        false,
    )
    .await
    .expect("shared pipeline must succeed");

    TypeScriptTargetGenerator
        .execute(&ctx)
        .await
        .expect("execute() must succeed, including run_generated_tests' real npm install/test");
}
