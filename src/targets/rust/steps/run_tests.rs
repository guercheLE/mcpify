use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::process::Command;
use tokio::time::timeout;

use crate::context::GeneratorContext;

/// Generous relative to `targets::typescript`'s 300s `npm install` budget:
/// a cold Cargo registry has ~150 crates to fetch and compile for this
/// target's toolchain (rmcp, fastembed/ort, rsa, axum, ...), and
/// `populate-embeddings` may also need to download the embedding model on
/// first run (mirroring `@xenova/transformers`' own first-run download).
const CARGO_TIMEOUT: Duration = Duration::from_secs(900);

/// `run_generated_tests` (architecture.md §1, step 11): runs the emitted
/// test suite to completion. A run that generates code but whose tests
/// fail (or don't run) is not a successful `execute()` (PRD REQ-2.5.1) —
/// there is no separate "does it build" step, since `cargo test` cannot
/// run against code that fails to compile either. Unlike
/// `targets::typescript`'s `run_generated_tests`, there's no separate
/// `npm install`-equivalent command: Cargo resolves and builds
/// dependencies as part of `cargo test` itself.
pub async fn run_generated_tests(ctx: &GeneratorContext) -> Result<()> {
    // Templates are hand-formatted, not run through `cargo fmt` at render
    // time, so their exact style can drift from rustfmt's defaults over
    // time; auto-fixing here guarantees the generated project's own CI
    // (which runs `cargo fmt --check`) is never red on first push
    // regardless.
    run_cargo_command(&ctx.output_dir, &["fmt"], "cargo fmt").await?;
    // Mirrors the generated project's own CI (`ci.yml.tera`'s
    // `cargo clippy --all-targets -- -D warnings`) so a template that
    // introduces a clippy violation fails generation itself, instead of
    // only being caught downstream in the end user's CI.
    run_cargo_command(
        &ctx.output_dir,
        &["clippy", "--all-targets", "--", "-D", "warnings"],
        "cargo clippy",
    )
    .await?;
    // mcp_store.db leaves the shared pipeline (Story 5/6) with an empty
    // semantic_endpoints table — vectors are computed here, not by
    // mcpify itself (see the plan's embeddings decision), so this must
    // run before `cargo test`.
    run_cargo_command(
        &ctx.output_dir,
        &["run", "--bin", "populate-embeddings"],
        "cargo run --bin populate-embeddings",
    )
    .await?;
    run_cargo_command(&ctx.output_dir, &["test"], "cargo test").await?;
    Ok(())
}

async fn run_cargo_command(cwd: &Path, args: &[&str], label: &str) -> Result<()> {
    let mut command = Command::new("cargo");
    command
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = command
        .spawn()
        .with_context(|| format!("failed to spawn '{label}' in '{}'", cwd.display()))?;

    let output = timeout(CARGO_TIMEOUT, child.wait_with_output())
        .await
        .with_context(|| format!("'{label}' timed out after {}s", CARGO_TIMEOUT.as_secs()))?
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
    async fn run_generated_tests_fails_fast_when_output_dir_has_no_cargo_toml() {
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
        };

        let err = run_generated_tests(&ctx).await.unwrap_err();
        assert!(err.to_string().contains("cargo fmt"));
    }
}
