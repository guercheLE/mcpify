//! Exercises the shared pipeline (ingest -> dir guard -> auth profiling ->
//! normalization -> mcp_store.db assembly) against every curated fixture
//! spec, for coverage breadth beyond the per-module unit tests (REQ-2.6.1).

use mcpify::auth_profile::AuthSchemeKind;
use mcpify::pipeline::run_shared_pipeline;

#[tokio::test]
async fn oauth2_json_fixture_is_classified_correctly() {
    let dir = tempfile::tempdir().unwrap();

    let ctx = run_shared_pipeline(
        "tests/fixtures/openapi/minimal-oauth2.json",
        dir.path().join("out"),
        false,
        false,
    )
    .await
    .unwrap();

    assert_eq!(ctx.auth_schemes.len(), 1);
    assert_eq!(ctx.auth_schemes[0].kind, AuthSchemeKind::OAuth2);
    assert_eq!(ctx.normalized_operations.len(), 1);
}

#[tokio::test]
async fn multi_scheme_fixture_discovers_all_four_kinds() {
    let dir = tempfile::tempdir().unwrap();

    let ctx = run_shared_pipeline(
        "tests/fixtures/openapi/minimal-multi-scheme.yaml",
        dir.path().join("out"),
        false,
        false,
    )
    .await
    .unwrap();

    let mut kinds: Vec<AuthSchemeKind> =
        ctx.auth_schemes.iter().map(|scheme| scheme.kind).collect();
    kinds.sort_by_key(|kind| format!("{kind:?}"));
    assert_eq!(ctx.auth_schemes.len(), 4);
    assert!(kinds.contains(&AuthSchemeKind::Basic));
    assert!(kinds.contains(&AuthSchemeKind::BearerPat));
    assert!(kinds.contains(&AuthSchemeKind::OAuth2));
    assert!(kinds.contains(&AuthSchemeKind::OAuth1));
    assert_eq!(ctx.normalized_operations.len(), 2);
}

#[tokio::test]
async fn no_auth_scheme_fixture_errors_when_non_interactive() {
    let dir = tempfile::tempdir().unwrap();

    let err = run_shared_pipeline(
        "tests/fixtures/openapi/minimal-no-auth-scheme.json",
        dir.path().join("out"),
        false,
        false,
    )
    .await
    .unwrap_err();

    assert!(err.to_string().contains("no usable auth scheme found"));
    // The directory guard runs before auth profiling, so it already
    // created output_dir by the time this fails; run_shared_pipeline's own
    // rollback must remove it rather than leaving an empty dir behind.
    assert!(!dir.path().join("out").exists());
}

#[tokio::test]
async fn refs_fixture_resolves_allof_and_self_referential_schemas() {
    let dir = tempfile::tempdir().unwrap();

    let ctx = run_shared_pipeline(
        "tests/fixtures/openapi/widgets-with-refs.yaml",
        dir.path().join("out"),
        false,
        false,
    )
    .await
    .unwrap();

    assert_eq!(ctx.normalized_operations.len(), 2);

    let create_widget = ctx
        .normalized_operations
        .iter()
        .find(|op| op.operation_id == "createWidget")
        .expect("createWidget must be present");

    let body_schema = &create_widget.validation_input_schema["properties"]["body"];
    assert!(
        body_schema["allOf"].is_array(),
        "body schema must preserve allOf"
    );
    assert_eq!(body_schema["allOf"][0]["$ref"], "#/$defs/BaseWidget");

    let output_schema = &create_widget.validation_output_schema;
    assert_eq!(output_schema["$ref"], "#/$defs/Widget");
    // The self-referential Widget.parent field must resolve to the
    // rewritten $defs location, proving the ref-rewriter handles cycles.
    assert_eq!(
        output_schema["$defs"]["Widget"]["properties"]["parent"]["$ref"],
        "#/$defs/Widget"
    );
}
