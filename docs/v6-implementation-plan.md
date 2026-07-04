# mcpify: v6 Coverage, Profiling & Registry Publishing Implementation Plan

> **Status: Not started (2026-07-03).** This plan is cross-cutting — unlike v1–v5 (each adding one new output target), v6 touches the generator itself plus all 5 existing targets (TypeScript, Rust, Python, C#, Go). It does not add a new `-l <language>` option; `targets::build_registry()` is unchanged.

## Context

Two threads led here. First: do the 5 generated targets already include tests? Yes — confirmed by reading `src/targets/*/steps/setup_and_tests.rs` and `run_tests.rs` for every target. Each target's `generate_setup_wizard_and_tests` step emits unit/integration/E2E tests in its own idiomatic framework (Vitest, `cargo test`, pytest, xUnit, `go test`), and `run_generated_tests` runs them as a **hard gate** — per `docs/architecture.md:39` and PRD REQ-2.5.1, a generation run is not considered successful if the emitted tests fail. Nothing to add there.

Second: a shared technical write-up (a Gemini conversation) laid out free, cross-platform coverage + profiling tooling per language — `dotnet-trace`/`dotnet-gcdump`/a `Microsoft.Diagnostics.NETCore.Client` orchestrator + `coverlet` for .NET; `go test -coverprofile/-cpuprofile/-memprofile` + `go tool pprof`/`cover` (all stdlib) for Go; `pytest-cov`, `cProfile`/`snakeviz`, `memray`/`pytest-memray` for Python; `cargo-tarpaulin`/`cargo-llvm-cov`, `cargo-flamegraph`, `tokio-console` for Rust; `--prof`/`--heap-prof` + Vitest/Jest's built-in coverage for Node. This plan adapts that guidance to two surfaces: mcpify's own Rust/Tokio codebase (so bottlenecks in generation itself are findable), and the 5 generated servers (so end users get the same capability).

Confirmed by reading the actual code, not assumed: mcpify's CI (`.github/workflows/ci.yml`) runs on both `ubuntu-latest` and `macos-latest`, ruling out `perf`-only profiling tools for the generator's own use. None of the 5 targets' templates reference any coverage tool today (grepped across `src/targets/*/templates`) — TypeScript's `vitest.config.ts.tera` already sets `coverage: { provider: 'v8' }`, but the matching `@vitest/coverage-v8` devDependency was never added to `package.json.tera`, so `--coverage` would fail as things stand. A third thread, added mid-plan: generated Rust/Python/C#/Go servers don't publish to a package registry today (`release.yml.tera` for each has an explicit comment explaining this is deliberate — they're applications tied to one API, not reusable libraries — TypeScript is the only target that already publishes, via `npx semantic-release` → npm).

**Design principle carried through every part below:** coverage/profiling tooling is diagnostic, never a correctness gate. Nothing here touches `run_generated_tests`, the CI `e2e` job's pass/fail gate, or mcpify's own required checks (`cargo fmt --check`, `cargo clippy`, `cargo test`). It's all new, additive, opt-in-to-*run* artifacts.

---

## Part GEN — Generator-side coverage & profiling (mcpify's own codebase)

Goal: text/JSON artifacts (not just HTML/SVG) summarizing coverage gaps and hot paths, small enough to paste into an LLM prompt, without a new blocking CI gate.

### GEN1 — Coverage via `cargo-llvm-cov`
**Goal:** `scripts/coverage.sh` running `cargo llvm-cov --workspace --lcov --output-path target/coverage/lcov.info`, `--json --output-path target/coverage/coverage.json`, and `--html --output-dir target/coverage/html`. Chosen over `cargo-tarpaulin` for more accurate branch coverage and no known macOS instrumentation issues (CI runs `macos-latest`). External `cargo` subcommand — no `Cargo.toml` change.

**Depends on:** none.

---

### GEN2 — CPU profiling via `samply`
**Goal:** `scripts/profile.sh` — release build, `samply record -- ./target/release/mcpify -i <fixture> -o <tmp> -l <target>` against one of the existing multi-scheme fixtures in `tests/fixtures/openapi/` (Story 15, `docs/v1-implementation-plan.md:252`). Chosen over `cargo-flamegraph`/`perf` specifically for macOS support without `sudo`/dtrace friction, matching the dual-OS CI matrix. Also exports a folded/collapsed-stack **text** file (`func;func;func count` lines) alongside the interactive profile — the LLM-ingestable form.

**Depends on:** none.

---

### GEN3 — Async bottleneck visibility via `tokio-console`
**Goal:** Add `console-subscriber` as an *optional* dependency behind a new `profiling` Cargo feature; initialize conditionally in `src/main.rs` (`#[cfg(feature = "profiling")]`). Zero cost in normal builds; lets contributors attach `tokio-console` when debugging concurrency-shaped bottlenecks (remote spec fetch, embeddings, per-target file writes all run concurrently under `tokio`).

**Depends on:** none.

---

### GEN4 — The LLM-ingestion artifact: `bottleneck-report.sh`
**Goal:** New `scripts/bottleneck-report.sh` orchestrates GEN1 + GEN2 and assembles `target/bottleneck-report.md`: a coverage-gap summary (files/functions below a threshold, parsed from `lcov.info`) plus a top-N hot-function list (aggregated from the folded stack text via `awk`/`sort`). This single small markdown file is the artifact meant to be pasted into an LLM or handed to another tool to answer "what should I optimize or test next."

**Depends on:** GEN1, GEN2.

---

### GEN5 — CI wiring + docs
**Goal:** New `.github/workflows/perf.yml`, `workflow_dispatch`-only (never on push/PR) — runs GEN4's script, uploads `target/coverage/`, the profile artifacts, and `bottleneck-report.md`. Existing `ci.yml`/`e2e` jobs untouched. New `docs/profiling.md` explaining what each script produces, how to read `bottleneck-report.md`, and the LLM-ingestion workflow.

**Depends on:** GEN4.

---

## Part COV — Generated-server coverage (all 5 targets, always-on)

Cheap per target (one dev dependency + one config tweak + one script, no production dependency), so emitted by default via each target's existing `generate_setup_wizard_and_tests` step — not gated behind a flag. Each target gets a `scripts/coverage.sh` (new for Rust/Python/C#/Go, alongside TypeScript's existing `scripts/` dir) and a "Coverage" section in `README.md.tera`.

### COV1 — TypeScript
**Goal:** `src/targets/typescript/templates/package.json.tera` — add the missing `@vitest/coverage-v8` devDependency (closes the existing config/dependency mismatch: `vitest.config.ts.tera` already sets `coverage: { provider: 'v8' }` with nothing to back it). Extend that block with `reporter: ['text','html','json']` + `reportsDirectory: 'coverage'`. Add npm script `"test:coverage": "vitest run --coverage"`. Add `coverage/` to `.gitignore.tera`.

**Depends on:** none.

---

### COV2 — Python
**Goal:** `src/targets/python/templates/pyproject.toml.tera` — add `pytest-cov>=5` to the `dev` deps array. New `scripts/coverage.sh`: `uv run pytest --cov=<package> --cov-report=html --cov-report=json`.

**Depends on:** none.

---

### COV3 — C#
**Goal:** `src/targets/csharp/templates/Tests/*.csproj.tera` — add `coverlet.collector` PackageReference. New `scripts/coverage.sh`: `dotnet test --collect:"XPlat Code Coverage"` then `reportgenerator` (documented `dotnet tool install`) for HTML.

**Depends on:** none.

---

### COV4 — Rust
**Goal:** No `Cargo.toml.tera` dependency needed (external tool, same as GEN1). New `scripts/coverage.sh` wraps `cargo llvm-cov` identically to GEN1's invocation.

**Depends on:** none.

---

### COV5 — Go
**Goal:** Zero new dependencies (stdlib). New `scripts/coverage.sh`: `go test -coverprofile=coverage.out ./... && go tool cover -html=coverage.out -o coverage.html`.

**Depends on:** none.

---

## Part PROF — Generated-server profiling (all 5 targets, fully automated + LLM-ingestable)

Decided over a docs-only approach: every target gets real, runnable profiling automation ending in the same shape as GEN4's artifact — a `bottleneck-report.md` a user or an LLM can read to find and fix bottlenecks in the *generated* code. Heavier per-target cost (an extra project for C#, optional deps for Python/Rust) is accepted deliberately. Still opt-in-to-*run*, never wired into `run_generated_tests` or any pass/fail gate — but the tooling itself is always generated, not left as copy-paste docs.

### PROF1 — Go
**Goal:** `scripts/profile.sh`: `go test -cpuprofile=cpu.prof -memprofile=mem.prof -bench=. ./...`, then both the interactive form (`go tool pprof -http=:8080 cpu.prof`, documented) and the LLM-ingestable text form (`go tool pprof -top -text cpu.prof` and `-text mem.prof`) piped into the report.

**Depends on:** COV5.

---

### PROF2 — TypeScript
**Goal:** `scripts/profile.sh`: `node --prof --heap-prof node_modules/.bin/vitest run`, then `node --prof-process isolate-*.log > cpu-profile.txt`. New `scripts/heap-profile-summary.js` reads the `.heapprofile` JSON directly and prints the top-N allocators by retained size as text — no need to open Chrome DevTools just to get a report.

**Depends on:** COV1.

---

### PROF3 — Python
**Goal:** `scripts/profile.sh`: `uv run python -m cProfile -o profile.out -m pytest`, converted via a one-liner (`python -c "import pstats; pstats.Stats('profile.out').sort_stats('cumulative').print_stats(30)"`) to `cpu-profile.txt`. Memory: `uv run memray run -o memray-report.bin -m pytest` then `uv run memray stats memray-report.bin > mem-profile.txt` (memray's own `stats` subcommand is already text). Add `memray` + `pytest-memray` as an **optional** dependency group (`[project.optional-dependencies] profiling = [...]`) — not installed by default `uv sync`, only `uv sync --extra profiling`.

**Depends on:** COV2.

---

### PROF4 — Rust
**Goal:** `scripts/profile.sh` wraps `samply record` (same cross-platform reasoning as GEN2) against the running generated server binary driven by a scripted set of sample requests, exporting a folded/collapsed-stack text file. Add `dhat-rs` as an optional dependency behind the same `profiling` Cargo feature pattern as GEN3's `console-subscriber` — pure-library heap profiler, no external OS tool, produces `dhat-heap.json` plus a text summary on drop, working identically on both CI OSes.

**Depends on:** COV4.

---

### PROF5 — C#
**Goal:** Generate the orchestrator project from the Gemini recipe for real — new `tools/Profiler/Profiler.csproj` + `Program.cs` implementing the `Microsoft.Diagnostics.NETCore.Client` pattern (CPU sample provider + GC/allocation provider combined), invoked via `scripts/profile.sh` → `dotnet run --project tools/Profiler`. It launches `dotnet test --collect:"XPlat Code Coverage"` as the target process, records a `.nettrace`, then converts via `dotnet-trace convert --format Speedscope test_profile.nettrace` to JSON (structured, LLM/tool-ingestable) alongside the coverage XML from COV3. Needs its own build smoke-check (see PROF6) since it's a real, separately-buildable project.

**Depends on:** COV3.

---

### PROF6 — Per-target `bottleneck-report.md` + CI smoke check
**Goal:** Each target's `scripts/profile.sh` (or a sibling `scripts/bottleneck-report.sh` for targets with multiple profiling outputs, e.g. C#/Python) concatenates the COV-part coverage-gap summary with the top-N hot-path/allocator text into one small markdown file at the generated project root. Add an `#[ignore]`-by-default smoke check (likely in the `e2e` job, alongside the other slow real-toolchain checks) confirming C#'s `tools/Profiler` project actually builds and runs — its success is not a generation gate, but it shouldn't silently rot either.

**Depends on:** PROF1–PROF5.

---

## Part PUB — Opt-in registry publishing for Rust, Python, C# (Go left as-is)

Generated Rust/Python/C#/Go servers deliberately don't publish to a registry today — each `release.yml.tera` explains why (generated *applications* tied to one API, not reusable libraries; release means a GitHub Release binary/wheel/zip instead). Decision: add registry publishing for Rust, Python, and C#, but **opt-in via a new CLI flag** (default off) — unlike npm, publishing to crates.io/PyPI/NuGet is public, named, and harder to undo, so it shouldn't be a silent default the way TypeScript's `semantic-release` is. Go is left as-is: `go install module@version` already works off any public GitHub tag via the module proxy.

### PUB1 — CLI flag + `GeneratorContext` plumbing
**Goal:** `src/cli.rs` — new `#[arg(long = "publish-registry")] pub publish_registry: bool`, alongside `force`/`language`. `src/context.rs` — new `publish_registry: bool` field on `GeneratorContext`, populated by the shared pipeline the same way `force` already is, and read by each target's template-context construction so `release.yml.tera` can conditionally emit the extra step (`{% if publish_registry %}`).

**Depends on:** none.

---

### PUB2 — Rust
**Goal:** `src/targets/rust/templates/Cargo.toml.tera` — when the flag is set, flip `publish = false` to omit/`true` and add `license`/`repository` fields (placeholder values + a comment telling the user to fill them in; crates.io rejects a publish without a license). `release.yml.tera` gains a `cargo publish --token ${{ secrets.CARGO_REGISTRY_TOKEN }}` step after the existing GitHub Release step, mirroring mcpify's own `release.yml`.

**Depends on:** PUB1.

---

### PUB3 — Python
**Goal:** `src/targets/python/templates/release.yml.tera` — add a `uv publish --token ${{ secrets.UV_PUBLISH_TOKEN }}` step after `uv build`, gated by the flag.

**Depends on:** PUB1.

---

### PUB4 — C#
**Goal:** `src/targets/csharp/templates/Project.csproj.tera` — package the generated server as a **dotnet global tool** (`<PackAsTool>true</PackAsTool>`, `<ToolCommandName>{{ project_name }}</ToolCommandName>`, plus `PackageId`/`Authors`/`Version` metadata) so `dotnet tool install -g` works like `npx`/`cargo install`/`pip install` do for the others. `release.yml.tera` gains `dotnet pack -c Release` + `dotnet nuget push ... --api-key ${{ secrets.NUGET_API_KEY }}`, gated by the flag.

**Depends on:** PUB1.

---

### PUB5 — Comments + no-flag regression check
**Goal:** Update each target's `release.yml.tera` comment to explain the flag-gated behavior (why it's off by default) instead of the current "we deliberately never do this" framing. Verify (see Verification below) that generating *without* `--publish-registry` produces byte-identical output to pre-v6 generation — this must not change default behavior for existing users.

**Depends on:** PUB2, PUB3, PUB4.

---

## Cross-cutting notes

- **Golden/snapshot tests** (`tests/golden_typescript.rs`, `golden_rust.rs`, `golden_python.rs`, `golden_csharp.rs`, `golden_go.rs`) need their expected file trees updated for every new file this plan adds — follow the existing `cargo insta review` workflow (`docs/v1-implementation-plan.md:262-269`).
- Nothing in Part COV, PROF, or PUB (when the flag is off) touches `run_generated_tests` or the CI `e2e` job's pass/fail gate.
- Suggested build order: Part GEN is independent and can land first. Part COV is low-risk/low-effort per target. Part PROF is the largest remaining chunk — C#'s `tools/Profiler` project is meaningfully heavier than the other four. Part PUB can land any time after PUB1.

## Verification

- **GEN:** run `scripts/coverage.sh` and `scripts/profile.sh` locally; confirm `target/coverage/lcov.info`, `coverage.json`, and `target/bottleneck-report.md` are produced and non-empty. Confirm `perf.yml` runs green via `workflow_dispatch`.
- **COV/PROF per target:** regenerate a project (`cargo run -- -i <fixture> -o <tmp> -l <target>`), run the new `scripts/coverage.sh` and `scripts/profile.sh` for real, confirm both a coverage report and a `bottleneck-report.md` are produced. For C#, additionally run `dotnet run --project tools/Profiler` and confirm it produces both the `.nettrace`→Speedscope JSON and a coverage XML.
- **PUB:** regenerate a project for Rust/Python/C# both with and without `--publish-registry` and diff the output — confirm the flag changes `release.yml`/`Cargo.toml`/`Project.csproj`, and that the *without*-flag case is byte-identical to today's output. `cargo publish --dry-run` / `dotnet pack` (without pushing) should succeed against the generated project to confirm packaging metadata is valid.
- Re-run `cargo test` (fast suite), `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and the golden/snapshot tests after every template change; update snapshots via `cargo insta review` where new files are now expected in the output tree.
