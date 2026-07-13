use std::path::Path;

use mcpify::pipeline::run_shared_pipeline;

#[tokio::test]
async fn generated_container_docs_and_compose_are_consistent_for_every_target() {
    for target in ["rust", "typescript", "python", "go", "csharp"] {
        assert_target(target).await;
    }
}

async fn assert_target(target: &str) {
    let dir = tempfile::tempdir().unwrap();
    let output_dir = dir.path().join("out");
    let ctx = run_shared_pipeline(
        "tests/fixtures/openapi/minimal-with-auth.yaml",
        output_dir.clone(),
        false,
        false,
        false,
        "default",
    )
    .await
    .expect("shared pipeline must succeed");

    match target {
        "rust" => {
            mcpify::targets::rust::steps::bootstrap::bootstrap_project(&ctx)
                .await
                .unwrap();
            mcpify::targets::rust::steps::enterprise::generate_enterprise_scaffolding(&ctx)
                .await
                .unwrap();
        }
        "typescript" => {
            mcpify::targets::typescript::steps::bootstrap::bootstrap_project(&ctx)
                .await
                .unwrap();
            mcpify::targets::typescript::steps::enterprise::generate_enterprise_scaffolding(&ctx)
                .await
                .unwrap();
        }
        "python" => {
            mcpify::targets::python::steps::bootstrap::bootstrap_project(&ctx)
                .await
                .unwrap();
            mcpify::targets::python::steps::enterprise::generate_enterprise_scaffolding(&ctx)
                .await
                .unwrap();
        }
        "go" => {
            mcpify::targets::go::steps::bootstrap::bootstrap_project(&ctx)
                .await
                .unwrap();
            mcpify::targets::go::steps::enterprise::generate_enterprise_scaffolding(&ctx)
                .await
                .unwrap();
        }
        "csharp" => {
            mcpify::targets::csharp::steps::bootstrap::bootstrap_project(&ctx)
                .await
                .unwrap();
            mcpify::targets::csharp::steps::enterprise::generate_enterprise_scaffolding(&ctx)
                .await
                .unwrap();
        }
        _ => unreachable!(),
    }

    assert_generated_files(target, &output_dir);
}

fn assert_generated_files(target: &str, output_dir: &Path) {
    let readme = std::fs::read_to_string(output_dir.join("README.md"))
        .expect("generated README must be readable");
    assert!(
        readme.contains("docker compose run --rm -T out"),
        "{target}"
    );
    assert!(readme.contains("docker compose up out-http"), "{target}");
    assert!(
        readme.contains("Docker Compose automatically discovers `docker-compose.yml`"),
        "{target}"
    );
    assert!(
        readme.contains("Stdio is a process transport, not a listening service"),
        "{target}"
    );

    let compose = std::fs::read_to_string(output_dir.join("docker-compose.yml"))
        .expect("generated Compose file must be readable");
    assert!(compose.contains("  out:\n"), "{target}");
    assert!(compose.contains("    command: [\"start\"]"), "{target}");
    assert!(compose.contains("  out-http:\n"), "{target}");
    assert!(
        compose.contains("    command: [\"http\", \"--host\", \"0.0.0.0\", \"--port\", \"3000\"]"),
        "{target}"
    );
}
