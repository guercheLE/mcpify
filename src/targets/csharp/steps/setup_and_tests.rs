use anyhow::Result;

use crate::auth_profile::AuthSchemeKind;
use crate::context::GeneratorContext;
use crate::targets::csharp::context::CsTemplateContext;
use crate::targets::csharp::emit::render_and_write;
use crate::targets::csharp::render::render_engine;

/// Rendered under the main project tree (not the `Tests/` project).
const PACKAGE_FILES: &[(&str, &str)] = &[("Cli/SetupWizard.cs.tera", "Cli/SetupWizard.cs")];

/// Re-rendered here: this story's edits to the underlying `.tera`
/// templates (the `setup`/`test-connection` subcommands in `Program.cs`,
/// `Roles.RunSetupAsync`/`RunTestConnectionAsync` in `Cli/Roles.cs`, the
/// `Tests/` exclusion in the main `.csproj`) only take effect once
/// something re-renders them — the same pattern `steps::tools` already
/// established.
const RERENDERED_FILES: &[(&str, &str)] = &[
    ("Program.cs.tera", "Program.cs"),
    ("Cli/Roles.cs.tera", "Cli/Roles.cs"),
];

/// Always emitted, regardless of which auth schemes were discovered — the
/// generated xUnit suite (test framework: open decision #3).
const SHARED_TEST_FILES: &[(&str, &str)] = &[
    (
        "Tests/Auth/StubAuthStrategyTests.cs.tera",
        "Tests/Auth/StubAuthStrategyTests.cs",
    ),
    (
        "Tests/Core/ConfigTests.cs.tera",
        "Tests/Core/ConfigTests.cs",
    ),
    (
        "Tests/Data/SqliteVecStoreTests.cs.tera",
        "Tests/Data/SqliteVecStoreTests.cs",
    ),
    (
        "Tests/Validation/ValidatorTests.cs.tera",
        "Tests/Validation/ValidatorTests.cs",
    ),
    ("scripts/coverage.sh.tera", "scripts/coverage.sh"),
    ("scripts/profile.sh.tera", "scripts/profile.sh"),
    (
        "scripts/speedscope_to_text.py.tera",
        "scripts/speedscope_to_text.py",
    ),
    (
        "Tools/Profiler/Profiler.csproj.tera",
        "Tools/Profiler/Profiler.csproj",
    ),
    (
        "Tools/Profiler/Program.cs.tera",
        "Tools/Profiler/Program.cs",
    ),
];

/// One test file per discovered `AuthSchemeKind` — never emit a test
/// importing a strategy that wasn't actually generated (Story C4), or
/// `run_generated_tests` (Story C8) would fail outright on a dangling
/// reference, not just a logical test failure. Mirrors
/// `targets::python::steps::setup_and_tests::auth_test_template`.
fn auth_test_template(kind: AuthSchemeKind) -> (&'static str, &'static str) {
    match kind {
        AuthSchemeKind::Basic => (
            "Tests/Auth/BasicAuthStrategyTests.cs.tera",
            "Tests/Auth/BasicAuthStrategyTests.cs",
        ),
        AuthSchemeKind::ApiKey => (
            "Tests/Auth/ApiKeyAuthStrategyTests.cs.tera",
            "Tests/Auth/ApiKeyAuthStrategyTests.cs",
        ),
        AuthSchemeKind::BearerPat => (
            "Tests/Auth/PatAuthStrategyTests.cs.tera",
            "Tests/Auth/PatAuthStrategyTests.cs",
        ),
        AuthSchemeKind::OAuth2 => (
            "Tests/Auth/OAuth2AuthStrategyTests.cs.tera",
            "Tests/Auth/OAuth2AuthStrategyTests.cs",
        ),
        AuthSchemeKind::OAuth1 => (
            "Tests/Auth/OAuth1SignerTests.cs.tera",
            "Tests/Auth/OAuth1SignerTests.cs",
        ),
    }
}

/// `generate_setup_wizard_and_tests` (architecture.md §1, step 10): the
/// interactive `setup`/`test-connection` commands and the generated
/// xUnit suite, exercising Stories C2-C6.
pub async fn generate_setup_wizard_and_tests(ctx: &GeneratorContext) -> Result<()> {
    let view = CsTemplateContext::from_context(ctx);
    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in PACKAGE_FILES {
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }

    for (template, out_name) in RERENDERED_FILES {
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }

    render_and_write(
        &tera,
        "Project.csproj.tera",
        &tera_ctx,
        &ctx.output_dir.join(format!("{}.csproj", view.namespace)),
    )
    .await?;

    render_and_write(
        &tera,
        "Tests/Project.Tests.csproj.tera",
        &tera_ctx,
        &ctx.output_dir
            .join("Tests")
            .join(format!("{}.Tests.csproj", view.namespace)),
    )
    .await?;

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
        }
    }

    fn output_dir(parent: &tempfile::TempDir) -> PathBuf {
        parent.path().join("output")
    }

    fn file_exists(dir: &Path, relative: &str) -> bool {
        dir.join(relative).is_file()
    }

    /// The canonical manual `dotnet` sanity check for C2-C7 combined —
    /// C2-C6 each previously had their own narrower version of this test
    /// (bootstrap-only, bootstrap+enterprise, etc.), but every one of
    /// them re-rendered `Program.cs.tera`/`Core/McpServer.cs.tera`/
    /// `Http/HttpServer.cs.tera` (there is exactly one authored copy of
    /// each, edited in place story-by-story), so once a later story
    /// extended those templates (C5's `Cli.Roles` reference in
    /// `Program.cs`, chiefly), the earlier stories' narrower tests
    /// silently stopped reflecting a buildable state — they were removed
    /// rather than kept in permanent lockstep with every later story's
    /// edits. This one (plus `tests/e2e_generation.rs`'s real
    /// `CSharpTargetGenerator::execute()` gate) is the one that matters.
    #[tokio::test]
    #[ignore = "manual sanity check: requires the dotnet SDK; not part of CI until C9 wires .NET into the pipeline"]
    async fn full_scaffold_through_c7_passes_dotnet_test() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let generator_ctx = ctx_with_schemes(
            dir.clone(),
            vec![
                descriptor("basicAuth", AuthSchemeKind::Basic),
                descriptor("apiKey", AuthSchemeKind::ApiKey),
                descriptor("pat", AuthSchemeKind::BearerPat),
                descriptor("oauth1", AuthSchemeKind::OAuth1),
                descriptor("oauth2", AuthSchemeKind::OAuth2),
            ],
        );

        crate::targets::csharp::steps::bootstrap::bootstrap_project(&generator_ctx)
            .await
            .unwrap();
        crate::targets::csharp::steps::enterprise::generate_enterprise_scaffolding(&generator_ctx)
            .await
            .unwrap();
        crate::targets::csharp::steps::auth::generate_auth_strategies(&generator_ctx)
            .await
            .unwrap();
        crate::targets::csharp::steps::transports::generate_transports_and_roles(&generator_ctx)
            .await
            .unwrap();
        crate::targets::csharp::steps::tools::generate_mcp_tools(&generator_ctx)
            .await
            .unwrap();
        generate_setup_wizard_and_tests(&generator_ctx)
            .await
            .unwrap();

        let build_status = std::process::Command::new("dotnet")
            .arg("build")
            .current_dir(&dir)
            .status()
            .unwrap();
        assert!(build_status.success(), "dotnet build failed");

        let test_status = std::process::Command::new("dotnet")
            .args(["test", "Tests"])
            .current_dir(&dir)
            .status()
            .unwrap();
        assert!(test_status.success(), "dotnet test failed");

        let format_status = std::process::Command::new("dotnet")
            .args(["format", "--verify-no-changes"])
            .current_dir(&dir)
            .status()
            .unwrap();
        assert!(
            format_status.success(),
            "dotnet format --verify-no-changes failed"
        );
    }

    /// v6 Part PROF: `Tools/Profiler` is a real, separately-buildable
    /// project (not just docs), so it needs its own smoke check — its
    /// success is diagnostic tooling, not a generation gate, so this stays
    /// `#[ignore]` like the check above rather than part of the fast suite.
    #[tokio::test]
    #[ignore = "manual sanity check: requires the dotnet SDK"]
    async fn tools_profiler_project_builds() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let generator_ctx = ctx_with_schemes(
            dir.clone(),
            vec![descriptor("basicAuth", AuthSchemeKind::Basic)],
        );

        crate::targets::csharp::steps::bootstrap::bootstrap_project(&generator_ctx)
            .await
            .unwrap();
        crate::targets::csharp::steps::enterprise::generate_enterprise_scaffolding(&generator_ctx)
            .await
            .unwrap();
        crate::targets::csharp::steps::auth::generate_auth_strategies(&generator_ctx)
            .await
            .unwrap();
        crate::targets::csharp::steps::transports::generate_transports_and_roles(&generator_ctx)
            .await
            .unwrap();
        crate::targets::csharp::steps::tools::generate_mcp_tools(&generator_ctx)
            .await
            .unwrap();
        generate_setup_wizard_and_tests(&generator_ctx)
            .await
            .unwrap();

        let build_status = std::process::Command::new("dotnet")
            .args(["build", "Tools/Profiler"])
            .current_dir(&dir)
            .status()
            .unwrap();
        assert!(build_status.success(), "dotnet build Tools/Profiler failed");
    }

    #[tokio::test]
    async fn writes_the_setup_wizard_and_every_shared_test_file() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_setup_wizard_and_tests(&ctx).await.unwrap();

        assert!(file_exists(&dir, "Cli/SetupWizard.cs"));
        assert!(file_exists(&dir, "Output.csproj"));
        assert!(file_exists(&dir, "Tests/Output.Tests.csproj"));
        for (_, out_name) in SHARED_TEST_FILES {
            assert!(file_exists(&dir, out_name), "missing {out_name}");
        }
    }

    #[tokio::test]
    async fn tests_csproj_references_the_main_project() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_setup_wizard_and_tests(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.join("Tests").join("Output.Tests.csproj"))
            .await
            .unwrap();
        assert!(contents.contains("..\\Output.csproj"));
    }

    #[tokio::test]
    async fn main_csproj_excludes_the_tests_folder() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_setup_wizard_and_tests(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.join("Output.csproj"))
            .await
            .unwrap();
        assert!(contents.contains("<Compile Remove=\"Tests/**\" />"));
    }

    #[tokio::test]
    async fn program_cs_dispatches_setup_and_test_connection() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        generate_setup_wizard_and_tests(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.join("Program.cs"))
            .await
            .unwrap();
        assert!(contents.contains("\"setup\""));
        assert!(contents.contains("\"test-connection\""));
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
            "Tests/Auth/BasicAuthStrategyTests.cs",
            "Tests/Auth/OAuth2AuthStrategyTests.cs",
        ] {
            assert!(file_exists(&dir, expected), "missing {expected}");
        }
        for undiscovered in [
            "Tests/Auth/PatAuthStrategyTests.cs",
            "Tests/Auth/OAuth1SignerTests.cs",
            "Tests/Auth/ApiKeyAuthStrategyTests.cs",
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

        assert!(file_exists(&dir, "Tests/Auth/ApiKeyAuthStrategyTests.cs"));
    }
}
