use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::process::Command;
use tokio::time::timeout;

use crate::context::GeneratorContext;

const NPM_TIMEOUT: Duration = Duration::from_secs(300);

/// `run_generated_tests` (architecture.md §1, step 11): installs
/// dependencies and runs the emitted test suite to completion. A run that
/// generates code but whose tests fail (or don't run) is not a successful
/// `execute()` (PRD REQ-2.5.1) — there is no separate "does it build" step,
/// since vitest running at all already requires the TS source to at least
/// type-check/import cleanly.
pub async fn run_generated_tests(ctx: &GeneratorContext) -> Result<()> {
    run_npm_command(&ctx.output_dir, &["install"], "npm install").await?;
    // Templates are hand-formatted, not run through Prettier at render
    // time, so their exact style can drift from .prettierrc.json over
    // time; auto-fixing here guarantees the generated project's own CI
    // (which runs `format:check`) is never red on first push regardless.
    run_npm_command(&ctx.output_dir, &["run", "format"], "npm run format").await?;
    // mcp_store.db leaves Story 5 with an empty semantic_endpoints table —
    // vectors are computed here, in TypeScript, not by mcpify itself (see
    // the plan's embeddings decision), so this must run before `npm test`.
    run_npm_command(
        &ctx.output_dir,
        &["run", "populate-embeddings"],
        "npm run populate-embeddings",
    )
    .await?;
    run_npm_command(&ctx.output_dir, &["test"], "npm test").await?;
    Ok(())
}

async fn run_npm_command(cwd: &Path, args: &[&str], label: &str) -> Result<()> {
    let mut command = Command::new("npm");
    command
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = command
        .spawn()
        .with_context(|| format!("failed to spawn '{label}' in '{}'", cwd.display()))?;

    let output = timeout(NPM_TIMEOUT, child.wait_with_output())
        .await
        .with_context(|| format!("'{label}' timed out after {}s", NPM_TIMEOUT.as_secs()))?
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
    async fn run_generated_tests_fails_fast_when_output_dir_has_no_package_json() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = GeneratorContext {
            openapi_input: "spec.yaml".to_string(),
            output_dir: dir.path().to_path_buf(),
            force: false,
            output_dir_preexisted: true,
            auth_schemes: Vec::new(),
            normalized_operations: Vec::new(),
            api_title: "Widget API".to_string(),
        };

        let err = run_generated_tests(&ctx).await.unwrap_err();
        assert!(err.to_string().contains("npm install"));
    }
}
