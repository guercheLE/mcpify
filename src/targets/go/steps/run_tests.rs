use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::process::Command;
use tokio::time::timeout;

use crate::context::GeneratorContext;

/// Generous relative to other targets' install/build budgets: `go mod
/// download` fetches this target's full toolchain (`mcp-go`,
/// `chromem-go`, `onnxruntime_go`, OpenTelemetry, `mattn/go-sqlite3`,
/// etc. — several with CGo native components), and `go run
/// ./cmd/populate-embeddings` includes the ~90MB `Xenova/all-MiniLM-L6-v2`
/// ONNX model download plus the ONNX Runtime session's first-inference
/// warmup (v5-implementation-plan.md's open decision #5/G8 goal text
/// flags this as a real, if smaller, analog of v1's
/// `@xenova/transformers` first-run download risk). Mirrors
/// `targets::python`'s 900s `uv sync` budget.
const GO_TIMEOUT: Duration = Duration::from_secs(900);

/// `run_generated_tests` (architecture.md §1, step 11) — the v5 launch
/// milestone. Sequenced exactly as this plan's G8 goal text specifies:
/// `go mod tidy` (fetches/reconciles the full dependency graph; more
/// robust than a plain `go mod download` since it also fixes any
/// direct/indirect annotation drift and completes `go.sum`) → `go build
/// ./...` (the compile step other targets either skip or fold into their
/// test runner — Go's static typing makes this a real, separate, valuable
/// gate) → `go run ./cmd/populate-embeddings` (must precede tests, since
/// the `chromem-go` collection starts empty — same sequencing requirement
/// v1's `populate-embeddings.ts` has relative to `npm test`; this step
/// also concretely confirms the ONNX Runtime shared library is resolvable
/// in the test environment, open decision #2) → `go test -tags=integration
/// ./...` (the `integration` tag is included here deliberately, unlike a
/// plain `go test ./...` — this is the one gate meant to catch a broken
/// embeddings pipeline for real, not just compile it). `golangci-lint run
/// ./...` (gocritic enabled via the generated `.golangci.yml`) runs
/// between the build and embeddings steps, mirroring the generated
/// project's own CI.
pub async fn run_generated_tests(ctx: &GeneratorContext) -> Result<()> {
    run_command(&ctx.output_dir, "go", &["mod", "tidy"], "go mod tidy").await?;
    run_command(&ctx.output_dir, "go", &["build", "./..."], "go build ./...").await?;
    // Mirrors the generated project's own CI (`ci.yml.tera`'s
    // `golangci-lint-action` step, gocritic enabled via `.golangci.yml`)
    // so a template that introduces a lint violation fails generation
    // itself, instead of only being caught downstream in the end user's
    // CI. Placed before the slower embeddings/model-download step so a
    // lint failure surfaces fast.
    run_command(
        &ctx.output_dir,
        "golangci-lint",
        &["run", "./..."],
        "golangci-lint run",
    )
    .await?;
    run_command(
        &ctx.output_dir,
        "go",
        &["run", "./cmd/populate-embeddings"],
        "go run ./cmd/populate-embeddings",
    )
    .await?;
    run_command(
        &ctx.output_dir,
        "go",
        &["test", "-tags=integration", "./..."],
        "go test -tags=integration ./...",
    )
    .await?;

    Ok(())
}

async fn run_command(cwd: &Path, program: &str, args: &[&str], label: &str) -> Result<()> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = command
        .spawn()
        .with_context(|| format!("failed to spawn '{label}' in '{}'", cwd.display()))?;

    let output = timeout(GO_TIMEOUT, child.wait_with_output())
        .await
        .with_context(|| format!("'{label}' timed out after {}s", GO_TIMEOUT.as_secs()))?
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
    async fn run_generated_tests_fails_fast_when_output_dir_has_no_go_mod() {
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
        assert!(err.to_string().contains("go mod tidy"));
    }
}
