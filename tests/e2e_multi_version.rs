//! v8 multi-version support's real, slow acceptance test: generates a
//! TypeScript project (including `run_generated_tests`' actual `npm
//! install`/`npm test`), adds two more spec versions via `add-version`
//! (one plain, one `--set-default`), then rebuilds and runs the generated
//! project's own `versions` CLI subcommand to prove the whole feature
//! works end to end — not just that mcpify's own file operations succeed.
//! Requires Node.js/npm and network access, so it's ignored by default;
//! run explicitly with:
//!
//! ```sh
//! cargo test --test e2e_multi_version -- --ignored
//! ```

use std::process::Command;

use mcpify::add_version::{self, AddVersionRequest, seed};
use mcpify::pipeline::run_shared_pipeline;
use mcpify::targets::McpServerTargetGenerator;
use mcpify::targets::typescript::TypeScriptTargetGenerator;

#[tokio::test]
#[ignore]
async fn generate_add_version_and_set_default_survive_a_real_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let output_dir = dir.path().join("generated");

    // 1. `generate` a project explicitly labeled "v1" (mirrors a real
    // operator who knows up front they'll be layering more versions on).
    let ctx = run_shared_pipeline(
        "tests/fixtures/openapi/minimal-with-auth.yaml",
        output_dir.clone(),
        false,
        false,
        false,
        "v1",
    )
    .await
    .expect("shared pipeline must succeed");

    TypeScriptTargetGenerator
        .execute(&ctx)
        .await
        .expect("execute() must succeed, including run_generated_tests' real npm install/test");

    seed::seed_ledger_after_generate(&ctx, "typescript", "v1")
        .await
        .expect("seeding the ledger after generate must succeed");

    // 2. Add a second version, plain (not default).
    add_version::run(AddVersionRequest {
        project_dir: output_dir.clone(),
        version_label: "v2".to_string(),
        input: "tests/fixtures/openapi/widgets-with-refs.yaml".to_string(),
        set_default: false,
        force: false,
    })
    .await
    .expect("add-version v2 must succeed");

    assert!(output_dir.join("mcp_store.db").is_file());
    assert!(output_dir.join("mcp_store_vv2.db").is_file());

    // 3. Promote a third version to default — the trickiest path: it
    // demotes v1 (the original default) to its own suffixed files, and
    // must leave v2's files untouched.
    add_version::run(AddVersionRequest {
        project_dir: output_dir.clone(),
        version_label: "v3".to_string(),
        input: "tests/fixtures/openapi/minimal-multi-scheme.yaml".to_string(),
        set_default: true,
        force: false,
    })
    .await
    .expect("add-version v3 --set-default must succeed");

    let ledger = add_version::ledger::read(&output_dir)
        .await
        .expect("ledger must still be readable");
    assert_eq!(ledger.default_version, "v3");
    assert_eq!(ledger.versions["v3"].db_file, "mcp_store.db");
    assert_eq!(ledger.versions["v1"].db_file, "mcp_store_vv1.db");
    assert_eq!(ledger.versions["v2"].db_file, "mcp_store_vv2.db");
    assert!(output_dir.join("mcp_store_vv1.db").is_file());
    assert!(output_dir.join("mcp_store_vv2.db").is_file());

    // v1's original data must have survived the demotion intact — the
    // whole point of `--set-default` never silently destroying data.
    let v1_endpoint_count = sqlite_endpoint_count(&output_dir.join("mcp_store_vv1.db"));
    let v3_endpoint_count = sqlite_endpoint_count(&output_dir.join("mcp_store.db"));
    assert_eq!(
        v1_endpoint_count, 1,
        "v1's single 'ping' operation must survive demotion"
    );
    assert!(v3_endpoint_count > 0);

    // 4. Rebuild (the patched .ts files must still typecheck) and run the
    // generated project's own `versions` command — proving the marker
    // patching produced valid, working TypeScript, not just files that
    // happen to exist.
    let build_status = Command::new("npm")
        .arg("run")
        .arg("build")
        .current_dir(&output_dir)
        .status()
        .expect("npm must be installed");
    assert!(
        build_status.success(),
        "npm run build must succeed after add-version"
    );

    let versions_output = Command::new("node")
        .arg("dist/cli.js")
        .arg("versions")
        .current_dir(&output_dir)
        .env("GENERATED_URL", "http://localhost:1234")
        .env("GENERATED_AUTH_METHOD", "basic")
        .output()
        .expect("node must be installed");
    let stdout = String::from_utf8_lossy(&versions_output.stdout);
    assert!(
        stdout.contains("v3 (default, active)"),
        "stdout was: {stdout}"
    );
    assert!(stdout.contains("v1"), "stdout was: {stdout}");
    assert!(stdout.contains("v2"), "stdout was: {stdout}");
}

fn sqlite_endpoint_count(db_path: &std::path::Path) -> i64 {
    let conn = rusqlite::Connection::open(db_path).expect("must be able to open the sqlite store");
    conn.query_row("SELECT count(*) FROM endpoints", [], |row| row.get(0))
        .expect("endpoints table must exist")
}
