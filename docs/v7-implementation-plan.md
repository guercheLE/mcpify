# mcpify: v7 Generation-Time Lint/Format Enforcement Implementation Plan

> **Status: Complete (2026-07-04).** Rust, C#, Go, and TypeScript now run their linter for real during mcpify's own `run_generated_tests` (not just in the generated project's own CI), and Go/C#/TypeScript's specific tools were upgraded per an explicit request: golangci-lint with gocritic for Go (net-new), Roslynator for C# (net-new), and Biome replacing ESLint+Prettier for TypeScript. Python needed no changes — it already ran both `ruff check --fix` and `black` against freshly generated code (`src/targets/python/steps/run_tests.rs`), which was the reference example this plan generalized to the other four targets. Real bugs found and fixed along the way, each confirmed against a live toolchain, not assumed: blanket `-p:TreatWarningsAsErrors=true` turned C#'s pre-existing, already-accepted `NU1903` vulnerability-advisory warning (on a transitive dependency with no patch published upstream) into a hard failure on every single generation, fixed by carving it out via `-p:WarningsNotAsErrors=NU1903`; Roslynator's `RCS1194` ("Implement exception constructors") analyzer doesn't understand C# 12's semicolon-bodied primary-constructor class syntax (`public class AuthException(string message) : Exception(message);`), and `dotnet format` auto-applying its fix corrupted the file into invalid C# — fixed by disabling the rule via a new `.editorconfig`; and golangci-lint surfaced 23 real issues across the Go templates (14 `errcheck` unchecked-`Close`/`Destroy`-error findings, plus `gocritic`'s `exitAfterDefer`, `httpNoBody`, `sprintfQuotedString`, `filepathJoin`, and `unnamedResult`), all fixed rather than suppressed except two stylistic false positives.

## Context

Prompted by a question about whether `clippy` covered generated Rust code too (it didn't, beyond the generated project's own CI) — which turned into an audit of every target's linter/formatter story: TypeScript (ESLint), C# (no analyzer beyond `dotnet format`), Go (`gofmt`/`go vet` only), and Python (`ruff`+`black`, already correct). Confirmed by reading each target's `steps/run_tests.rs`: mcpify's own generation pipeline (`run_generated_tests`, `architecture.md` §1 step 11) is where every target already runs its formatter as an auto-fixing hard gate before the emitted test suite runs — but for Rust, C#, Go, and TypeScript, the *linter* either didn't run there at all (Go, TypeScript's ESLint) or only ran as an unenforced package addition in CI (Rust's clippy already had a generated-CI step; C# had none). A template regression that introduced a lint violation would ship silently and only be caught downstream, in the end user's own CI run.

Follow-up requests specified the exact tool for three of the four targets: **Biome** for TypeScript (full replacement of both ESLint and Prettier — one tool, one config), **golangci-lint with gocritic** for Go (net-new; Go had no static-analysis linter beyond `go vet` at all), and **Roslynator** for C# (net-new, alongside the existing `dotnet format`). Rust kept clippy — it already existed in the generated CI, so the change there was purely "run it during generation too."

Three judgment calls were confirmed with the user before implementation:
- **C#/Roslynator: hard gate.** Violations fail the build via `-p:TreatWarningsAsErrors=true` passed as an MSBuild property at verification time (both mcpify's own pipeline and the generated CI), not baked into the shipped `.csproj` — so an end user's own local `dotnet build` isn't unexpectedly stricter than before.
- **Go/golangci-lint: assume preinstalled.** Same tier as this target's `cargo`/`uv`/`dotnet` counterparts — no auto-install logic in `run_tests.rs`. mcpify's own repo-root CI installs it via `go install` (not a piped shell-install script — an earlier attempt using `curl | sh` was flagged and reverted).
- **TypeScript/Biome: full replacement.** Both `eslint.config.js` and `.prettierrc.json` are gone; Biome handles lint and format from one `biome.json`.

---

## Part RUST — run `cargo clippy` during generation

**Done:** `src/targets/rust/steps/run_tests.rs`'s `run_generated_tests` gained one more `run_cargo_command` call — `cargo clippy --all-targets -- -D warnings`, matching the generated CI's existing invocation exactly (`src/targets/rust/templates/.github/workflows/ci.yml.tera`). Checked-only, no `--fix`: clippy fixes aren't as safe to auto-apply as formatting, and the templates already carried the two `#[allow(clippy::missing_transmute_annotations)]` suppressions needed to pass cleanly (`src/targets/rust/templates/data/store.rs.tera:28,158`). No new file, no new trait method, no rendered-output changes — golden snapshots were unaffected.

**Verified:** `cargo build`/`cargo fmt --check`/`cargo clippy --all-targets -- -D warnings` on mcpify's own codebase; full `cargo test` suite green.

---

## Part CSHARP — add Roslynator, enforce as a hard gate

**Done:**
- Added `Roslynator.Analyzers` (v4.15.0, confirmed current via NuGet at implementation time) to `src/targets/csharp/templates/Project.csproj.tera` as a `PrivateAssets="all"` package reference.
- `src/targets/csharp/steps/run_tests.rs`'s `dotnet test Tests` call and `src/targets/csharp/templates/.github/workflows/ci.yml.tera`'s `dotnet build --no-restore` step both gained `-p:TreatWarningsAsErrors=true -p:WarningsNotAsErrors=NU1903`.
- Added `src/targets/csharp/templates/.editorconfig.tera`, registered in `bootstrap.rs`'s `STATIC_FILES`, disabling `RCS1194` with a comment explaining both why (this codebase's domain exceptions are deliberately concise, not general-purpose) and the concrete corruption bug it caused.

**Bugs found and fixed** (both only surfaced by actually running the real, `#[ignore]`d `dotnet restore`/`dotnet test` path against generated output, not by unit tests):
1. The blanket warnings-as-errors flag caught `NU1903` (a pre-existing, already-documented, currently-unpatchable advisory on the transitive `SQLitePCLRaw.lib.e_sqlite3` dependency) and failed every generation. Fixed via the `WarningsNotAsErrors` carve-out above.
2. `RCS1194` ("Implement exception constructors") fired on 8 locations across `AuthErrors.cs.tera`/`Errors.cs.tera`; its auto-fix, applied through `dotnet format`, doesn't understand C# 12 semicolon-bodied primary-constructor classes and rewrote them into syntactically invalid C# (`CS1514`/`CS1513`/`CS1597`). Fixed by disabling the rule via `.editorconfig` rather than restructuring every exception type to the 4-constructor pattern it wants.

**Verified:** reproduced the corruption directly (`cargo run -- --language csharp ...` against `tests/fixtures/openapi/minimal-with-auth.yaml`, inspecting the mangled `Auth/AuthErrors.cs` on disk), confirmed the fix by regenerating and inspecting the file post-`dotnet format`, then ran the real `cargo test --test e2e_generation generates_a_csharp_project_and_passes_its_own_test_suite -- --ignored` end-to-end (passes). Golden snapshots regenerated for the new `.editorconfig` file and the `Project.csproj.tera` diff.

---

## Part GO — add golangci-lint with gocritic

**Done:**
- New `src/targets/go/templates/.golangci.yml.tera` (v2 config format, `gocritic` enabled via `enabled-tags: [diagnostic, style, opinionated]`), registered in `enterprise.rs`'s `ROOT_FILES`.
- `src/targets/go/templates/.github/workflows/ci.yml.tera` gained a `golangci/golangci-lint-action@v6` step.
- `src/targets/go/steps/run_tests.rs`'s `run_go_command` was generalized to `run_command(cwd, program, args, label)` (program name now a parameter, contained to this one file) and a `golangci-lint run ./...` call added between `go build` and the embeddings step.
- `tests/go_build_smoke.rs` (the fast, non-`#[ignore]`'d sanity test that already ran `gofmt`/`go vet`/`go build` against a rendered fixture on every `cargo test`) gained the same `golangci-lint run ./...` check — this is the one that actually runs on every PR.
- mcpify's own repo-root `.github/workflows/ci.yml` fast job now installs golangci-lint via `go install github.com/golangci/golangci-lint/v2/cmd/golangci-lint@v2.0.0` (not `golangci-lint-action`, since that action expects to lint a real Go module at the repo root, and this repo's root is Rust).

**Bugs found and fixed** (golangci-lint was installed locally specifically to get real signal, rather than trusting the config would work untested): 23 issues on first run against the `minimal-multi-scheme` fixture —
- **errcheck (14):** unchecked error returns from deferred/bare `Close()`/`Destroy()` calls across `cmd/populate-embeddings/main.go.tera`, `internal/cli/roles.go.tera`, `internal/data/store.go.tera`, `internal/services/apiclient.go.tera`, and `internal/services/embedding.go.tera` (7 of the 14 in this one file) — fixed by wrapping each in `defer func() { _ = x.Close() }()` (or `_ = x.Close()` for non-deferred calls).
- **gocritic `exitAfterDefer` (1):** `cmd/populate-embeddings/main.go.tera`'s `main()` called `os.Exit(1)` in the same scope as `defer store.Close()`/`defer embedSvc.Close()`, so those defers would never run on an error exit. Fixed by splitting into `main()`/`run() error`, the idiomatic Go pattern.
- **gocritic `httpNoBody` (4):** `http.NewRequestWithContext(..., nil)` calls in `roles.go.tera`, `setup.go.tera`, and `embedding.go.tera` (2 occurrences) — changed `nil` to `http.NoBody`.
- **gocritic `sprintfQuotedString` (1):** `oauth1.go.tera`'s `fmt.Sprintf(`%s="%s"`, ...)` — changed to `%s=%q`.
- **gocritic `filepathJoin` (1):** `config.go.tera`'s `filepath.Join("/etc", ...)` — a real false positive (the leading `/etc` is intentionally absolute), suppressed with a scoped `//nolint:gocritic` comment.
- **gocritic `unnamedResult` (4, one only surfacing after the first three were suppressed):** `credentialstorage.go.tera`'s four `Get(key string) (string, bool, error)` methods — naming the return values would collide with each method's own local `:=` declarations of the same names, so suppressed with scoped `//nolint:gocritic` comments rather than renamed.
- Also swapped one `interface{}` for `any` in `apiclient.go.tera` (not itself flagged by this config, but a trivial, safe, idiomatic cleanup found while auditing the same file).

**Verified:** golangci-lint installed locally (`go install .../golangci-lint@v2.0.0`) specifically to get this signal rather than shipping an unverified config; `cargo test --test go_build_smoke` passes clean after all fixes; full `cargo test` suite green with `golangci-lint` on `PATH`.

---

## Part TYPESCRIPT — replace ESLint+Prettier with Biome

**Done:**
- New `src/targets/typescript/templates/biome.json.tera`; deleted `eslint.config.js.tera` and `.prettierrc.json.tera`.
- `bootstrap.rs`'s `STATIC_FILES` updated accordingly.
- `package.json.tera`: `@eslint/js`/`eslint`/`prettier`/`typescript-eslint` devDependencies replaced with a single `@biomejs/biome` (v2.5.2); `lint`/`lint:fix`/`format`/`format:check` script *names* unchanged (so `ci.yml.tera` needed no edits), commands repointed to `biome lint`/`biome format`.
- `src/targets/typescript/steps/run_tests.rs`'s `run_generated_tests` gained an `npm run lint:fix` call ahead of the existing `npm run format`, mirroring Python's `ruff check --fix` → `black` ordering — TypeScript previously ran its formatter during generation but never its linter.

**Verified:** golden snapshots regenerated for the file-tree and `package.json` diffs; full `cargo test` suite green.

---

## Cross-cutting notes

- Golden/snapshot tests (`golden_csharp.rs`, `golden_go.rs`, `golden_typescript.rs`) needed updated snapshots for every new/removed template file and every changed curated-content check — same `cargo insta review`-equivalent workflow as prior plans (no `cargo-insta` CLI installed in this environment; `.snap.new` files were reviewed by diff and moved over the `.snap` originals by hand).
- `golden_rust.rs` needed no snapshot changes — clippy's addition to `run_tests.rs` doesn't touch rendered template output.
- Real end-to-end verification (installing `golangci-lint` locally, running the `#[ignore]`'d C# e2e test) is what actually caught the two C# bugs and all 23 Go issues — unit tests and golden snapshots alone would have shipped all of them silently, since none of those check runtime behavior of the generated project's own toolchain.

## Verification

- `cargo build`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` on mcpify's own codebase.
- Full `cargo test` (277 unit tests + all golden/snapshot tests + `go_build_smoke`, the latter requiring `golangci-lint` on `PATH`) — green.
- `cargo test --test e2e_generation generates_a_csharp_project_and_passes_its_own_test_suite -- --ignored` — the real, network-touching `dotnet restore`/`dotnet format`/`dotnet test Tests -p:TreatWarningsAsErrors=true` path — green after both C# fixes.
- Manual reproduction: generated a C# project via the CLI directly against `tests/fixtures/openapi/minimal-with-auth.yaml` to confirm the `RCS1194` corruption and its fix on disk, not just via test output.
- **Follow-up (2026-07-04):** at first, only C# had actually been run through its real `--ignored` e2e test — Rust and TypeScript had only been checked via golden snapshots (rendered content, not toolchain behavior), and Go only via the fast `go_build_smoke` check (`gofmt`/`go vet`/`golangci-lint`/`go build`, not `go test -tags=integration` or the embeddings pipeline). Closed that gap by running `cargo test --test e2e_generation -- --ignored` for all five targets individually: `generates_a_rust_project_and_passes_its_own_test_suite` (clippy + full build/test, 123s), `generates_a_project_and_passes_its_own_test_suite` (Biome `lint:fix` + full build/test, TypeScript, 186s), `generates_a_go_project_and_passes_its_own_test_suite` (golangci-lint + `go test -tags=integration` + embeddings — required fetching `onnxruntime-osx-arm64-1.27.0` from GitHub Releases and setting `ONNXRUNTIME_SHARED_LIBRARY_PATH`, since this local environment isn't Linux like CI's cached copy, 18s), and `generates_a_python_project_and_passes_its_own_test_suite` (unchanged target, run for completeness, 74s). All five passed clean — no further bugs found beyond the two C# issues and 23 Go issues already fixed.
