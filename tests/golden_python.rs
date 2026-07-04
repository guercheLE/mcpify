//! Golden/snapshot tests for the Python target's generation steps
//! (REQ-2.6.1) — the strongest regression guard against unintentional
//! template drift. Deliberately does NOT call `run_generated_tests`
//! (Story P8's real `uv sync`/`pytest` gate, including the embedding
//! model download, is exercised in `e2e_generation.rs`, not here): these
//! tests only assert that the *content* mcpify writes hasn't changed, and
//! stay fast/offline by skipping that step entirely.
//!
//! Review changes with `cargo insta review` (or accept them directly with
//! `INSTA_UPDATE=always cargo test --test golden_python`) after an
//! intentional template edit.

use std::path::{Path, PathBuf};

use mcpify::context::GeneratorContext;
use mcpify::pipeline::run_shared_pipeline;
use mcpify::targets::python::steps::{
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

    insta::assert_debug_snapshot!(
        "python_file_tree_single_basic_scheme",
        file_tree(&output_dir)
    );
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
        "python_file_tree_single_oauth2_scheme",
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

    insta::assert_debug_snapshot!("python_file_tree_all_four_schemes", file_tree(&output_dir));
}

/// Full-content snapshots of a curated subset of "interesting" files,
/// rather than all ~60 emitted files, to keep review signal-to-noise
/// reasonable when a template changes.
const CURATED_FILES: &[(&str, &str)] = &[
    ("pyproject_toml", "pyproject.toml"),
    ("config_py", "src/out/core/config.py"),
    ("auth_manager_py", "src/out/auth/auth_manager.py"),
    ("mcp_server_py", "src/out/core/mcp_server.py"),
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
        insta::assert_snapshot!(format!("python_{name}_single_basic_scheme"), contents);
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
        insta::assert_snapshot!(format!("python_{name}_all_four_schemes"), contents);
    }
}
