use anyhow::Result;

use crate::auth_profile::AuthSchemeKind;
use crate::context::GeneratorContext;
use crate::targets::python::context::PyTemplateContext;
use crate::targets::python::emit::render_and_write;
use crate::targets::python::render::render_engine;

/// Always emitted, regardless of which schemes were discovered.
const SHARED_FILES: &[(&str, &str)] = &[
    ("auth/__init__.py.tera", "auth/__init__.py"),
    ("auth/auth_strategy.py.tera", "auth/auth_strategy.py"),
    ("auth/errors.py.tera", "auth/errors.py"),
    (
        "auth/request_credentials.py.tera",
        "auth/request_credentials.py",
    ),
    (
        "auth/strategies/__init__.py.tera",
        "auth/strategies/__init__.py",
    ),
    ("auth/strategies/stub.py.tera", "auth/strategies/stub.py"),
    ("auth/auth_manager.py.tera", "auth/auth_manager.py"),
];

/// One strategy module per discovered `AuthSchemeKind`; content depends
/// only on the kind, not the scheme's declared name, so kinds are
/// deduplicated before rendering (a spec can declare more than one scheme
/// of the same kind, e.g. two `apiKey` schemes). Mirrors
/// `targets::rust::steps::auth::strategy_template`.
fn strategy_template(kind: AuthSchemeKind) -> (&'static str, &'static str) {
    match kind {
        AuthSchemeKind::Basic => ("auth/strategies/basic.py.tera", "auth/strategies/basic.py"),
        AuthSchemeKind::ApiKey => (
            "auth/strategies/api_key.py.tera",
            "auth/strategies/api_key.py",
        ),
        AuthSchemeKind::BearerPat => ("auth/strategies/pat.py.tera", "auth/strategies/pat.py"),
        AuthSchemeKind::OAuth2 => (
            "auth/strategies/oauth2.py.tera",
            "auth/strategies/oauth2.py",
        ),
        AuthSchemeKind::OAuth1 => (
            "auth/strategies/oauth1.py.tera",
            "auth/strategies/oauth1.py",
        ),
    }
}

/// `generate_auth_strategies` (architecture.md §1, step 7): one strategy
/// module per discovered `AuthSchemeDescriptor`, plus the auth-manager that
/// selects the single active strategy from config at runtime.
pub async fn generate_auth_strategies(ctx: &GeneratorContext) -> Result<()> {
    let view = PyTemplateContext::from_context(ctx);
    let package_root = ctx.output_dir.join("src").join(&view.module_name);
    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in SHARED_FILES {
        render_and_write(&tera, template, &tera_ctx, &package_root.join(out_name)).await?;
    }

    let mut emitted_kinds: Vec<AuthSchemeKind> = Vec::new();
    for scheme in &ctx.auth_schemes {
        if emitted_kinds.contains(&scheme.kind) {
            continue;
        }
        emitted_kinds.push(scheme.kind);

        let (template, out_name) = strategy_template(scheme.kind);
        render_and_write(&tera, template, &tera_ctx, &package_root.join(out_name)).await?;
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
        }
    }

    // A named subdirectory (rather than the tempdir root, whose name is
    // random) so `module_name` — and therefore `package_root` below — is
    // deterministic.
    fn output_dir(parent: &tempfile::TempDir) -> PathBuf {
        parent.path().join("output")
    }

    fn file_exists(package_root: &Path, relative: &str) -> bool {
        package_root.join(relative).is_file()
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

        let package_root = dir.join("src").join("output");
        for expected in [
            "auth/__init__.py",
            "auth/auth_strategy.py",
            "auth/errors.py",
            "auth/request_credentials.py",
            "auth/auth_manager.py",
            "auth/strategies/__init__.py",
            "auth/strategies/stub.py",
            "auth/strategies/basic.py",
            "auth/strategies/oauth2.py",
        ] {
            assert!(file_exists(&package_root, expected), "missing {expected}");
        }

        for undiscovered in [
            "auth/strategies/pat.py",
            "auth/strategies/oauth1.py",
            "auth/strategies/api_key.py",
        ] {
            assert!(
                !file_exists(&package_root, undiscovered),
                "unexpected {undiscovered}"
            );
        }
    }

    #[tokio::test]
    async fn all_four_kinds_fixture_emits_every_strategy_and_no_dangling_imports() {
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

        let package_root = dir.join("src").join("output");
        for expected in [
            "auth/strategies/basic.py",
            "auth/strategies/pat.py",
            "auth/strategies/oauth1.py",
            "auth/strategies/oauth2.py",
        ] {
            assert!(file_exists(&package_root, expected), "missing {expected}");
        }
        assert!(!file_exists(&package_root, "auth/strategies/api_key.py"));

        let auth_manager = tokio::fs::read_to_string(package_root.join("auth/auth_manager.py"))
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

        let strategies_init =
            tokio::fs::read_to_string(package_root.join("auth/strategies/__init__.py"))
                .await
                .unwrap();
        assert!(!strategies_init.contains("api_key"));
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

        let package_root = dir.join("src").join("output");
        assert!(file_exists(&package_root, "auth/strategies/api_key.py"));
    }
}
