//! A fast, non-ignored sanity check that the Go target's templated
//! output actually compiles — deliberately narrower than
//! `e2e_generation.rs`'s real acceptance gate: no `go test`, no
//! `populate-embeddings`, and therefore no ONNX Runtime shared library or
//! ~90MB model download needed, just `go mod tidy` (a small, fast
//! dependency-graph fetch, no bigger than any other target's npm
//! install/dotnet restore) + `gofmt -l` + `go vet` + `go build ./...`.
//! Exists because two of this target's real implementation bugs (a
//! literal `{{` in a Go composite literal parsed as a Tera expression,
//! and gofmt's column-alignment of grouped `const`/map-literal
//! declarations) were only ever caught by actually compiling generated
//! output — this test keeps that check running on every push instead of
//! relying on manual verification during development.

use std::process::Command;

use mcpify::context::GeneratorContext;
use mcpify::pipeline::run_shared_pipeline;
use mcpify::targets::go::steps::{auth, bootstrap, enterprise, setup_and_tests, tools, transports};

async fn generate(fixture: &str, output_dir: std::path::PathBuf) -> GeneratorContext {
    let ctx = run_shared_pipeline(fixture, output_dir, false, false)
        .await
        .expect("shared pipeline must succeed");
    bootstrap::bootstrap_project(&ctx)
        .await
        .expect("bootstrap_project must succeed");
    enterprise::generate_enterprise_scaffolding(&ctx)
        .await
        .expect("generate_enterprise_scaffolding must succeed");
    auth::generate_auth_strategies(&ctx)
        .await
        .expect("generate_auth_strategies must succeed");
    tools::generate_mcp_tools(&ctx)
        .await
        .expect("generate_mcp_tools must succeed");
    transports::generate_transports_and_roles(&ctx)
        .await
        .expect("generate_transports_and_roles must succeed");
    setup_and_tests::generate_setup_wizard_and_tests(&ctx)
        .await
        .expect("generate_setup_wizard_and_tests must succeed");
    ctx
}

#[tokio::test]
async fn generated_go_project_gofmt_vet_and_build_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let output_dir = dir.path().join("widget-api-mcp");
    generate(
        "tests/fixtures/openapi/minimal-multi-scheme.yaml",
        output_dir.clone(),
    )
    .await;

    let gofmt = Command::new("gofmt")
        .arg("-l")
        .arg(".")
        .current_dir(&output_dir)
        .output()
        .expect("failed to run gofmt — is Go installed?");
    assert!(
        gofmt.stdout.is_empty(),
        "gofmt found unformatted files:\n{}",
        String::from_utf8_lossy(&gofmt.stdout)
    );

    let mod_tidy = Command::new("go")
        .args(["mod", "tidy"])
        .current_dir(&output_dir)
        .output()
        .expect("failed to run go mod tidy");
    assert!(
        mod_tidy.status.success(),
        "go mod tidy failed: {}",
        String::from_utf8_lossy(&mod_tidy.stderr)
    );

    let vet = Command::new("go")
        .args(["vet", "./..."])
        .current_dir(&output_dir)
        .output()
        .expect("failed to run go vet");
    assert!(
        vet.status.success(),
        "go vet failed: {}",
        String::from_utf8_lossy(&vet.stderr)
    );

    let build = Command::new("go")
        .args(["build", "./..."])
        .current_dir(&output_dir)
        .output()
        .expect("failed to run go build");
    assert!(
        build.status.success(),
        "go build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );
}
