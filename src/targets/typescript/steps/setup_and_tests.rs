use anyhow::Result;

use crate::auth_profile::AuthSchemeKind;
use crate::context::GeneratorContext;
use crate::targets::typescript::context::TsTemplateContext;
use crate::targets::typescript::emit::render_and_write;
use crate::targets::typescript::render::render_engine;

/// Always emitted, regardless of which schemes were discovered.
const SHARED_FILES: &[(&str, &str)] = &[
    ("cli/setup-wizard.ts.tera", "src/cli/setup-wizard.ts"),
    ("vitest.config.ts.tera", "vitest.config.ts"),
    (
        "tests/helpers/mock-api-server.ts.tera",
        "tests/helpers/mock-api-server.ts",
    ),
    (
        "tests/helpers/mcp-test-client.ts.tera",
        "tests/helpers/mcp-test-client.ts",
    ),
    (
        "tests/helpers/dummy-credentials.ts.tera",
        "tests/helpers/dummy-credentials.ts",
    ),
    (
        "tests/helpers/log-capture.ts.tera",
        "tests/helpers/log-capture.ts",
    ),
    (
        "tests/helpers/skip-integration.ts.tera",
        "tests/helpers/skip-integration.ts",
    ),
    (
        "tests/unit/core/config-manager.test.ts.tera",
        "tests/unit/core/config-manager.test.ts",
    ),
    (
        "tests/unit/tools/search-tool.test.ts.tera",
        "tests/unit/tools/search-tool.test.ts",
    ),
    (
        "tests/unit/tools/get-tool.test.ts.tera",
        "tests/unit/tools/get-tool.test.ts",
    ),
    (
        "tests/unit/tools/call-tool.test.ts.tera",
        "tests/unit/tools/call-tool.test.ts",
    ),
    (
        "tests/unit/auth/stub.test.ts.tera",
        "tests/unit/auth/stub.test.ts",
    ),
    (
        "tests/integration/call-pipeline.test.ts.tera",
        "tests/integration/call-pipeline.test.ts",
    ),
    (
        "tests/e2e/mcp-server.test.ts.tera",
        "tests/e2e/mcp-server.test.ts",
    ),
];

/// One unit test file per discovered `AuthSchemeKind` — never emit a test
/// importing a strategy module that wasn't actually generated (Story 10),
/// or `run_generated_tests` (Story 14) would fail outright on a dangling
/// import, not just a logical test failure.
fn auth_test_template(kind: AuthSchemeKind) -> (&'static str, &'static str) {
    match kind {
        AuthSchemeKind::Basic => (
            "tests/unit/auth/basic.test.ts.tera",
            "tests/unit/auth/basic.test.ts",
        ),
        AuthSchemeKind::ApiKey => (
            "tests/unit/auth/api-key.test.ts.tera",
            "tests/unit/auth/api-key.test.ts",
        ),
        AuthSchemeKind::BearerPat => (
            "tests/unit/auth/pat.test.ts.tera",
            "tests/unit/auth/pat.test.ts",
        ),
        AuthSchemeKind::OAuth2 => (
            "tests/unit/auth/oauth2.test.ts.tera",
            "tests/unit/auth/oauth2.test.ts",
        ),
        AuthSchemeKind::OAuth1 => (
            "tests/unit/auth/oauth1.test.ts.tera",
            "tests/unit/auth/oauth1.test.ts",
        ),
    }
}

/// `generate_setup_wizard_and_tests` (architecture.md §1, step 10): the
/// interactive `setup` command and the generated test suite exercising
/// Stories 8-12.
pub async fn generate_setup_wizard_and_tests(ctx: &GeneratorContext) -> Result<()> {
    let view = TsTemplateContext::from_context(ctx);
    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in SHARED_FILES {
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
    use std::path::PathBuf;

    use super::*;
    use crate::auth_profile::AuthSchemeDescriptor;

    fn ctx_with_schemes(
        output_dir: PathBuf,
        auth_schemes: Vec<AuthSchemeDescriptor>,
    ) -> GeneratorContext {
        GeneratorContext {
            openapi_input: "spec.yaml".to_string(),
            output_dir,
            force: false,
            output_dir_preexisted: false,
            auth_schemes,
            normalized_operations: Vec::new(),
            api_title: "Widget API".to_string(),
        }
    }

    fn descriptor(name: &str, kind: AuthSchemeKind) -> AuthSchemeDescriptor {
        AuthSchemeDescriptor {
            name: name.to_string(),
            kind,
        }
    }

    #[tokio::test]
    async fn writes_every_shared_file() {
        let dir = tempfile::tempdir().unwrap();
        // The real pipeline (profile_auth) always guarantees at least one
        // auth scheme, whether classified or supplied via the interactive
        // fallback prompt, so this fixture matches that invariant.
        let ctx = ctx_with_schemes(
            dir.path().to_path_buf(),
            vec![descriptor("basicAuth", AuthSchemeKind::Basic)],
        );

        generate_setup_wizard_and_tests(&ctx).await.unwrap();

        for (_, out_name) in SHARED_FILES {
            assert!(dir.path().join(out_name).is_file(), "missing {out_name}");
        }
    }

    #[tokio::test]
    async fn only_emits_auth_tests_for_discovered_schemes() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_schemes(
            dir.path().to_path_buf(),
            vec![descriptor("basicAuth", AuthSchemeKind::Basic)],
        );

        generate_setup_wizard_and_tests(&ctx).await.unwrap();

        assert!(dir.path().join("tests/unit/auth/basic.test.ts").is_file());
        assert!(dir.path().join("tests/unit/auth/stub.test.ts").is_file());
        for undiscovered in [
            "tests/unit/auth/pat.test.ts",
            "tests/unit/auth/oauth1.test.ts",
            "tests/unit/auth/oauth2.test.ts",
            "tests/unit/auth/api-key.test.ts",
        ] {
            assert!(
                !dir.path().join(undiscovered).exists(),
                "unexpected {undiscovered}"
            );
        }
    }

    #[tokio::test]
    async fn duplicate_scheme_kind_only_emits_one_test_file() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_schemes(
            dir.path().to_path_buf(),
            vec![
                descriptor("oauth2Primary", AuthSchemeKind::OAuth2),
                descriptor("oauth2Secondary", AuthSchemeKind::OAuth2),
            ],
        );

        generate_setup_wizard_and_tests(&ctx).await.unwrap();

        assert!(dir.path().join("tests/unit/auth/oauth2.test.ts").is_file());
    }
}
