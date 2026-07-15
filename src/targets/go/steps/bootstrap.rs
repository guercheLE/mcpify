use anyhow::Result;

use crate::context::GeneratorContext;
use crate::targets::go::context::GoTemplateContext;
use crate::targets::go::emit::render_and_write;
use crate::targets::go::render::render_engine;

const INTERNAL_SUBDIRS: &[&str] = &[
    "internal/auth",
    "internal/cli",
    "internal/core",
    "internal/data",
    "internal/http",
    "internal/services",
    "internal/tools",
    "internal/validation",
];

const STATIC_FILES: &[(&str, &str)] = &[
    ("go.mod.tera", "go.mod"),
    (".gitignore.tera", ".gitignore"),
    (".env.example.tera", ".env.example"),
    ("README.md.tera", "README.md"),
    ("LICENSE.tera", "LICENSE"),
];

/// `bootstrap_project` (architecture.md §1, step 5): project skeleton and
/// `go.mod` manifest — everything before enterprise scaffolding (Story G3).
/// `mcp_store.db` is already written directly into `ctx.output_dir` by the
/// shared pipeline (Story 5/6), so this step only lays out the
/// `internal/`-per-concern skeleton (Go convention favors `internal/` to
/// prevent accidental external imports of implementation packages, unlike
/// every other target's plain top-level folders) plus two `cmd/` binary
/// directories: `cmd/<binary>/` (the entry point, Story G5) and the
/// sibling `cmd/populate-embeddings/` (Story G6/G8) — two separate `go
/// build` targets, not one nested under the other, matching G8's own
/// `go run ./cmd/populate-embeddings` invocation. Mirrors
/// `targets::csharp::steps::bootstrap::bootstrap_project`'s
/// folder-per-concern shape.
pub async fn bootstrap_project(ctx: &GeneratorContext) -> Result<()> {
    let view = GoTemplateContext::from_context(ctx);

    for subdir in INTERNAL_SUBDIRS {
        tokio::fs::create_dir_all(ctx.output_dir.join(subdir)).await?;
    }
    tokio::fs::create_dir_all(ctx.output_dir.join("cmd").join(&view.project_name)).await?;
    tokio::fs::create_dir_all(ctx.output_dir.join("cmd").join("populate-embeddings")).await?;

    let tera = render_engine()?;
    let tera_ctx = tera::Context::from_serialize(&view)?;

    for (template, out_name) in STATIC_FILES {
        render_and_write(&tera, template, &tera_ctx, &ctx.output_dir.join(out_name)).await?;
    }

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
    // random) so `project_name`/`module_path`/`tool_prefix_env` are
    // deterministic.
    fn output_dir(parent: &tempfile::TempDir) -> PathBuf {
        parent.path().join("output")
    }

    #[tokio::test]
    async fn creates_every_internal_subdirectory() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        bootstrap_project(&ctx).await.unwrap();

        for subdir in INTERNAL_SUBDIRS {
            assert!(dir.join(subdir).is_dir(), "missing {subdir}/");
        }
    }

    #[tokio::test]
    async fn creates_the_cmd_binary_and_populate_embeddings_directories() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        bootstrap_project(&ctx).await.unwrap();

        assert!(dir.join("cmd").join("output").is_dir());
        assert!(dir.join("cmd").join("populate-embeddings").is_dir());
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

    #[tokio::test]
    async fn writes_go_mod_with_the_kebab_case_module_path() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        bootstrap_project(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.join("go.mod")).await.unwrap();
        assert!(contents.contains("module output"));
        assert!(contents.contains("go 1.26"));
        assert!(contents.contains("github.com/mark3labs/mcp-go"));
        assert!(contents.contains("github.com/philippgille/chromem-go"));
        assert!(contents.contains("github.com/yalue/onnxruntime_go"));
        assert!(contents.contains("github.com/sugarme/tokenizer"));
    }

    #[tokio::test]
    async fn writes_the_readme_with_the_onnx_runtime_prerequisite_called_out() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(dir.clone(), Vec::new());

        bootstrap_project(&ctx).await.unwrap();

        let contents = tokio::fs::read_to_string(dir.join("README.md"))
            .await
            .unwrap();
        assert!(contents.contains("ONNX Runtime shared library"));
        assert!(contents.contains("libonnxruntime"));
    }

    #[tokio::test]
    async fn env_example_only_lists_vars_for_discovered_schemes() {
        let parent = tempfile::tempdir().unwrap();
        let dir = output_dir(&parent);
        let ctx = ctx_with_schemes(
            dir.clone(),
            vec![AuthSchemeDescriptor {
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
