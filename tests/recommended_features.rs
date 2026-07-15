use std::path::PathBuf;

use mcpify::auth_profile::{AuthSchemeKind, profile_auth_with_overrides};
use mcpify::context::GeneratorContext;
use mcpify::openapi::parse::{Format, parse_document};
use mcpify::package_preflight::analyze_tree;
use mcpify::project_config::{ProjectManifest, select_versions, write_settings};

fn context(output_dir: PathBuf) -> GeneratorContext {
    GeneratorContext {
        publish_registry: true,
        openapi_input: "spec.yaml".to_string(),
        output_dir,
        force: false,
        output_dir_preexisted: false,
        auth_schemes: Vec::new(),
        normalized_operations: Vec::new(),
        api_title: "Widget API".to_string(),
        version_label: "1.2.1".to_string(),
    }
}

const MANIFEST: &str = r#"
language: rust
output: ./widget-mcp
publish_registry: true
publication:
  license: MIT
  repository: https://github.com/example/widget-mcp
  readme: README.md
  authors: [Example Org]
  keywords: [mcp, widget]
  categories: [command-line-utilities]
  exclude: [tests/fixtures/**]
default_headers:
  Accept: application/vnd.widget+json
  User-Agent: widget-mcp/1
auth:
  - name: personalAccessToken
    kind: pat
versions:
  - version: 1.2.0
    source: specs/1.2.0.yaml
  - version: 1.2.1
    source: specs/1.2.1.yaml
    default: true
  - version: 1.3.0
    source: specs/1.3.0.yaml
version_policy:
  mode: latest-per-minor
package_size_limit_mb: 12
"#;

#[test]
fn manifest_selects_latest_patch_per_minor_and_preserves_default() {
    let manifest = ProjectManifest::from_yaml(MANIFEST).unwrap();
    manifest.validate().unwrap();
    let selected = select_versions(&manifest.versions, &manifest.version_policy).unwrap();
    let labels: Vec<_> = selected
        .iter()
        .map(|version| version.version.as_str())
        .collect();
    assert_eq!(labels, vec!["1.2.1", "1.3.0"]);
    assert!(selected[0].default);
    assert_eq!(manifest.settings().default_headers.len(), 2);
    assert_eq!(
        manifest.settings().publication.license.as_deref(),
        Some("MIT")
    );
}

#[tokio::test]
async fn supplemental_auth_is_merged_with_discovered_auth() {
    let manifest = ProjectManifest::from_yaml(MANIFEST).unwrap();
    let doc = parse_document(
        r#"openapi: 3.0.0
info: { title: Test, version: 1.0.0 }
paths: {}
components:
  securitySchemes:
    basicAuth: { type: http, scheme: basic }
"#,
        Some(Format::Yaml),
    )
    .unwrap();

    let auth = profile_auth_with_overrides(&doc, false, &manifest.auth)
        .await
        .unwrap();
    assert_eq!(auth.len(), 2);
    assert_eq!(auth[0].kind, AuthSchemeKind::Basic);
    assert_eq!(auth[1].kind, AuthSchemeKind::BearerPat);
}

#[tokio::test]
async fn manifest_keeps_portable_paths_separate_from_resolved_paths() {
    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join("mcpify.yaml");
    tokio::fs::write(&manifest_path, MANIFEST.replace("./widget-mcp", "./out"))
        .await
        .unwrap();

    let (portable, resolved) = ProjectManifest::read_portable_and_resolved(&manifest_path)
        .await
        .unwrap();

    assert_eq!(portable.output, PathBuf::from("./out"));
    assert_eq!(portable.versions[0].source, "specs/1.2.0.yaml");
    assert_eq!(resolved.output, dir.path().join("./out"));
    assert_eq!(
        resolved.versions[0].source,
        dir.path().join("specs/1.2.0.yaml").to_string_lossy()
    );
}

#[test]
fn swagger_2_is_converted_and_missing_parameter_schema_is_repaired() {
    let doc = parse_document(
        r#"swagger: '2.0'
info: { title: Legacy API, version: 1.0.0 }
basePath: /api
paths:
  /widgets:
    get:
      operationId: listWidgets
      parameters:
        - { name: limit, in: query }
      responses:
        '200': { description: ok }
securityDefinitions:
  basicAuth: { type: basic }
"#,
        Some(Format::Yaml),
    )
    .unwrap();

    assert_eq!(doc.raw()["openapi"], "3.0.3");
    assert_eq!(doc.raw()["servers"][0]["url"], "/api");
    assert_eq!(
        doc.raw()["paths"]["/widgets"]["get"]["parameters"][0]["schema"]["type"],
        "string"
    );
    assert_eq!(
        doc.raw()["components"]["securitySchemes"]["basicAuth"]["scheme"],
        "basic"
    );
}

#[tokio::test]
async fn settings_are_visible_to_all_five_template_contexts() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = ProjectManifest::from_yaml(MANIFEST).unwrap();
    write_settings(dir.path(), &manifest.settings())
        .await
        .unwrap();
    let ctx = context(dir.path().to_path_buf());

    let contexts = [
        serde_json::to_value(mcpify::targets::rust::context::RsTemplateContext::from_context(&ctx))
            .unwrap(),
        serde_json::to_value(
            mcpify::targets::typescript::context::TsTemplateContext::from_context(&ctx),
        )
        .unwrap(),
        serde_json::to_value(
            mcpify::targets::python::context::PyTemplateContext::from_context(&ctx),
        )
        .unwrap(),
        serde_json::to_value(
            mcpify::targets::csharp::context::CsTemplateContext::from_context(&ctx),
        )
        .unwrap(),
        serde_json::to_value(mcpify::targets::go::context::GoTemplateContext::from_context(&ctx))
            .unwrap(),
    ];

    for value in contexts {
        assert_eq!(value["default_headers"].as_array().unwrap().len(), 2);
        assert_eq!(value["publication"]["license"], "MIT");
    }
}

#[tokio::test]
async fn all_five_clients_render_default_headers_and_publication_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = ProjectManifest::from_yaml(MANIFEST).unwrap();
    write_settings(dir.path(), &manifest.settings())
        .await
        .unwrap();
    let ctx = context(dir.path().to_path_buf());

    let rust_view = mcpify::targets::rust::context::RsTemplateContext::from_context(&ctx);
    let rust_context = tera::Context::from_serialize(rust_view).unwrap();
    let rust = mcpify::targets::rust::render::render_engine().unwrap();
    let rust_client = rust
        .render("services/api_client.rs.tera", &rust_context)
        .unwrap();
    let cargo = rust.render("Cargo.toml.tera", &rust_context).unwrap();
    assert!(rust_client.contains("User-Agent"));
    assert!(cargo.contains("license = \"MIT\""));
    assert!(cargo.contains("authors = [\"Example Org\"]"));
    assert!(cargo.contains("tests/fixtures/**"));

    let ts_view = mcpify::targets::typescript::context::TsTemplateContext::from_context(&ctx);
    let ts_context = tera::Context::from_serialize(ts_view).unwrap();
    let ts = mcpify::targets::typescript::render::render_engine().unwrap();
    let ts_client = ts
        .render("services/api-client.ts.tera", &ts_context)
        .unwrap();
    let package = ts.render("package.json.tera", &ts_context).unwrap();
    assert!(ts_client.contains("User-Agent"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&package).unwrap()["license"],
        "MIT"
    );

    let py_view = mcpify::targets::python::context::PyTemplateContext::from_context(&ctx);
    let py_context = tera::Context::from_serialize(py_view).unwrap();
    let py = mcpify::targets::python::render::render_engine().unwrap();
    assert!(
        py.render("services/api_client.py.tera", &py_context)
            .unwrap()
            .contains("User-Agent")
    );
    assert!(
        py.render("pyproject.toml.tera", &py_context)
            .unwrap()
            .contains("license = \"MIT\"")
    );

    let cs_view = mcpify::targets::csharp::context::CsTemplateContext::from_context(&ctx);
    let cs_context = tera::Context::from_serialize(cs_view).unwrap();
    let cs = mcpify::targets::csharp::render::render_engine().unwrap();
    assert!(
        cs.render("Services/ApiClient.cs.tera", &cs_context)
            .unwrap()
            .contains("User-Agent")
    );
    assert!(
        cs.render("Project.csproj.tera", &cs_context)
            .unwrap()
            .contains("<PackageLicenseExpression>MIT</PackageLicenseExpression>")
    );

    let go_view = mcpify::targets::go::context::GoTemplateContext::from_context(&ctx);
    let go_context = tera::Context::from_serialize(go_view).unwrap();
    let go = mcpify::targets::go::render::render_engine().unwrap();
    assert!(
        go.render("internal/services/apiclient.go.tera", &go_context)
            .unwrap()
            .contains("User-Agent")
    );
    assert!(
        go.render("LICENSE.tera", &go_context)
            .unwrap()
            .starts_with("MIT License")
    );
}

#[tokio::test]
async fn rust_automatic_user_agent_tracks_the_package_version() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("widget-mcp");
    tokio::fs::create_dir_all(&output).await.unwrap();
    let manifest =
        ProjectManifest::from_yaml(&MANIFEST.replace("  User-Agent: widget-mcp/1\n", "")).unwrap();
    write_settings(&output, &manifest.settings()).await.unwrap();
    let ctx = context(output);
    let view = mcpify::targets::rust::context::RsTemplateContext::from_context(&ctx);
    let tera_context = tera::Context::from_serialize(view).unwrap();
    let tera = mcpify::targets::rust::render::render_engine().unwrap();
    let client = tera
        .render("services/api_client.rs.tera", &tera_context)
        .unwrap();

    assert!(client.contains("env!(\"CARGO_PKG_VERSION\")"));
    assert!(client.contains("DEFAULT_USER_AGENT.to_string()"));
    assert!(!client.contains("widget-mcp/0.1.0 (generated by mcpify)"));
}

#[tokio::test]
async fn rust_release_templates_encode_onnx_runtime_constraints() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = ProjectManifest::from_yaml(MANIFEST).unwrap();
    write_settings(dir.path(), &manifest.settings())
        .await
        .unwrap();
    let ctx = context(dir.path().to_path_buf());
    let view = mcpify::targets::rust::context::RsTemplateContext::from_context(&ctx);
    let tera_context = tera::Context::from_serialize(view).unwrap();
    let tera = mcpify::targets::rust::render::render_engine().unwrap();

    let dist = tera
        .render("dist-workspace.toml.tera", &tera_context)
        .unwrap();
    assert!(dist.contains("cargo-dist-version = \"0.32.0\""));
    assert!(dist.contains("msvc-crt-static = false"));
    assert!(dist.contains("ubuntu-24.04"));
    assert!(!dist.contains("\"x86_64-apple-darwin\""));
    let release = tera
        .render(".github/workflows/release.yml.tera", &tera_context)
        .unwrap();
    assert!(release.contains("cargo test --locked"));
    assert!(release.contains("dist build --artifacts=local"));
    assert!(!release.contains("docker/build-push-action"));
    let container = tera
        .render(".github/workflows/container-image.yml.tera", &tera_context)
        .unwrap();
    assert!(container.contains("docker/build-push-action"));
    assert!(container.contains("ghcr.io"));
    let dockerignore = tera.render(".dockerignore.tera", &tera_context).unwrap();
    assert!(dockerignore.lines().any(|line| line == ".fastembed_cache/"));
    assert!(dockerignore.lines().any(|line| line == "target/"));
    let publish = tera
        .render(".github/workflows/publish-crate.yml.tera", &tera_context)
        .unwrap();
    assert!(publish.contains("cargo package --locked"));
    assert!(publish.contains("cargo publish --locked"));
}

#[tokio::test]
async fn size_preflight_reports_largest_contributors() {
    let dir = tempfile::tempdir().unwrap();
    tokio::fs::write(dir.path().join("large.bin"), vec![0_u8; 32])
        .await
        .unwrap();
    tokio::fs::write(dir.path().join("small.bin"), vec![0_u8; 8])
        .await
        .unwrap();

    let report = analyze_tree(dir.path()).unwrap();
    assert_eq!(report.total_bytes, 40);
    assert_eq!(report.largest[0].path, PathBuf::from("large.bin"));
    let err = report.enforce(16).unwrap_err();
    assert!(err.to_string().contains("large.bin"));
    assert!(err.to_string().contains("32 B"));
}

#[tokio::test]
async fn size_preflight_ignores_fastembed_model_cache() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join(".fastembed_cache").join("models");
    tokio::fs::create_dir_all(&cache).await.unwrap();
    tokio::fs::write(cache.join("model.onnx"), vec![0_u8; 1024])
        .await
        .unwrap();
    tokio::fs::write(dir.path().join("mcp_store.db"), vec![0_u8; 32])
        .await
        .unwrap();

    let report = analyze_tree(dir.path()).unwrap();
    assert_eq!(report.total_bytes, 32);
    assert_eq!(report.largest[0].path, PathBuf::from("mcp_store.db"));
}
