use anyhow::Result;

use crate::auth_profile::AuthSchemeKind;
use crate::context::GeneratorContext;
use crate::targets::python::context::PyTemplateContext;
use crate::targets::python::emit::render_and_write;
use crate::targets::python::render::render_engine;

/// Rendered under `src/<module_name>/` (package code, not test code).
const PACKAGE_FILES: &[(&str, &str)] = &[("cli/setup_wizard.py.tera", "cli/setup_wizard.py")];

/// Rendered directly into `ctx.output_dir` — the generated `pytest` suite
/// (`pytest-asyncio` for the async paths), always emitted regardless of
/// which auth schemes were discovered.
const SHARED_TEST_FILES: &[(&str, &str)] = &[
    ("tests/conftest.py.tera", "tests/conftest.py"),
    (
        "tests/unit/core/test_config.py.tera",
        "tests/unit/core/test_config.py",
    ),
    (
        "tests/unit/core/test_circuit_breaker.py.tera",
        "tests/unit/core/test_circuit_breaker.py",
    ),
    (
        "tests/unit/core/test_rate_limiter.py.tera",
        "tests/unit/core/test_rate_limiter.py",
    ),
    (
        "tests/unit/core/test_cache.py.tera",
        "tests/unit/core/test_cache.py",
    ),
    (
        "tests/unit/tools/test_search_tool.py.tera",
        "tests/unit/tools/test_search_tool.py",
    ),
    (
        "tests/unit/tools/test_get_tool.py.tera",
        "tests/unit/tools/test_get_tool.py",
    ),
    (
        "tests/unit/tools/test_call_tool.py.tera",
        "tests/unit/tools/test_call_tool.py",
    ),
    (
        "tests/unit/validation/test_validator.py.tera",
        "tests/unit/validation/test_validator.py",
    ),
    (
        "tests/unit/auth/test_stub.py.tera",
        "tests/unit/auth/test_stub.py",
    ),
    (
        "tests/integration/test_call_pipeline.py.tera",
        "tests/integration/test_call_pipeline.py",
    ),
    (
        "tests/e2e/test_mcp_server.py.tera",
        "tests/e2e/test_mcp_server.py",
    ),
    // Fix 8b regression test: asserts `semantic_endpoints` row count
    // equals `endpoints` row count for every version, once
    // `populate_embeddings --all` has been run against the real on-disk
    // store files. Skipped unless RUN_PACKAGING_TESTS=true (see the
    // template's own doc comment), so it's safe to include in this
    // always-emitted list alongside the rest of the suite.
    (
        "tests/packaging/test_embeddings_populated.py.tera",
        "tests/packaging/test_embeddings_populated.py",
    ),
    ("scripts/coverage.sh.tera", "scripts/coverage.sh"),
    ("scripts/profile.sh.tera", "scripts/profile.sh"),
];

/// One unit test file per discovered `AuthSchemeKind` — never emit a test
/// importing a strategy module that wasn't actually generated (Story P4),
/// or `run_generated_tests` (Story P8) would fail outright on a dangling
/// import, not just a logical test failure. Mirrors
/// `targets::typescript::steps::setup_and_tests::auth_test_template`.
fn auth_test_template(kind: AuthSchemeKind) -> (&'static str, &'static str) {
    match kind {
        AuthSchemeKind::Basic => (
            "tests/unit/auth/test_basic.py.tera",
            "tests/unit/auth/test_basic.py",
        ),
        AuthSchemeKind::ApiKey => (
            "tests/unit/auth/test_api_key.py.tera",
            "tests/unit/auth/test_api_key.py",
        ),
        AuthSchemeKind::BearerPat => (
            "tests/unit/auth/test_pat.py.tera",
            "tests/unit/auth/test_pat.py",
        ),
        AuthSchemeKind::OAuth2 => (
            "tests/unit/auth/test_oauth2.py.tera",
            "tests/unit/auth/test_oauth2.py",
        ),
        AuthSchemeKind::OAuth1 => (
            "tests/unit/auth/test_oauth1.py.tera",
            "tests/unit/auth/test_oauth1.py",
        ),
    }
}

/// `generate_setup_wizard_and_tests` (architecture.md §1, step 10): the
/// interactive `setup` command and the generated `pytest` suite, exercising
/// Stories P2-P6.
pub async fn generate_setup_wizard_and_tests(ctx: &GeneratorContext) -> Result<()> {
    let view = PyTemplateContext::from_context(ctx);
    let package_root = ctx.output_dir.join("src").join(&view.module_name);
    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in PACKAGE_FILES {
        render_and_write(&tera, template, &tera_ctx, &package_root.join(out_name)).await?;
    }

    for (template, out_name) in SHARED_TEST_FILES {
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }

    let mut emitted_kinds: Vec<AuthSchemeKind> = Vec::new();
    for scheme in &ctx.auth_schemes {
        if emitted_kinds.contains(&scheme.kind) {
            continue;
        }
        emitted_kinds.push(scheme.kind);

        let (template, out_name) = auth_test_template(scheme.kind);
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::auth_profile::{AuthSchemeDescriptor, default_location_for};

    fn ctx_with_schemes(
        output_dir: PathBuf,
        auth_schemes: Vec<AuthSchemeDescriptor>,
    ) -> GeneratorContext {
        GeneratorContext {
            publish_registry: false,
            openapi_input: "spec.yaml".to_string(),
            output_dir,
            force: false,
            output_dir_preexisted: false,
            auth_schemes,
            normalized_operations: Vec::new(),
            api_title: "Widget API".to_string(),
            version_label: "default".to_string(),
        }
    }

    fn descriptor(name: &str, kind: AuthSchemeKind) -> AuthSchemeDescriptor {
        AuthSchemeDescriptor {
            name: name.to_string(),
            kind,
            location: default_location_for(kind),
            scopes: Vec::new(),
            authorization_url: None,
            token_url: None,
        }
    }

    // A named subdirectory (rather than the tempdir root, whose name is
    // random) so `module_name` is deterministic.
    fn output_dir(parent: &tempfile::TempDir) -> PathBuf {
        parent.path().join("output")
    }

    #[tokio::test]
    async fn writes_the_setup_wizard_and_every_shared_test_file() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        // The real pipeline (profile_auth) always guarantees at least one
        // scheme; a stub-only project is the only case with zero.
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_setup_wizard_and_tests(&ctx).await.unwrap();

        let setup_wizard = dir.join("src").join("output").join("cli/setup_wizard.py");
        assert!(setup_wizard.is_file());

        for (_, out_name) in SHARED_TEST_FILES {
            assert!(dir.join(out_name).is_file(), "missing {out_name}");
        }
    }

    #[tokio::test]
    async fn basic_and_oauth2_fixture_emits_exactly_the_expected_auth_tests() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(
            dir.clone(),
            vec![
                descriptor("basicAuth", AuthSchemeKind::Basic),
                descriptor("oauth2", AuthSchemeKind::OAuth2),
            ],
        );

        generate_setup_wizard_and_tests(&ctx).await.unwrap();

        for expected in [
            "tests/unit/auth/test_basic.py",
            "tests/unit/auth/test_oauth2.py",
        ] {
            assert!(file_exists(&dir, expected), "missing {expected}");
        }
        for undiscovered in [
            "tests/unit/auth/test_pat.py",
            "tests/unit/auth/test_oauth1.py",
            "tests/unit/auth/test_api_key.py",
        ] {
            assert!(
                !file_exists(&dir, undiscovered),
                "unexpected {undiscovered}"
            );
        }
    }

    #[tokio::test]
    async fn duplicate_scheme_kind_only_emits_one_auth_test_file() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(
            dir.clone(),
            vec![
                descriptor("apiKeyOne", AuthSchemeKind::ApiKey),
                descriptor("apiKeyTwo", AuthSchemeKind::ApiKey),
            ],
        );

        generate_setup_wizard_and_tests(&ctx).await.unwrap();

        assert!(file_exists(&dir, "tests/unit/auth/test_api_key.py"));
    }

    fn file_exists(dir: &Path, relative: &str) -> bool {
        dir.join(relative).is_file()
    }
}
