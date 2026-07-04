use anyhow::Result;

use crate::auth_profile::AuthSchemeKind;
use crate::context::GeneratorContext;
use crate::targets::rust::context::RsTemplateContext;
use crate::targets::rust::emit::render_and_write;
use crate::targets::rust::render::render_engine;

/// Always emitted, regardless of which schemes were discovered.
const SHARED_FILES: &[(&str, &str)] = &[
    ("auth/mod.rs.tera", "src/auth/mod.rs"),
    ("auth/auth_strategy.rs.tera", "src/auth/auth_strategy.rs"),
    ("auth/errors.rs.tera", "src/auth/errors.rs"),
    ("auth/strategies/mod.rs.tera", "src/auth/strategies/mod.rs"),
    (
        "auth/strategies/stub.rs.tera",
        "src/auth/strategies/stub.rs",
    ),
    ("auth/auth_manager.rs.tera", "src/auth/auth_manager.rs"),
];

/// One strategy module per discovered `AuthSchemeKind`; content depends only
/// on the kind, not the scheme's declared name, so kinds are deduplicated
/// before rendering (a spec can declare more than one scheme of the same
/// kind, e.g. two `apiKey` schemes). Mirrors
/// `targets::typescript::steps::auth::strategy_template`.
fn strategy_template(kind: AuthSchemeKind) -> (&'static str, &'static str) {
    match kind {
        AuthSchemeKind::Basic => (
            "auth/strategies/basic.rs.tera",
            "src/auth/strategies/basic.rs",
        ),
        AuthSchemeKind::ApiKey => (
            "auth/strategies/api_key.rs.tera",
            "src/auth/strategies/api_key.rs",
        ),
        AuthSchemeKind::BearerPat => ("auth/strategies/pat.rs.tera", "src/auth/strategies/pat.rs"),
        AuthSchemeKind::OAuth2 => (
            "auth/strategies/oauth2.rs.tera",
            "src/auth/strategies/oauth2.rs",
        ),
        AuthSchemeKind::OAuth1 => (
            "auth/strategies/oauth1.rs.tera",
            "src/auth/strategies/oauth1.rs",
        ),
    }
}

/// `generate_auth_strategies` (architecture.md §1, step 7): one strategy
/// module per discovered `AuthSchemeDescriptor`, plus the auth-manager that
/// selects the single active strategy from config at runtime.
pub async fn generate_auth_strategies(ctx: &GeneratorContext) -> Result<()> {
    let view = RsTemplateContext::from_context(ctx);
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

        let (template, out_name) = strategy_template(scheme.kind);
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

    fn file_exists(dir: &Path, relative: &str) -> bool {
        dir.join(relative).is_file()
    }

    #[tokio::test]
    async fn basic_and_oauth2_fixture_emits_exactly_the_expected_files() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_schemes(
            dir.path().to_path_buf(),
            vec![
                descriptor("basicAuth", AuthSchemeKind::Basic),
                descriptor("oauth2", AuthSchemeKind::OAuth2),
            ],
        );

        generate_auth_strategies(&ctx).await.unwrap();

        for expected in [
            "src/auth/mod.rs",
            "src/auth/auth_strategy.rs",
            "src/auth/errors.rs",
            "src/auth/auth_manager.rs",
            "src/auth/strategies/mod.rs",
            "src/auth/strategies/stub.rs",
            "src/auth/strategies/basic.rs",
            "src/auth/strategies/oauth2.rs",
        ] {
            assert!(file_exists(dir.path(), expected), "missing {expected}");
        }

        for undiscovered in [
            "src/auth/strategies/pat.rs",
            "src/auth/strategies/oauth1.rs",
            "src/auth/strategies/api_key.rs",
        ] {
            assert!(
                !file_exists(dir.path(), undiscovered),
                "unexpected {undiscovered}"
            );
        }
    }

    #[tokio::test]
    async fn all_four_kinds_fixture_emits_every_strategy_and_no_dangling_imports() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_schemes(
            dir.path().to_path_buf(),
            vec![
                descriptor("basicAuth", AuthSchemeKind::Basic),
                descriptor("pat", AuthSchemeKind::BearerPat),
                descriptor("oauth1", AuthSchemeKind::OAuth1),
                descriptor("oauth2", AuthSchemeKind::OAuth2),
            ],
        );

        generate_auth_strategies(&ctx).await.unwrap();

        for expected in [
            "src/auth/strategies/basic.rs",
            "src/auth/strategies/pat.rs",
            "src/auth/strategies/oauth1.rs",
            "src/auth/strategies/oauth2.rs",
        ] {
            assert!(file_exists(dir.path(), expected), "missing {expected}");
        }
        assert!(!file_exists(dir.path(), "src/auth/strategies/api_key.rs"));

        let auth_manager = tokio::fs::read_to_string(dir.path().join("src/auth/auth_manager.rs"))
            .await
            .unwrap();
        assert!(auth_manager.contains("BasicAuthStrategy"));
        assert!(auth_manager.contains("PatAuthStrategy"));
        assert!(auth_manager.contains("OAuth1Strategy"));
        assert!(auth_manager.contains("OAuth2Strategy"));
        assert!(
            !auth_manager.contains("ApiKeyStrategy"),
            "must not import an undiscovered strategy"
        );

        let strategies_mod =
            tokio::fs::read_to_string(dir.path().join("src/auth/strategies/mod.rs"))
                .await
                .unwrap();
        assert!(!strategies_mod.contains("pub mod api_key;"));
    }

    #[tokio::test]
    async fn duplicate_scheme_kind_only_emits_one_strategy_file() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_schemes(
            dir.path().to_path_buf(),
            vec![
                descriptor("apiKeyOne", AuthSchemeKind::ApiKey),
                descriptor("apiKeyTwo", AuthSchemeKind::ApiKey),
            ],
        );

        generate_auth_strategies(&ctx).await.unwrap();

        assert!(file_exists(dir.path(), "src/auth/strategies/api_key.rs"));
    }
}
