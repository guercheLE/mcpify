use std::path::PathBuf;

fn workflow(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github/workflows")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

#[test]
fn manual_ci_can_run_the_full_acceptance_suite() {
    let ci = workflow("ci.yml");

    assert!(ci.contains("run_full_e2e:"));
    assert!(ci.contains("if: inputs.run_full_e2e"));
    assert!(ci.contains("cargo test --locked --test e2e_generation -- --ignored"));
    assert!(ci.contains("cargo test --locked --test e2e_multi_version -- --ignored"));
}

#[test]
fn tag_workflows_verify_locked_sources_before_releasing() {
    let release = workflow("release.yml");
    let publish = workflow("publish-crate.yml");

    for contents in [&release, &publish] {
        assert!(contents.contains("cargo fmt --check"));
        assert!(contents.contains("cargo clippy --locked --all-targets -- -D warnings"));
        assert!(contents.contains("cargo test --locked"));
    }

    assert!(publish.contains("cargo package --locked"));
    assert!(publish.contains("cargo publish --locked --token"));
}

#[test]
fn perf_installs_generated_go_smoke_test_dependencies() {
    let perf = workflow("perf.yml");

    assert!(perf.contains("actions/setup-go@v6"));
    assert!(perf.contains("echo \"$(go env GOPATH)/bin\" >> \"$GITHUB_PATH\""));
}

#[test]
fn every_root_workflow_uses_the_canonical_golangci_lint_module() {
    const INSTALL: &str =
        "go install github.com/golangci/golangci-lint/v2/cmd/golangci-lint@v2.0.0";
    const INVALID_INSTALL: &str = "go install github.com/golangci-lint/v2/cmd/golangci-lint@v2.0.0";

    for name in ["ci.yml", "perf.yml", "publish-crate.yml", "release.yml"] {
        let contents = workflow(name);
        assert!(
            contents.contains(INSTALL),
            "{name} must install golangci-lint"
        );
        assert!(
            !contents.contains(INVALID_INSTALL),
            "{name} uses an invalid module path"
        );
    }
}
