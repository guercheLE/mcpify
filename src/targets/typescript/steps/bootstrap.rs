use anyhow::Result;

use crate::context::GeneratorContext;
use crate::targets::typescript::context::TsTemplateContext;
use crate::targets::typescript::emit::render_and_write;
use crate::targets::typescript::render::render_engine;

const SOURCE_SUBDIRS: &[&str] = &[
    "auth",
    "cli",
    "core",
    "data",
    "http",
    "services",
    "tools",
    "validation",
];

const STATIC_FILES: &[(&str, &str)] = &[
    ("package.json.tera", "package.json"),
    ("tsconfig.json.tera", "tsconfig.json"),
    ("biome.json.tera", "biome.json"),
    (".gitignore.tera", ".gitignore"),
    (".env.example.tera", ".env.example"),
    ("README.md.tera", "README.md"),
    ("LICENSE.tera", "LICENSE"),
    (".npmignore.tera", ".npmignore"),
    (".githooks/pre-commit.tera", ".githooks/pre-commit"),
];

/// Git only runs a hook file that's executable — `render_and_write` writes
/// plain, non-executable files, so the pre-commit hook needs its own
/// explicit chmod after being written.
#[cfg(unix)]
async fn make_pre_commit_hook_executable(output_dir: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let hook_path = output_dir.join(".githooks").join("pre-commit");
    let mut permissions = tokio::fs::metadata(&hook_path).await?.permissions();
    permissions.set_mode(0o755);
    tokio::fs::set_permissions(&hook_path, permissions).await?;
    Ok(())
}

#[cfg(not(unix))]
async fn make_pre_commit_hook_executable(_output_dir: &std::path::Path) -> Result<()> {
    Ok(())
}

/// `bootstrap_project` (architecture.md §1, step 5): project skeleton,
/// manifest, and config files — everything before enterprise scaffolding
/// (Story 9). `mcp_store.db` is already written directly into
/// `ctx.output_dir` by the shared pipeline (Story 5/6), so this step only
/// lays out `src/` and the root-level project files around it.
pub async fn bootstrap_project(ctx: &GeneratorContext) -> Result<()> {
    for subdir in SOURCE_SUBDIRS {
        tokio::fs::create_dir_all(ctx.output_dir.join("src").join(subdir)).await?;
    }

    let view = TsTemplateContext::from_context(ctx);
    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in STATIC_FILES {
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }
    make_pre_commit_hook_executable(&ctx.output_dir).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::auth_profile::{AuthSchemeDescriptor, AuthSchemeKind};

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

    // A named subdirectory (rather than the tempdir root, whose name is
    // random) so `project_name`/`tool_prefix_env` are deterministic.
    fn output_dir(parent: &tempfile::TempDir) -> PathBuf {
        parent.path().join("output")
    }

    #[tokio::test]
    async fn creates_every_source_subdirectory() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        bootstrap_project(&ctx).await.unwrap();

        for subdir in SOURCE_SUBDIRS {
            assert!(
                dir.join("src").join(subdir).is_dir(),
                "missing src/{subdir}"
            );
        }
    }

    #[tokio::test]
    async fn writes_every_static_file() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        bootstrap_project(&ctx).await.unwrap();

        for (_, out_name) in STATIC_FILES {
            assert!(dir.join(out_name).is_file(), "missing {out_name}");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pre_commit_hook_is_executable() {
        use std::os::unix::fs::PermissionsExt;

        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        bootstrap_project(&ctx).await.unwrap();

        let mode = tokio::fs::metadata(dir.join(".githooks").join("pre-commit"))
            .await
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "pre-commit hook is not executable");
    }

    #[tokio::test]
    async fn package_json_and_tsconfig_are_valid_json() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        bootstrap_project(&ctx).await.unwrap();

        for file in ["package.json", "tsconfig.json"] {
            let contents = tokio::fs::read_to_string(dir.join(file)).await.unwrap();
            serde_json::from_str::<serde_json::Value>(&contents)
                .unwrap_or_else(|e| panic!("{file} is not valid JSON: {e}"));
        }
    }

    #[tokio::test]
    async fn env_example_only_lists_vars_for_discovered_schemes() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(
            dir.clone(),
            vec![AuthSchemeDescriptor {
                scopes: Vec::new(),
                authorization_url: None,
                token_url: None,
                name: "oauth2".to_string(),
                kind: AuthSchemeKind::OAuth2,
                location: None,
            }],
        );

        bootstrap_project(&ctx).await.unwrap();

        let env_example = tokio::fs::read_to_string(dir.join(".env.example"))
            .await
            .unwrap();
        assert!(env_example.contains("OUTPUT_CLIENT_ID="));
        assert!(env_example.contains("OUTPUT_CLIENT_SECRET="));
        assert!(!env_example.contains("_USERNAME="));
        assert!(!env_example.contains("_CONSUMER_KEY="));
    }
}
