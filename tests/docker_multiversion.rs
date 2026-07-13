//! Regression coverage for multi-version database packaging in generated images.

const RUST_DOCKERFILE: &str = include_str!("../src/targets/rust/templates/Dockerfile.tera");
const PYTHON_DOCKERFILE: &str = include_str!("../src/targets/python/templates/Dockerfile.tera");
const TYPESCRIPT_DOCKERFILE: &str =
    include_str!("../src/targets/typescript/templates/Dockerfile.tera");
const CSHARP_DOCKERFILE: &str = include_str!("../src/targets/csharp/templates/Dockerfile.tera");
const GO_DOCKERFILE: &str = include_str!("../src/targets/go/templates/Dockerfile.tera");

#[test]
fn every_language_packages_and_populates_all_version_stores() {
    for (language, dockerfile, populate_all) in [
        (
            "rust",
            RUST_DOCKERFILE,
            "{{ project_name }}-populate-embeddings --all",
        ),
        (
            "python",
            PYTHON_DOCKERFILE,
            "services.populate_embeddings --all",
        ),
        (
            "typescript",
            TYPESCRIPT_DOCKERFILE,
            "npm run populate-embeddings -- --all",
        ),
        ("csharp", CSHARP_DOCKERFILE, "populate-embeddings --all"),
        ("go", GO_DOCKERFILE, "./populate-embeddings --all"),
    ] {
        assert!(
            dockerfile.contains("COPY mcp_store*.db ./"),
            "{language} Dockerfile must package databases added after initial generation"
        );
        assert!(
            dockerfile.contains(populate_all),
            "{language} Dockerfile must populate every packaged database"
        );
    }
}

#[test]
fn rust_rebuilds_after_population_to_refresh_embedded_database_bytes() {
    let populate = RUST_DOCKERFILE
        .find("{{ project_name }}-populate-embeddings --all")
        .expect("Rust Dockerfile must populate all stores");
    let final_build = RUST_DOCKERFILE
        .rfind("RUN cargo build --release")
        .expect("Rust Dockerfile must perform its final release build");

    assert!(
        populate < final_build,
        "Rust must rebuild after population so include_bytes! embeds populated stores"
    );
}
