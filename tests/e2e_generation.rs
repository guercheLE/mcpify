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
use mcpify::targets::csharp::CSharpTargetGenerator;
use mcpify::targets::go::GoTargetGenerator;
use mcpify::targets::python::PythonTargetGenerator;
use mcpify::targets::rust::RustTargetGenerator;
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

/// Story R8's analogous acceptance test for the Rust target: runs the full
/// `RustTargetGenerator::execute()` lifecycle — including
/// `run_generated_tests`' actual `cargo run --bin populate-embeddings`
/// (with the `fastembed` model download) and `cargo test` — against the
/// same fixture spec used for the TypeScript e2e test above, for direct
/// comparability. Requires a Rust toolchain (already present, since
/// mcpify itself is Rust — see the plan's CI notes) and network access,
/// so it's ignored by default; run explicitly with:
///
/// ```sh
/// cargo test --test e2e_generation -- --ignored
/// ```
#[tokio::test]
#[ignore]
async fn generates_a_rust_project_and_passes_its_own_test_suite() {
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

    RustTargetGenerator
        .execute(&ctx)
        .await
        .expect("execute() must succeed, including run_generated_tests' real cargo test");
}

/// Story P8's analogous acceptance test for the Python target: runs the
/// full `PythonTargetGenerator::execute()` lifecycle — including
/// `run_generated_tests`' actual `uv sync` and `uv run pytest` (with the
/// `sentence-transformers` `all-mpnet-base-v2` model download during
/// `populate_embeddings`) — against the same fixture spec used for the
/// TypeScript/Rust e2e tests above, for direct comparability. Requires
/// `uv` (already present in CI alongside the Rust/Node toolchains — see
/// the plan's CI notes) and network access, so it's ignored by default;
/// run explicitly with:
///
/// ```sh
/// cargo test --test e2e_generation -- --ignored
/// ```
#[tokio::test]
#[ignore]
async fn generates_a_python_project_and_passes_its_own_test_suite() {
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

    PythonTargetGenerator
        .execute(&ctx)
        .await
        .expect("execute() must succeed, including run_generated_tests' real uv sync/pytest");
}

/// Story C8's analogous acceptance test for the C# target — this plan's
/// "real gate": runs the full `CSharpTargetGenerator::execute()`
/// lifecycle — including `run_generated_tests`' actual `dotnet restore`,
/// `dotnet format`, and `dotnet test` (which compiles both the main
/// project and `Tests/` as a prerequisite, so this one command proves
/// build correctness and functional correctness together, per PRD
/// REQ-2.5.1) — against the same fixture spec used for the TypeScript/
/// Rust/Python e2e tests above, for direct comparability. Requires a
/// .NET SDK (net10.0) and network access (NuGet restore), so it's
/// ignored by default; run explicitly with:
///
/// ```sh
/// cargo test --test e2e_generation -- --ignored
/// ```
#[tokio::test]
#[ignore]
async fn generates_a_csharp_project_and_passes_its_own_test_suite() {
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

    CSharpTargetGenerator
        .execute(&ctx)
        .await
        .expect("execute() must succeed, including run_generated_tests' real dotnet test");
}

/// Story G8's analogous acceptance test for the Go target — this plan's
/// "real gate": runs the full `GoTargetGenerator::execute()` lifecycle —
/// including `run_generated_tests`' actual `go mod tidy`, `go build
/// ./...`, `go run ./cmd/populate-embeddings` (the real `~90MB`
/// `Xenova/all-MiniLM-L6-v2` ONNX model download plus a real ONNX Runtime
/// inference session), and `go test -tags=integration ./...` — against
/// the same fixture spec used for the TypeScript/Rust/Python/C# e2e tests
/// above, for direct comparability. Requires a Go toolchain, network
/// access, and the `ONNXRUNTIME_SHARED_LIBRARY_PATH` env var pointing at
/// a real ONNX Runtime shared library for your platform (see
/// v5-implementation-plan.md's open decision #2), so it's ignored by
/// default; run explicitly with:
///
/// ```sh
/// ONNXRUNTIME_SHARED_LIBRARY_PATH=/path/to/libonnxruntime.dylib \
///     cargo test --test e2e_generation -- --ignored
/// ```
#[tokio::test]
#[ignore]
async fn generates_a_go_project_and_passes_its_own_test_suite() {
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

    GoTargetGenerator
        .execute(&ctx)
        .await
        .expect("execute() must succeed, including run_generated_tests' real go test");
}
