use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

use crate::context::GeneratorContext;
use crate::progress;

/// Generous relative to other targets' install/build budgets: `dotnet
/// restore` fetches this target's full toolchain (the MCP SDK, ONNX
/// Runtime, OpenTelemetry, Serilog, etc.), several of which ship sizeable
/// native binaries — mirrors `targets::python`'s 900s `uv sync` budget,
/// which accounts for a comparably heavy dependency set.
const DOTNET_TIMEOUT: Duration = Duration::from_secs(900);

/// `run_generated_tests` (architecture.md §1, step 11): restores,
/// auto-formats, and runs the emitted xUnit suite (`Tests/`) to
/// completion. `dotnet test` compiles both the main project and `Tests/`
/// as a prerequisite via the `ProjectReference` Story C7 wired up, so
/// — like `targets::rust`'s `cargo test` — there's no separate "does it
/// build" check needed. A run that generates code but whose tests fail
/// (or don't run) is not a successful `execute()` (PRD REQ-2.5.1).
///
/// Deliberately does not shell out to `populate-embeddings` first (unlike
/// `targets::python`'s equivalent step, which backfills
/// `semantic_endpoints` before `pytest`): doing so here would require the
/// sqlite-vec native `vec0` library to already be present next to the
/// built assembly, which nothing in the generation pipeline fetches yet
/// (see `Data/SqliteVecStore.cs`'s doc comment — the Dockerfile's build
/// stage is the one place that's expected to provide it). The generated
/// xUnit suite is deliberately hermetic (no DB/native-extension
/// dependency) so this gate doesn't require that native library either;
/// real search-quality verification against a populated store is a
/// manual check (see this plan's Verification section), not part of this
/// automated gate.
pub async fn run_generated_tests(ctx: &GeneratorContext) -> Result<()> {
    run_dotnet_command(&ctx.output_dir, &["restore"], "dotnet restore").await?;

    // Templates are hand-formatted, not run through `dotnet format` at
    // render time, so their exact style can drift from its defaults over
    // time; auto-fixing here guarantees the generated project's own CI
    // (which runs `dotnet format --verify-no-changes`) is never red on
    // first push regardless — mirrors `targets::python`'s `ruff --fix`/
    // `black` auto-fix in this same step.
    run_dotnet_command(&ctx.output_dir, &["format"], "dotnet format").await?;

    // `-p:TreatWarningsAsErrors=true` makes Roslynator (and any other
    // analyzer) violations fail this step, without adding
    // `TreatWarningsAsErrors` to the shipped `.csproj` itself — an end
    // user's own local `dotnet build` stays unaffected. `WarningsNotAsErrors`
    // carves out NU1903 specifically: Project.csproj.tera's own NU1903 note
    // already documents this as a known, currently-unpatchable advisory on a
    // transitive dependency (SQLitePCLRaw.lib.e_sqlite3) — without this
    // carve-out, the blanket warnaserror flag turns that pre-existing,
    // deliberately-tolerated warning into a hard failure on every
    // generation, which isn't what this gate is meant to catch.
    run_dotnet_command(
        &ctx.output_dir,
        &[
            "test",
            "Tests",
            "-p:TreatWarningsAsErrors=true",
            "-p:WarningsNotAsErrors=NU1903",
        ],
        "dotnet test Tests",
    )
    .await?;

    if ctx.publish_registry {
        run_dotnet_command(
            &ctx.output_dir,
            &["pack", "--no-restore"],
            "dotnet pack --no-restore",
        )
        .await?;
    }
    if ctx.publish_registry {
        crate::package_preflight::enforce_artifact_limit(ctx, &["nupkg"])
    } else {
        crate::package_preflight::enforce_project_limit(ctx)
    }
}

async fn run_dotnet_command(cwd: &Path, args: &[&str], label: &str) -> Result<()> {
    let mut command = Command::new("dotnet");
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

    let (stdout, stderr, status) = timeout(DOTNET_TIMEOUT, async {
        tokio::try_join!(
            drain(&mut stdout_pipe, false),
            drain(&mut stderr_pipe, true),
            child.wait(),
        )
    })
    .await
    .with_context(|| format!("'{label}' timed out after {}s", DOTNET_TIMEOUT.as_secs()))?
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
/// watching `mcpify` run sees `dotnet`'s own output live instead of a
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
    async fn run_generated_tests_fails_fast_when_output_dir_has_no_csproj() {
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
        assert!(err.to_string().contains("dotnet restore"));
    }
}
