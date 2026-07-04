use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::process::Command;
use tokio::time::timeout;

use crate::context::GeneratorContext;
use crate::targets::python::context::PyTemplateContext;

/// Generous relative to `targets::typescript`'s 300s `npm install` budget:
/// `uv sync` resolves and downloads this target's full toolchain
/// (including `torch`, `sentence-transformers`' heaviest dependency), and
/// `populate_embeddings` also needs to download the `all-mpnet-base-v2`
/// model on first run — mirrors `targets::rust`'s 900s `CARGO_TIMEOUT`,
/// which budgets for the same two costs (dependency resolution + model
/// download).
const UV_TIMEOUT: Duration = Duration::from_secs(900);

/// `run_generated_tests` (architecture.md §1, step 11): installs
/// dependencies and runs the emitted `pytest` suite to completion. A run
/// that generates code but whose tests fail (or don't run) is not a
/// successful `execute()` (PRD REQ-2.5.1). Unlike `targets::rust` (no
/// separate build step; `cargo test` compiles as part of running) but
/// like `targets::typescript` (`npm install` then `npm test` are two
/// separate steps), Python has no compile step at all — `pytest`
/// collecting and running the suite is already the full proof that the
/// generated code at least imports and runs.
pub async fn run_generated_tests(ctx: &GeneratorContext) -> Result<()> {
    run_uv_command(&ctx.output_dir, &["sync"], "uv sync").await?;

    // Templates are hand-formatted, not run through `ruff`/`black` at
    // render time, so their exact style can drift from those tools'
    // defaults over time; auto-fixing here guarantees the generated
    // project's own CI (which runs `ruff check`/`black --check`) is never
    // red on first push regardless — mirrors `targets::rust`'s
    // `cargo fmt` auto-fix in this same step.
    run_uv_command(
        &ctx.output_dir,
        &["run", "ruff", "check", "--fix", "."],
        "uv run ruff check --fix .",
    )
    .await?;
    run_uv_command(&ctx.output_dir, &["run", "black", "."], "uv run black .").await?;

    // mcp_store.db leaves the shared pipeline (Story 5/6) with an empty
    // semantic_endpoints table — vectors are computed here, not by
    // mcpify itself (see the plan's embeddings decision), so this must
    // run before `pytest`.
    let view = PyTemplateContext::from_context(ctx);
    let populate_module = format!("{}.services.populate_embeddings", view.module_name);
    run_uv_command(
        &ctx.output_dir,
        &["run", "python", "-m", &populate_module],
        "uv run python -m <module>.services.populate_embeddings",
    )
    .await?;

    run_uv_command(&ctx.output_dir, &["run", "pytest"], "uv run pytest").await?;
    Ok(())
}

async fn run_uv_command(cwd: &Path, args: &[&str], label: &str) -> Result<()> {
    let mut command = Command::new("uv");
    command
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = command
        .spawn()
        .with_context(|| format!("failed to spawn '{label}' in '{}'", cwd.display()))?;

    let output = timeout(UV_TIMEOUT, child.wait_with_output())
        .await
        .with_context(|| format!("'{label}' timed out after {}s", UV_TIMEOUT.as_secs()))?
        .with_context(|| format!("failed to run '{label}'"))?;

    if !output.status.success() {
        let stdout = tail(&String::from_utf8_lossy(&output.stdout), 4000);
        let stderr = tail(&String::from_utf8_lossy(&output.stderr), 4000);
        bail!(
            "'{label}' failed (exit {:?})\n--- stdout (tail) ---\n{stdout}\n--- stderr (tail) ---\n{stderr}",
            output.status.code(),
        );
    }

    Ok(())
}

/// Last `max_chars` characters of `s`, cut on a `char` boundary rather than
/// a byte index (which could otherwise land mid-UTF-8-sequence and panic).
fn tail(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else {
        chars[chars.len() - max_chars..].iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_returns_whole_string_when_shorter_than_limit() {
        assert_eq!(tail("hello", 100), "hello");
    }

    #[test]
    fn tail_truncates_to_the_last_n_chars() {
        assert_eq!(tail("abcdefgh", 3), "fgh");
    }

    #[test]
    fn tail_does_not_panic_on_multi_byte_characters() {
        let s = "🦀".repeat(10);
        assert_eq!(tail(&s, 3), "🦀".repeat(3));
    }

    #[tokio::test]
    async fn run_generated_tests_fails_fast_when_output_dir_has_no_pyproject_toml() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = GeneratorContext {
            publish_registry: false,
            openapi_input: "spec.yaml".to_string(),
            output_dir: dir.path().to_path_buf(),
            force: false,
            output_dir_preexisted: true,
            auth_schemes: Vec::new(),
            normalized_operations: Vec::new(),
            api_title: "Widget API".to_string(),
            version_label: "default".to_string(),
        };

        let err = run_generated_tests(&ctx).await.unwrap_err();
        assert!(err.to_string().contains("uv sync"));
    }
}
