use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

use crate::context::GeneratorContext;
use crate::progress;

const NPM_TIMEOUT: Duration = Duration::from_secs(300);

/// `run_generated_tests` (architecture.md §1, step 11): installs
/// dependencies and runs the emitted test suite to completion. A run that
/// generates code but whose production build or tests fail (or don't run) is
/// not a successful `execute()` (PRD REQ-2.5.1). Vitest transpiles test imports
/// without enforcing every production TypeScript diagnostic, so `npm run
/// build` is a separate, mandatory gate.
pub async fn run_generated_tests(ctx: &GeneratorContext) -> Result<()> {
    run_npm_command(&ctx.output_dir, &["install"], "npm install").await?;
    // Templates are hand-formatted, not run through Biome at render
    // time, so their exact style/lint-cleanliness can drift over time;
    // auto-fixing here guarantees the generated project's own CI (which
    // runs `lint`/`format:check`) is never red on first push regardless
    // — mirrors `targets::python`'s `ruff check --fix` → `black` order.
    run_npm_command(&ctx.output_dir, &["run", "lint:fix"], "npm run lint:fix").await?;
    run_npm_command(&ctx.output_dir, &["run", "format"], "npm run format").await?;
    run_npm_command(&ctx.output_dir, &["run", "build"], "npm run build").await?;
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
    run_npm_command(
        &ctx.output_dir,
        &["pack", "--dry-run", "--json"],
        "npm pack --dry-run",
    )
    .await?;
    crate::package_preflight::enforce_project_limit(ctx)
}

async fn run_npm_command(cwd: &Path, args: &[&str], label: &str) -> Result<()> {
    let mut command = Command::new("npm");
    command
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if progress::enabled() {
        eprintln!("  -> running '{label}'...");
    }

    let mut child = command
        .spawn()
        .with_context(|| format!("failed to spawn '{label}' in '{}'", cwd.display()))?;

    let mut stdout_pipe = child.stdout.take().expect("stdout is piped");
    let mut stderr_pipe = child.stderr.take().expect("stderr is piped");

    let (stdout, stderr, status) = timeout(NPM_TIMEOUT, async {
        tokio::try_join!(
            drain(&mut stdout_pipe, false),
            drain(&mut stderr_pipe, true),
            child.wait(),
        )
    })
    .await
    .with_context(|| format!("'{label}' timed out after {}s", NPM_TIMEOUT.as_secs()))?
    .with_context(|| format!("failed to run '{label}'"))?;

    if !status.success() {
        let stdout = tail(&String::from_utf8_lossy(&stdout), 4000);
        let stderr = tail(&String::from_utf8_lossy(&stderr), 4000);
        bail!(
            "'{label}' failed (exit {:?})\n--- stdout (tail) ---\n{stdout}\n--- stderr (tail) ---\n{stderr}",
            status.code(),
        );
    }

    Ok(())
}

/// Reads `pipe` to completion, echoing every chunk to the real stdout/
/// stderr as it arrives when progress output is enabled (so a human
/// watching `mcpify` run sees `npm`'s own output live instead of a
/// multi-minute silence), while always accumulating the full bytes —
/// mirrors `Child::wait_with_output()`'s capture semantics, just
/// observable in real time too.
async fn drain(
    pipe: &mut (impl tokio::io::AsyncRead + Unpin),
    is_stderr: bool,
) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = pipe.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        if progress::enabled() {
            use std::io::Write;
            if is_stderr {
                std::io::stderr().write_all(&chunk[..n])?;
            } else {
                std::io::stdout().write_all(&chunk[..n])?;
            }
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Ok(buf)
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
        assert!(err.to_string().contains("npm install"));
    }
}
