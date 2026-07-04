use anyhow::Result;

use crate::auth_profile::AuthSchemeKind;
use crate::context::GeneratorContext;
use crate::targets::go::context::GoTemplateContext;
use crate::targets::go::emit::render_and_write;
use crate::targets::go::render::render_engine;

/// Always emitted, regardless of which schemes were discovered.
/// `internal/cli/roles.go.tera` is re-rendered here too — it's the same
/// shared template `steps::transports` (Story G5) already wrote once, but
/// its `RunSetup` reference only resolves once this story's
/// `internal/cli/setup.go` exists alongside it, mirroring
/// `targets::csharp::steps::enterprise` re-rendering `Program.cs.tera`
/// after `steps::bootstrap` already wrote it once.
const FILES: &[(&str, &str)] = &[
    ("internal/cli/roles.go.tera", "internal/cli/roles.go"),
    ("internal/cli/setup.go.tera", "internal/cli/setup.go"),
    (
        "internal/core/cache_test.go.tera",
        "internal/core/cache_test.go",
    ),
    (
        "internal/core/healthcheck_test.go.tera",
        "internal/core/healthcheck_test.go",
    ),
    (
        "internal/services/embedding_integration_test.go.tera",
        "internal/services/embedding_integration_test.go",
    ),
    ("scripts/coverage.sh.tera", "scripts/coverage.sh"),
    ("scripts/profile.sh.tera", "scripts/profile.sh"),
];

/// One test file per discovered `AuthSchemeKind` — same dedup-by-kind
/// rule as `steps::auth::strategy_template`, so a spec declaring two
/// `apiKey` schemes still only gets one `apikey_test.go`. This is the
/// "hard requirement" this story's goal text calls out: Go's compiler
/// treats an unused import as a hard build error, so a test file
/// referencing an undiscovered strategy type would break the build
/// outright, not just trip a lint warning the way it would for the other
/// targets.
fn test_template(kind: AuthSchemeKind) -> (&'static str, &'static str) {
    match kind {
        AuthSchemeKind::Basic => (
            "internal/auth/basic_test.go.tera",
            "internal/auth/basic_test.go",
        ),
        AuthSchemeKind::ApiKey => (
            "internal/auth/apikey_test.go.tera",
            "internal/auth/apikey_test.go",
        ),
        AuthSchemeKind::BearerPat => (
            "internal/auth/pat_test.go.tera",
            "internal/auth/pat_test.go",
        ),
        AuthSchemeKind::OAuth2 => (
            "internal/auth/oauth2_test.go.tera",
            "internal/auth/oauth2_test.go",
        ),
        AuthSchemeKind::OAuth1 => (
            "internal/auth/oauth1_test.go.tera",
            "internal/auth/oauth1_test.go",
        ),
    }
}

/// `generate_setup_wizard_and_tests` (architecture.md §1, step 10): the
/// interactive credential setup wizard (`internal/cli/setup.go`, via
/// `AlecAivazis/survey`) and the generated `go test` suite — table-driven
/// unit tests per package, conditionally emitted per discovered auth
/// scheme, plus one `//go:build integration`-gated test for the
/// network/ONNX-dependent embedding pipeline (Go's native equivalent of
/// `vitest.config.ts`/`pytest.ini` gating slow tests out of the default
/// run).
pub async fn generate_setup_wizard_and_tests(ctx: &GeneratorContext) -> Result<()> {
    let view = GoTemplateContext::from_context(ctx);
    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in FILES {
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }

    let mut emitted_kinds: Vec<AuthSchemeKind> = Vec::new();
    for scheme in &ctx.auth_schemes {
        if emitted_kinds.contains(&scheme.kind) {
            continue;
        }
        emitted_kinds.push(scheme.kind);

        let (template, out_name) = test_template(scheme.kind);
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::auth_profile::AuthSchemeDescriptor;

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
        }
    }

    fn descriptor(name: &str, kind: AuthSchemeKind) -> AuthSchemeDescriptor {
        AuthSchemeDescriptor {
            name: name.to_string(),
            kind,
        }
    }

    fn output_dir(parent: &tempfile::TempDir) -> PathBuf {
        parent.path().join("output")
    }

    fn file_exists(dir: &Path, relative: &str) -> bool {
        dir.join(relative).is_file()
    }

    #[tokio::test]
    async fn writes_every_shared_file() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_setup_wizard_and_tests(&ctx).await.unwrap();

        for (_, out_name) in FILES {
            assert!(file_exists(&dir, out_name), "missing {out_name}");
        }
    }

    #[tokio::test]
    async fn roles_go_no_longer_declares_a_run_setup_stub() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_setup_wizard_and_tests(&ctx).await.unwrap();

        let roles = tokio::fs::read_to_string(dir.join("internal/cli/roles.go"))
            .await
            .unwrap();
        assert!(!roles.contains("func RunSetup"));

        let setup = tokio::fs::read_to_string(dir.join("internal/cli/setup.go"))
            .await
            .unwrap();
        assert!(setup.contains("func RunSetup"));
    }

    #[tokio::test]
    async fn all_five_kinds_emit_every_test_file_and_no_others() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(
            dir.clone(),
            vec![
                descriptor("basicAuth", AuthSchemeKind::Basic),
                descriptor("apiKey", AuthSchemeKind::ApiKey),
                descriptor("pat", AuthSchemeKind::BearerPat),
                descriptor("oauth1", AuthSchemeKind::OAuth1),
                descriptor("oauth2", AuthSchemeKind::OAuth2),
            ],
        );

        generate_setup_wizard_and_tests(&ctx).await.unwrap();

        for expected in [
            "internal/auth/basic_test.go",
            "internal/auth/apikey_test.go",
            "internal/auth/pat_test.go",
            "internal/auth/oauth1_test.go",
            "internal/auth/oauth2_test.go",
        ] {
            assert!(file_exists(&dir, expected), "missing {expected}");
        }
    }

    #[tokio::test]
    async fn no_schemes_emits_no_auth_test_files() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_setup_wizard_and_tests(&ctx).await.unwrap();

        for undiscovered in [
            "internal/auth/basic_test.go",
            "internal/auth/apikey_test.go",
            "internal/auth/pat_test.go",
            "internal/auth/oauth1_test.go",
            "internal/auth/oauth2_test.go",
        ] {
            assert!(
                !file_exists(&dir, undiscovered),
                "unexpected {undiscovered}"
            );
        }

        let setup = tokio::fs::read_to_string(dir.join("internal/cli/setup.go"))
            .await
            .unwrap();
        assert!(setup.contains("return core.AuthMethodNone"));
    }

    #[tokio::test]
    async fn duplicate_scheme_kind_only_emits_one_test_file() {
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

        assert!(file_exists(&dir, "internal/auth/apikey_test.go"));
    }
}
