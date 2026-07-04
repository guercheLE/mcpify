use anyhow::Result;

use crate::auth_profile::AuthSchemeKind;
use crate::context::GeneratorContext;
use crate::targets::csharp::context::CsTemplateContext;
use crate::targets::csharp::emit::render_and_write;
use crate::targets::csharp::render::render_engine;

/// Always emitted, regardless of which schemes were discovered.
const SHARED_FILES: &[(&str, &str)] = &[
    ("Auth/IAuthStrategy.cs.tera", "Auth/IAuthStrategy.cs"),
    ("Auth/AuthErrors.cs.tera", "Auth/AuthErrors.cs"),
    ("Auth/AuthManager.cs.tera", "Auth/AuthManager.cs"),
    (
        "Auth/Strategies/StubAuthStrategy.cs.tera",
        "Auth/Strategies/StubAuthStrategy.cs",
    ),
    ("Core/AuthStrategies.cs.tera", "Core/AuthStrategies.cs"),
];

/// One strategy file per discovered `AuthSchemeKind`; content depends only
/// on the kind, not the scheme's declared name, so kinds are deduplicated
/// before rendering (a spec can declare more than one scheme of the same
/// kind, e.g. two `apiKey` schemes). Mirrors
/// `targets::python::steps::auth::strategy_template`.
fn strategy_template(kind: AuthSchemeKind) -> (&'static str, &'static str) {
    match kind {
        AuthSchemeKind::Basic => (
            "Auth/Strategies/BasicAuthStrategy.cs.tera",
            "Auth/Strategies/BasicAuthStrategy.cs",
        ),
        AuthSchemeKind::ApiKey => (
            "Auth/Strategies/ApiKeyAuthStrategy.cs.tera",
            "Auth/Strategies/ApiKeyAuthStrategy.cs",
        ),
        AuthSchemeKind::BearerPat => (
            "Auth/Strategies/PatAuthStrategy.cs.tera",
            "Auth/Strategies/PatAuthStrategy.cs",
        ),
        AuthSchemeKind::OAuth2 => (
            "Auth/Strategies/OAuth2AuthStrategy.cs.tera",
            "Auth/Strategies/OAuth2AuthStrategy.cs",
        ),
        AuthSchemeKind::OAuth1 => (
            "Auth/Strategies/OAuth1AuthStrategy.cs.tera",
            "Auth/Strategies/OAuth1AuthStrategy.cs",
        ),
    }
}

/// `generate_auth_strategies` (architecture.md §1, step 7): one strategy
/// implementation per discovered `AuthSchemeDescriptor`, keyed-DI-
/// registered in `Core/AuthStrategies.cs`, plus the `AuthManager` that
/// selects the single active strategy from config at runtime.
pub async fn generate_auth_strategies(ctx: &GeneratorContext) -> Result<()> {
    let view = CsTemplateContext::from_context(ctx);
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

    fn output_dir(parent: &tempfile::TempDir) -> PathBuf {
        parent.path().join("output")
    }

    fn file_exists(dir: &Path, relative: &str) -> bool {
        dir.join(relative).is_file()
    }

    #[tokio::test]
    async fn basic_and_oauth2_fixture_emits_exactly_the_expected_files() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(
            dir.clone(),
            vec![
                descriptor("basicAuth", AuthSchemeKind::Basic),
                descriptor("oauth2", AuthSchemeKind::OAuth2),
            ],
        );

        generate_auth_strategies(&ctx).await.unwrap();

        for expected in [
            "Auth/IAuthStrategy.cs",
            "Auth/AuthErrors.cs",
            "Auth/AuthManager.cs",
            "Auth/Strategies/StubAuthStrategy.cs",
            "Core/AuthStrategies.cs",
            "Auth/Strategies/BasicAuthStrategy.cs",
            "Auth/Strategies/OAuth2AuthStrategy.cs",
        ] {
            assert!(file_exists(&dir, expected), "missing {expected}");
        }

        for undiscovered in [
            "Auth/Strategies/PatAuthStrategy.cs",
            "Auth/Strategies/OAuth1AuthStrategy.cs",
            "Auth/Strategies/ApiKeyAuthStrategy.cs",
        ] {
            assert!(
                !file_exists(&dir, undiscovered),
                "unexpected {undiscovered}"
            );
        }
    }

    #[tokio::test]
    async fn all_four_kinds_fixture_emits_every_strategy_and_registers_each_key() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(
            dir.clone(),
            vec![
                descriptor("basicAuth", AuthSchemeKind::Basic),
                descriptor("pat", AuthSchemeKind::BearerPat),
                descriptor("oauth1", AuthSchemeKind::OAuth1),
                descriptor("oauth2", AuthSchemeKind::OAuth2),
            ],
        );

        generate_auth_strategies(&ctx).await.unwrap();

        for expected in [
            "Auth/Strategies/BasicAuthStrategy.cs",
            "Auth/Strategies/PatAuthStrategy.cs",
            "Auth/Strategies/OAuth1AuthStrategy.cs",
            "Auth/Strategies/OAuth2AuthStrategy.cs",
        ] {
            assert!(file_exists(&dir, expected), "missing {expected}");
        }
        assert!(!file_exists(&dir, "Auth/Strategies/ApiKeyAuthStrategy.cs"));

        let registrations = tokio::fs::read_to_string(dir.join("Core").join("AuthStrategies.cs"))
            .await
            .unwrap();
        assert!(registrations.contains("BasicAuthStrategy>(AuthMethod.Basic)"));
        assert!(registrations.contains("PatAuthStrategy>(AuthMethod.Pat)"));
        assert!(registrations.contains("OAuth1AuthStrategy>(AuthMethod.OAuth1)"));
        assert!(registrations.contains("OAuth2AuthStrategy>(AuthMethod.OAuth2)"));
        assert!(
            !registrations.contains("ApiKeyAuthStrategy"),
            "must not register an undiscovered strategy"
        );
        assert!(
            !registrations.contains("StubAuthStrategy>(AuthMethod.None)"),
            "must not register a fallback for AuthMethod.None when it isn't a member of the enum"
        );

        let auth_manager = tokio::fs::read_to_string(dir.join("Auth").join("AuthManager.cs"))
            .await
            .unwrap();
        assert!(auth_manager.contains("AuthMethod.OAuth1"));
    }

    #[tokio::test]
    async fn duplicate_scheme_kind_only_emits_one_strategy_file() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(
            dir.clone(),
            vec![
                descriptor("apiKeyOne", AuthSchemeKind::ApiKey),
                descriptor("apiKeyTwo", AuthSchemeKind::ApiKey),
            ],
        );

        generate_auth_strategies(&ctx).await.unwrap();

        assert!(file_exists(&dir, "Auth/Strategies/ApiKeyAuthStrategy.cs"));
    }

    #[tokio::test]
    async fn no_schemes_registers_stub_under_auth_method_none() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_auth_strategies(&ctx).await.unwrap();

        let registrations = tokio::fs::read_to_string(dir.join("Core").join("AuthStrategies.cs"))
            .await
            .unwrap();
        assert!(registrations.contains("StubAuthStrategy>(AuthMethod.None)"));

        let auth_manager = tokio::fs::read_to_string(dir.join("Auth").join("AuthManager.cs"))
            .await
            .unwrap();
        assert!(
            !auth_manager.contains("AuthMethod.OAuth1"),
            "must not reference AuthMethod.OAuth1 when it isn't a member of the enum"
        );
    }
}
