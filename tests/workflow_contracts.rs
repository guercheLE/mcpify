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
