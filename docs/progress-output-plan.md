# Add console progress output to mcpify

## Context

`mcpify` currently runs completely silently: `src/main.rs` prints nothing
until either success (no message at all) or a final `eprintln!("error:
{err:#}")` on failure. The user has run it once or twice and can't tell
whether it's working or hung.

The worst offender is `run_generated_tests` in each target (`src/targets/
{rust,python,csharp,go,typescript}/steps/run_tests.rs`): it shells out to
`cargo`/`npm`/`dotnet`/`go`/`uv` with `Stdio::piped()` and a timeout as long
as **900s (15 min)** (typescript's npm timeout is 300s), printing nothing
until the whole command finishes, fails, or times out.

Two things the user confirmed:
- Daily interactive use: live-stream the subprocess output (cargo/npm/
  dotnet/go/uv) as it happens, plain text, no spinner library.
- mcpify's own test suite (unit tests + the `tests/*.rs` integration/golden/
  e2e tests, several of which drive real target generation through
  `run_generated_tests` and therefore spawn these same real subprocesses)
  should default to quiet — silence is preferred over marker lines if it's
  the easier/more reliable option.

These two requirements point at one mechanism: progress output (both the
per-stage marker lines and the live subprocess streaming) must be **off by
default** and only switched on by the real `mcpify` binary's entry point —
never by library code called from a test binary. Concretely: a
process-global flag, set once at the very top of `main()`, that every test
(unit or integration) simply never sets.

## Design: a small `progress` module gating everything

New file `src/progress.rs`:

```rust
use std::sync::OnceLock;

static ENABLED: OnceLock<bool> = OnceLock::new();

/// Turns on progress output. Called once, at the very top of `main()` —
/// every other entry point (unit tests, the `tests/*.rs` integration
/// binaries, and any future library consumer) never calls this, so
/// `enabled()` defaults to `false` and mcpify stays exactly as quiet as it
/// is today unless run as the actual CLI binary.
pub fn init(enabled: bool) {
    let _ = ENABLED.set(enabled);
}

pub fn enabled() -> bool {
    ENABLED.get().copied().unwrap_or(false)
}
```

No env var, no new `GeneratorContext` field, no changes to
`run_shared_pipeline`'s signature or any of its ~20 call sites in
`tests/*.rs` — this is the key reason a global flag was chosen over
threading a `verbose: bool` parameter through: `run_shared_pipeline` and
`McpServerTargetGenerator` are called identically by `main.rs` and by every
integration test, so distinguishing them requires an ambient signal, not a
parameter.

`src/main.rs`: call `mcpify::progress::init(true);` as the very first line
of `main()`, before `run().await`.

Every progress line elsewhere is gated with `if crate::progress::enabled()
{ eprintln!(...) }`, so:
- Running the real `mcpify` binary → full stage markers + live subprocess
  output.
- `cargo test` (unit tests and every `tests/*.rs` integration/golden/e2e
  test) → `progress::init` is never called → completely silent, byte-for-
  byte the same behavior as today.

## Stages to instrument (all gated by `progress::enabled()`)

### 1. Shared pipeline — `src/pipeline/mod.rs`

In `run_shared_pipeline` / `assemble_context`, one line before each shared
step:
- before `openapi::ingest(input)` → `"==> Fetching OpenAPI spec from {input}"`
- before `dir_guard::check_output_dir` → `"==> Preparing output directory {output_dir}"`
- before `auth_profile::profile_auth` → `"==> Profiling auth schemes"`
- before `normalize_operations(doc)` → `"==> Normalizing operations"`, then after → `"==> Found {n} operations"`
- before `db::assemble_store(&ctx)` → `"==> Assembling mcp_store.db"`

### 2. Per-target step dispatch — `src/targets/mod.rs`

`McpServerTargetGenerator::execute`'s default body already lists all 7
lifecycle steps in one place, common to every target — the single spot to
add per-step lines rather than duplicating them across 5 target modules.
One line before each call, using `self.name()` and a `Step N/7` counter so
the user can see concrete progress through the run, not just a step name:

- `"==> [{name}] Step 1/7: Bootstrapping project"`
- `"==> [{name}] Step 2/7: Generating enterprise scaffolding"`
- `"==> [{name}] Step 3/7: Generating auth strategies"`
- `"==> [{name}] Step 4/7: Generating transports and roles"`
- `"==> [{name}] Step 5/7: Generating MCP tools"`
- `"==> [{name}] Step 6/7: Generating setup wizard and tests"`
- `"==> [{name}] Step 7/7: Running generated project's test suite (installs dependencies, builds, and runs tests — this can take several minutes)"`

### 3. Live subprocess streaming — each target's `run_tests.rs`

All five targets share the same shape: a private `run_{cargo,npm,dotnet,go,
uv}_command(cwd, args, label)` helper that spawns with `Stdio::piped()`,
awaits `child.wait_with_output()` under a timeout, and on failure builds an
error message from a `tail()` of the captured stdout/stderr (e.g.
`src/targets/rust/steps/run_tests.rs:57`'s `run_cargo_command` +
`src/targets/rust/steps/run_tests.rs:88`'s `tail()`). This duplication
across the 5 files is already the codebase's existing convention (each
target's `tail()` is independently copy-pasted too), so the fix follows
the same pattern rather than extracting a new shared module.

In each of the 5 files, change the helper to:
1. Print `"  -> running '{label}'..."` when `progress::enabled()`.
2. Replace the single `child.wait_with_output()` call with manually
   draining `child.stdout`/`child.stderr` concurrently (two loops reading
   into growable `Vec<u8>` buffers, e.g. via `tokio::io::AsyncReadExt::
   read`), each writing every chunk straight to the real stdout/stderr
   (`std::io::Write`) as it arrives **only when `progress::enabled()`** —
   otherwise just accumulate, identical to today's silent capture.
3. Await the child's exit status alongside both drain loops, still wrapped
   in the same overall `timeout(..., ...)` as today.
4. Reconstruct the accumulated bytes into the exact same `tail()`-based
   failure message as today — the failure-reporting format doesn't change,
   only how the bytes are gathered (streamed-and-collected vs.
   collected-only).

This makes the "silent under test" and "live under real use" behaviors
exactly byte-identical to what exists today when `progress::enabled()` is
false, since the accumulation logic doesn't change — only whether it's
*also* echoed live.

Files to change:
- `src/targets/rust/steps/run_tests.rs` (`run_cargo_command`)
- `src/targets/python/steps/run_tests.rs` (`run_uv_command`)
- `src/targets/csharp/steps/run_tests.rs` (`run_dotnet_command`)
- `src/targets/go/steps/run_tests.rs` (`run_command`)
- `src/targets/typescript/steps/run_tests.rs` (`run_npm_command`)

### 4. Final confirmation — `src/main.rs`

After `run_generate` finishes successfully (after `add_version::seed::
seed_ledger_after_generate` returns `Ok`), print (gated)
`"==> Generated project ready at {output_dir}"` so a successful run ends
with an unambiguous confirmation instead of silently returning to the
shell prompt.

## Notes

- All new output uses `eprintln!`/direct stdout/stderr writes — stderr for
  markers, and each subprocess's own stdout/stderr are echoed to the
  matching real stream — consistent with the existing `error: {err:#}`
  line in `main.rs`.
- No new crate dependency: `tokio = { features = ["full"] }` already
  includes the `io-util`/`process` pieces needed for manual pipe draining.
- Zero behavior change for `cargo test` / any integration test: since
  `progress::init` is only ever called from `main()`, every existing test
  keeps its current silent, fully-captured behavior exactly as is.

## Also fix: pre-existing Go-template `golangci-lint` failure

`tests/go_build_smoke.rs` (`generated_go_project_gofmt_vet_and_build_cleanly`)
currently fails independent of this progress-output work — confirmed via
`git stash` that it fails identically on `main` before any of the above
changes. `golangci-lint run` (v2.0.0, per `.github/workflows/ci.yml`)
reports, on the generated fixture:

```
internal/core/config.go:108:32: filepathJoin: "/etc" contains a path separator (gocritic)
internal/core/credentialstorage.go:36:1: unnamedResult (gocritic)
internal/core/credentialstorage.go:95:1: unnamedResult (gocritic)
internal/core/credentialstorage.go:233:1: unnamedResult (gocritic)
```

Both templates already carry `//nolint:gocritic` comments directly above
the flagged lines (`src/targets/go/templates/internal/core/config.go.tera:106`
and 4 occurrences in `.../credentialstorage.go.tera` at lines 35, 94, 232,
270) — added by an earlier attempt at this exact fix — but they are
demonstrably not suppressing the issue (the smoke test fails with these
exact violations today). Rather than continue relying on `nolint`
placement (whose exact line-attachment semantics under golangci-lint v2
aren't reliable enough to trust here), fix the underlying code shape so
the lint issue can't fire at all, and delete the now-unnecessary `nolint`
comments:

**`config.go.tera`** — gocritic's `filepathJoin` flags a literal argument
that itself embeds a path separator. Change:
```go
applyFile(&cfg, filepath.Join("/etc", "{{ project_name }}", "config.json"))
```
to
```go
applyFile(&cfg, filepath.Join("/", "etc", "{{ project_name }}", "config.json"))
```
(a bare `"/"` leading segment is the standard idiom for an absolute path
that doesn't trip this check — same resulting path, no embedded separator
in any single literal). Remove the `//nolint:gocritic` line above it.

**Update after implementation:** gocritic's `filepathJoin` check turned out
to flag *any* argument containing a separator, including a bare `"/"` —
the fix actually shipped keeps the leading slash out of `filepath.Join`
entirely: `"/" + filepath.Join("etc", "{{ project_name }}", "config.json")`.

**`credentialstorage.go.tera`** — gocritic's `unnamedResult` flags the 4
`Get(key string) (string, bool, error)` methods (`KeyringStore`,
`EncryptedFileStore`, `FallbackStore`, `CachingStore`) for having no result
names. The existing comments explain unnamed was chosen because naming
them `value, found, err` would collide with the method's own local `:=`
declarations of the same names — true only where the `:=` is in the same
scope as the named returns. Fix by naming the results
`(value string, found bool, err error)` and, only where a same-scope `:=`
would now redeclare zero new variables, switching that specific line from
`:=` to `=`:
- `KeyringStore.Get`: `value, err := keyring.Get(...)` → `value, err = keyring.Get(...)`.
- `EncryptedFileStore.Get`: `store, err := s.readAll()` stays `:=` (`store` is still new); `value, ok := store[key]` → `value, found = store[key]`.
- `FallbackStore.Get`: the `:=` is inside an `if` init-clause (its own nested scope), so it already doesn't collide — just rename result names in the signature and the shadowing `if value, ok, err := ...` to `if value, found, err := ...` for consistency.
- `CachingStore.Get`: the `if cached, ok := s.cache[key]; ok` init-clause is its own scope (untouched); the top-level `value, ok, err := s.inner.Get(key)` → `value, found, err = s.inner.Get(key)`, and the later `if ok` → `if found`.

Remove all 4 `//nolint:gocritic` comments once the signatures are named —
no suppression needed since the trigger condition is gone.

Both fixes are purely structural/cosmetic — same resulting behavior — so
no other template or generated-code consumer should be affected.

## Verification

- `cargo build` and `cargo test` (existing + affected `run_tests.rs` unit
  tests) still pass, with no new console output (progress stays off).
- `cargo test --test go_build_smoke` passes (currently fails on `main`).
- Manually run mcpify end-to-end against a small fixture and confirm
  staged + live output, e.g.:
  `cargo run -- -i tests/fixtures/openapi/minimal-with-auth.yaml -o /tmp/mcpify-progress-check -l typescript --force`
  Confirm: stage markers appear incrementally (fetch → auth profiling →
  normalize → db assemble → each of the 7 target steps, each showing its
  `Step N/7` counter), the `"-> running 'npm install'..."`-style marker
  appears before each subprocess, and the actual `npm`/`cargo`/etc. output
  streams live to the terminal during the `run_generated_tests` step
  instead of going silent for minutes, ending with the final
  `"==> Generated project ready at ..."` line.
- Also run `cargo run -- -i ... -o /tmp/mcpify-progress-check-go -l go --force`
  (or re-run `cargo test --test go_build_smoke`) to confirm the Go template
  fix actually clears `golangci-lint` end to end.

## Outcome

Implemented and verified in full: `cargo build`/`cargo test`/`cargo fmt --check`/
`cargo clippy` all clean, `go_build_smoke` passes, golden snapshots updated
for the two curated `config.go` fixtures, and a real `cargo run` against the
`minimal-with-auth.yaml` fixture confirmed stage markers, `Step N/7`
counters, subprocess markers, and live-streamed `npm` output all appear
incrementally, ending with the `"==> Generated project ready at ..."` line.
