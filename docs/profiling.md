# Coverage & profiling

Diagnostic tooling for finding bottlenecks in mcpify itself (the generator's
own Rust codebase) — separate from the generated servers' own coverage/
profiling scripts (see each generated project's own README). None of this
is a correctness gate: it never runs in `ci.yml` or the `e2e` job, and a red
`cargo test` is still the only thing that blocks a merge.

## Prerequisites

```bash
cargo install cargo-llvm-cov samply
rustup component add llvm-tools-preview
samply setup   # macOS only — codesigns samply so it can attach to processes
```

## Scripts

- **`scripts/coverage.sh`** — runs `cargo-llvm-cov` across mcpify's own test
  suite. Writes `target/coverage/lcov.info` (machine-readable), `coverage.json`,
  and `target/coverage/html/index.html` (human-readable). Chosen over
  `cargo-tarpaulin` for more accurate branch coverage and no known macOS
  instrumentation issues — mcpify's own CI runs both `ubuntu-latest` and
  `macos-latest`.
- **`scripts/profile.sh`** — builds `--release` and runs a real generation
  invocation (`mcpify -i <fixture> -o <tmp> -l <language>`) under `samply`,
  which works on both macOS and Linux without `sudo`/dtrace friction (unlike
  `cargo-flamegraph`/`perf`, which is Linux-only in practice). Override the
  fixture/target language with `MCPIFY_PROFILE_FIXTURE`/`MCPIFY_PROFILE_LANGUAGE`
  env vars. Writes:
  - `target/profile/profile.json.gz` — open interactively with `samply load target/profile/profile.json.gz`.
  - `target/profile/profile.json.syms.json` — a `--unstable-presymbolicate` sidecar samply writes alongside the profile; `scripts/samply_to_text.py` uses it to resolve real function names offline (a saved samply profile's own function names are unresolved hex addresses otherwise — samply normally resolves them lazily when viewed live in-browser). Since this is an explicitly unstable samply format, the converter tolerates unresolvable entries by falling back to the raw address rather than failing.
  - `target/profile/folded-stacks.txt` — collapsed-stack text (`func;func;func count` per line), the classic flamegraph-input format.
  - `target/profile/top-functions.txt` — top 30 functions by CPU self-time (sample count).

  The profiled `mcpify` invocation's own exit status doesn't matter to this
  script — a profile is still captured and converted even if
  `run_generated_tests` fails for the fixture/language used (this script's
  job is capturing a profile, not asserting generation succeeded).

  Note: `samply record` needs the `perf_event_open` syscall, which some
  sandboxed CI environments block outright (no `CAP_PERFMON`, or the syscall
  denied regardless of `kernel.perf_event_paranoid`) — this is common on a
  subset of GitHub-hosted runners. `profile.sh` tries lowering
  `kernel.perf_event_paranoid` via `sudo sysctl` first (harmless no-op if it
  doesn't help or isn't permitted), and if samply still can't produce a
  profile, degrades gracefully: it writes placeholder
  `folded-stacks.txt`/`top-functions.txt` explaining why and exits 0 rather
  than failing the job — this workflow is diagnostic-only and never a gate,
  so a blocked profiler shouldn't turn the run red.

  Note: the profiled command itself invokes `run_generated_tests`, which for
  most targets means a real `npm install`/`cargo build`/`uv sync`/`dotnet
  restore`/`go mod tidy` plus a real embeddings-model download on first run
  (per `docs/architecture.md`'s "generation only succeeds if its tests
  pass" gate). The first `scripts/profile.sh` run is therefore slow;
  reruns reuse warmed package-manager/model caches.
- **`scripts/bottleneck-report.sh`** — runs both of the above and assembles
  `target/bottleneck-report.md`: a table of the least-covered source files
  plus the hottest functions by CPU self-time. This is the one small,
  plain-text file meant to be pasted into an LLM prompt (or handed to
  another tool) with something like "here's mcpify's own coverage gaps and
  hot paths — what should I fix or optimize?"

## Async bottlenecks (`tokio-console`)

mcpify uses `tokio` for concurrent I/O (remote OpenAPI spec fetch,
embeddings, per-target file writes). To inspect task scheduling/blocking
live:

```bash
RUSTFLAGS="--cfg tokio_unstable" cargo run --features profiling -- -i <spec> -o <out> -l <target>
```

then attach with the `tokio-console` CLI (`cargo install tokio-console`) in
another terminal. The `profiling` Cargo feature (gating the optional
`console-subscriber` dependency) is zero-cost when not enabled — normal
builds are unaffected.

## CI

`.github/workflows/perf.yml` runs `scripts/bottleneck-report.sh` on
`workflow_dispatch` only (never on push/PR) and uploads the report plus raw
coverage/profile artifacts. Trigger it from the Actions tab when you want a
fresh snapshot; it never blocks anything.
