//! Golden/snapshot tests for the Rust target's generation steps
//! (REQ-2.6.1) — the strongest regression guard against unintentional
//! template drift. Deliberately does NOT call `run_generated_tests`
//! (Story R8's real `cargo test` gate, including the embedding model
//! download, is exercised in `e2e_generation.rs`, not here): these tests
//! only assert that the *content* mcpify writes hasn't changed, and stay
//! fast/offline by skipping that step entirely.
//!
//! Review changes with `cargo insta review` (or accept them directly with
//! `INSTA_UPDATE=always cargo test --test golden_rust`) after an
//! intentional template edit.

use std::path::{Path, PathBuf};

use mcpify::context::GeneratorContext;
use mcpify::pipeline::run_shared_pipeline;
use mcpify::targets::rust::steps::{
    auth, bootstrap, enterprise, setup_and_tests, tools, transports,
};

async fn generate(fixture: &str, output_dir: PathBuf) -> GeneratorContext {
    let ctx = run_shared_pipeline(fixture, output_dir, false, false, false, "default")
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
    transports::generate_transports_and_roles(&ctx)
        .await
        .expect("generate_transports_and_roles must succeed");
    tools::generate_mcp_tools(&ctx)
        .await
        .expect("generate_mcp_tools must succeed");
    setup_and_tests::generate_setup_wizard_and_tests(&ctx)
        .await
        .expect("generate_setup_wizard_and_tests must succeed");
    ctx
}

fn collect_relative_paths(root: &Path, current: &Path, paths: &mut Vec<String>) {
    for entry in std::fs::read_dir(current).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_relative_paths(root, &path, paths);
        } else {
            let relative = path.strip_prefix(root).unwrap();
            paths.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// A cheap smoke check that always runs: the exact set of files emitted,
/// independent of their content. Catches any accidental addition/removal
/// of a generated file, including conditional-emission regressions (an
/// auth-strategy module leaking in when its scheme wasn't discovered, etc).
fn file_tree(root: &Path) -> Vec<String> {
    let mut paths = Vec::new();
    collect_relative_paths(root, root, &mut paths);
    paths.sort();
    paths
}

#[tokio::test]
async fn file_tree_single_basic_scheme() {
    let dir = tempfile::tempdir().unwrap();
    let output_dir = dir.path().join("out");
    generate(
        "tests/fixtures/openapi/minimal-with-auth.yaml",
        output_dir.clone(),
    )
    .await;

    insta::assert_debug_snapshot!("rust_file_tree_single_basic_scheme", file_tree(&output_dir));
}

#[tokio::test]
async fn file_tree_single_oauth2_scheme() {
    let dir = tempfile::tempdir().unwrap();
    let output_dir = dir.path().join("out");
    generate(
        "tests/fixtures/openapi/minimal-oauth2.json",
        output_dir.clone(),
    )
    .await;

    insta::assert_debug_snapshot!(
        "rust_file_tree_single_oauth2_scheme",
        file_tree(&output_dir)
    );
}

#[tokio::test]
async fn file_tree_all_four_schemes() {
    let dir = tempfile::tempdir().unwrap();
    let output_dir = dir.path().join("out");
    generate(
        "tests/fixtures/openapi/minimal-multi-scheme.yaml",
        output_dir.clone(),
    )
    .await;

    insta::assert_debug_snapshot!("rust_file_tree_all_four_schemes", file_tree(&output_dir));
}

/// Full-content snapshots of a curated subset of "interesting" files,
/// rather than all ~60 emitted files, to keep review signal-to-noise
/// reasonable when a template changes.
const CURATED_FILES: &[(&str, &str)] = &[
    ("cargo_toml", "Cargo.toml"),
    ("config_schema_rs", "src/core/config_schema.rs"),
    ("auth_manager_rs", "src/auth/auth_manager.rs"),
    ("mcp_server_rs", "src/core/mcp_server.rs"),
];

#[tokio::test]
async fn curated_file_contents_single_basic_scheme() {
    let dir = tempfile::tempdir().unwrap();
    let output_dir = dir.path().join("out");
    generate(
        "tests/fixtures/openapi/minimal-with-auth.yaml",
        output_dir.clone(),
    )
    .await;

    for (name, relative_path) in CURATED_FILES {
        let contents = std::fs::read_to_string(output_dir.join(relative_path))
            .unwrap_or_else(|e| panic!("failed to read {relative_path}: {e}"));
        insta::assert_snapshot!(format!("rust_{name}_single_basic_scheme"), contents);
    }
}

#[tokio::test]
async fn curated_file_contents_all_four_schemes() {
    let dir = tempfile::tempdir().unwrap();
    let output_dir = dir.path().join("out");
    generate(
        "tests/fixtures/openapi/minimal-multi-scheme.yaml",
        output_dir.clone(),
    )
    .await;

    for (name, relative_path) in CURATED_FILES {
        let contents = std::fs::read_to_string(output_dir.join(relative_path))
            .unwrap_or_else(|e| panic!("failed to read {relative_path}: {e}"));
        insta::assert_snapshot!(format!("rust_{name}_all_four_schemes"), contents);
    }
}

/// Profiling is exposed through generated commands and scripts, so assert at
/// that public generated-project seam rather than against template internals.
#[tokio::test]
async fn generated_profiling_is_self_contained_and_keeps_instrumentation_separate() {
    let dir = tempfile::tempdir().unwrap();
    let output_dir = dir.path().join("out");
    generate(
        "tests/fixtures/openapi/minimal-with-auth.yaml",
        output_dir.clone(),
    )
    .await;

    let cargo_toml = std::fs::read_to_string(output_dir.join("Cargo.toml")).unwrap();
    let readme = std::fs::read_to_string(output_dir.join("README.md")).unwrap();
    let profile_script = std::fs::read_to_string(output_dir.join("scripts/profile.sh")).unwrap();
    let heap_script = std::fs::read_to_string(output_dir.join("scripts/profile-heap.sh")).unwrap();

    assert!(cargo_toml.contains("default-run = \"out\""));

    let cpu_build = profile_script
        .lines()
        .find(|line| line.starts_with("cargo build "))
        .expect("generated profile script must build the CPU-profiled binary");
    assert_eq!(cpu_build, "cargo build --release --bin out");
    assert!(!cpu_build.contains("profiling"));
    assert!(
        profile_script
            .lines()
            .any(|line| line == "export OUT_URL=\"${OUT_URL:-http://127.0.0.1}\"")
    );
    assert!(
        profile_script
            .lines()
            .any(|line| line == "export OUT_AUTH_METHOD=\"${OUT_AUTH_METHOD:-basic}\"")
    );
    assert!(profile_script.contains("For heap profiling: bash scripts/profile-heap.sh"));
    assert!(profile_script.contains("## Coverage gaps (most missed lines)"));
    assert!(profile_script.contains("sort -nr -k1,1 | head -20"));

    assert!(heap_script.contains("cargo run --release --features profiling --bin out -- search"));
    assert!(heap_script.contains("--profile-warmups \"$profile_warmups\""));
    assert!(heap_script.contains("--profile-iterations \"$profile_iterations\""));
    assert!(heap_script.contains("PROFILE_HEAP_WARMUPS"));
    assert!(heap_script.contains("PROFILE_HEAP_ITERATIONS"));
    assert!(
        heap_script
            .lines()
            .any(|line| line == "export OUT_URL=\"${OUT_URL:-http://127.0.0.1}\"")
    );
    assert!(
        heap_script
            .lines()
            .any(|line| line == "export OUT_AUTH_METHOD=\"${OUT_AUTH_METHOD:-basic}\"")
    );
    assert!(readme.contains("bash scripts/profile-heap.sh"));
    assert!(readme.contains("CPU and heap profiling use separate builds"));
}

#[tokio::test]
async fn generated_store_uses_content_addressing_and_no_clobber_publication() {
    let dir = tempfile::tempdir().unwrap();
    let output_dir = dir.path().join("out");
    generate(
        "tests/fixtures/openapi/minimal-with-auth.yaml",
        output_dir.clone(),
    )
    .await;

    let store = std::fs::read_to_string(output_dir.join("src/data/store.rs")).unwrap();
    assert!(store.contains("Sha256::digest(bytes)"));
    assert!(store.contains("std::fs::hard_link(tmp_path, path)"));
    assert!(store.contains("if !path.is_file()"));
    assert!(!store.contains("std::fs::rename(&tmp_path, &path)"));
}
