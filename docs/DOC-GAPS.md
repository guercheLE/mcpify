# Documentation Gaps

A retrospective log of requirements, edge cases, and design constraints that `docs/product-brief.md`, `docs/prd.md`, `docs/architecture.md`, and the versioned `docs/vN-implementation-plan.md` files did not anticipate up front — and that only surfaced later as `fix:` commits or retrofitted `feat:` commits. Grouped by release, in the same `## [X.Y.Z] - YYYY-MM-DD` sections `CHANGELOG.md` uses, so the two files can be updated in tandem. A gap whose fix spans multiple releases gets its full write-up under the release where it was substantially resolved, with a one-line pointer under every other release it also touched.

Each full entry has two parts:

- **Doc gap** — what the planning doc(s) should have specified but didn't (and where it should have lived).
- **Resulting work** — what fix/feature had to be built as a consequence, and when.

## Lessons for future docs

Distilled, reusable guidance for writing `prd.md`/`architecture.md` on the *next* project, based on the recurring patterns below:

1. **Specify transport/mode-aware behavior explicitly for anything touching shared OS resources (stdout/stderr, ports, sockets).** Don't describe logging, output, or I/O as if there's one deployment mode. If a component behaves differently under `stdio` vs `http` transport (or CLI vs server mode), say so in the requirement itself — "logs go to stderr" is only correct for half of this project's transports. A requirement written as "logs go to X" invites an unconditional implementation; a requirement written as "logs go to X when transport is stdio, Y otherwise" does not.

2. **State a blanket "no CWD-relative filesystem paths" constraint once in `architecture.md`, not per-target.** This project's five language targets independently discovered (across at least 6 separate fix commits, v0.5.4 through v0.10.1, plus the still-open embedding-cache/config-cascade work) that anything the generated binary writes or reads — a store DB, a project name, a model cache — must resolve from an explicit absolute base (an override env var, `$HOME`, or the binary's own directory), never from `getcwd()`. Write this as a cross-cutting non-functional requirement exactly once (e.g. under `architecture.md` §2 or a new "Cross-Cutting Constraints" section) and reference it from every target's implementation plan, rather than trusting each target to independently arrive at the same fix.

3. **Treat "install a required native/runtime dependency" as a shippability requirement, not an implementation detail.** Two targets (C# for `sqlite-vec`, Go for the ONNX Runtime shared library) initially assumed a native dependency would already be present on the host, and both had to retrofit an auto-download-on-first-use path. If the product brief's "enterprise-first, zero-placeholder" bar (product-brief.md, PRD §2.5) is meant to include "the generated binary runs without the operator manually installing anything," say that explicitly and check it per target, rather than discovering it as a support burden after each target ships.

4. **When a design doc says a document/blob will be "self-contained," attach a cost budget, not just a correctness claim.** `v1-implementation-plan.md`'s decision to embed full standalone schema copies per operation ("simpler and more robust than cross-schema `$ref` resolution, at the cost of some file size") was correct on correctness but silent on the cost, which grew to ~115 MB of duplication before `v9-implementation-plan.md` caught it — and even that fix needed two more follow-up iterations (v0.11.5, v0.11.7) to get fully right. Any "self-contained by design" decision should come with an explicit size/scale sanity check in the same doc, not just a qualitative trade-off note.

5. **Concurrency and platform-specific I/O semantics belong in the architecture doc for any embedded local datastore, not just in Rust doc-comments.** The `mcp_store.db` design (architecture.md §2 "Data Layer") never mentions what happens when two calls trigger extraction/rename concurrently, or that Windows' file-handle release timing differs from POSIX. This produced a long tail of narrowly-scoped fixes (atomic extraction, mutex-poisoning recovery, cross-process rename retries, Windows-specific test skips) spread across nine releases. A single paragraph up front — "the store file may be extracted/renamed concurrently; treat this as a locking problem with platform-specific semantics" — would have let each target's implementer design for it instead of patching around it target-by-target.

6. **Specify that anything "operator-observable" must expose its own internal state, not just its result.** The config cascade (PRD §2.2, "REQ-2.2's cascade" in code comments) defines seven precedence tiers but never states that the `config` command must let an operator see *which* tier produced a given value. The same blind spot applies to git hygiene: the PRD/architecture never asked "how does a contributor find out their code fails a gate *before* pushing it," even though every target's own CI already encodes that gate. When a design doc defines a pipeline with multiple stages/sources, add an explicit requirement that the pipeline's own tooling can report per-stage state — not just the final merged output.

7. **A retrofitted requirement should get a retrofitted line in the PRD, immediately, not just a code fix.** The output-schema-validation behavior (fatal on mismatch → non-fatal warning, PRD §"call Pipeline") is a case that *was* eventually written back into `architecture.md` after the v0.8.1 fix — that's the right instinct, and it's the exception here rather than the rule. Most of the gaps below never got a corresponding doc update. Treat a `fix:` commit that changes documented behavior as incomplete until the doc that described the old behavior is also updated.

8. **When a generated project patches around a bug that clearly lives in the generator itself (a template default, a release-workflow setting, a dependency pin), propagate the fix back upstream in the same change, not as a follow-up someone might get to.** The `[profile.dist]` ThinLTO/macOS build failure (see `sqlserver-mcp-rs`, v0.1.3 below) was fixed once, locally, in a single downstream repo — and then silently left unfixed in the shared template, so every other generated project (and mcpify's own release build) stayed exposed to the identical failure. A generated project's own fix commit is the moment the gap is best understood; that's also the cheapest moment to fix it once, upstream, instead of leaving it to be independently rediscovered per repo (or, as here, never rediscovered until an unrelated retrospective audit went looking for exactly this pattern).

---

## [Unreleased]

### Doc gap: config command observability

`docs/prd.md` §2.2 defines the configuration cascade's precedence order (seven tiers: CLI flags → env vars → local file → home file → system file → install-dir file → defaults) and code comments elsewhere refer to it as "REQ-2.2's cascade," but neither `prd.md` nor `docs/architecture.md` ever specifies that the generated `config` CLI subcommand must be *observable* — i.e., that it should show which cascade layer produced each resolved value, or which config files actually exist on disk. As implemented, every target's `config` command just dumped the final merged/sanitized config as flat JSON (or hand-written print statements for Go/C#), with no per-layer provenance.

### Resulting work

Addressed in this same unreleased window: the `config` command across all 5 targets now shows every cascade tier by source (CLI flags, environment variables actually set, each config file's path/existence/sanitized contents, built-in defaults, a `.env`-never-auto-loaded note, and credential-storage presence) alongside the final resolved value.

### Doc gap: filesystem paths must not depend on launch-time CWD (cross-cutting)

No document — not `docs/architecture.md`, not `docs/prd.md`, not `docs/v2-implementation-plan.md` through `v5-implementation-plan.md` — ever states a blanket constraint that anything a generated project writes or reads from disk (store DB paths, embedding-model caches, project-name inference) must resolve from an explicit absolute base rather than the process's current working directory. `v2-implementation-plan.md`'s embeddings decision even documents the Rust target's fastembed cache defaulting to a bare relative `.fastembed_cache` path (overridable via `FASTEMBED_CACHE_DIR`/`HF_HOME`) without flagging that as a CWD-relative risk when no override is set. Surfaced repeatedly, target by target, rather than being fixed once — earlier touchpoints: v0.5.4, v0.5.5, v0.5.6 (below), v0.10.1 (below).

### Resulting work

The Rust `fastembed` cache and Python's hardcoded relative `.sentence_transformers_cache` literal were given an explicit, absolute, 3-tier precedence (override env var → `$HOME`-based path → binary-adjacent app folder), and Go/C#'s `$HOME`-only resolution was given the same explicit-override tier and absolute final fallback it was missing (Go's `os.UserHomeDir()` failure had been a hard error with no fallback at all). TypeScript's `@xenova/transformers` default was already absolute (module-install-dir-relative), so needed no fix.

### Doc gap: stdio vs HTTP logging stream (cross-cutting)

`docs/prd.md` §2.3.1 ("Logging") specifies structured JSON logging with secret redaction, but never states that the log *destination* must depend on transport mode. `docs/architecture.md` describes the `stdio`/`http` transport choice (§2, "Dual-Role Execution") without connecting it to logging at all. The result: logging was designed and implemented as if it were transport-agnostic. Earlier touchpoint: v0.5.10 (below).

### Resulting work

Made log destination conditional on `{tool_prefix}_TRANSPORT` across all 5 targets. Python/Go/Rust/C# needed only a conditional at logger-construction time; TypeScript needed a lazy-Proxy logger since its transport mode isn't known until after the `pino` logger's module-level construction.

### Doc gap: no local pre-commit gate despite every CI pipeline enforcing one

`docs/prd.md` REQ-2.3.7 and every target's own `v1`-`v7` implementation plan describe a CI workflow whose first steps are fast format/lint gates — `cargo fmt --check` + `cargo clippy` (Rust targets, including mcpify itself), `gofmt` + `golangci-lint` (Go), `ruff` + `black` (Python), `biome lint` + `biome format` (TypeScript), `dotnet format --verify-no-changes` (C#) — but no doc ever asks "how does a contributor catch this before pushing, not just in CI." Neither mcpify itself nor any of the 5 generated-project templates shipped a local git pre-commit hook mirroring those same gates.

### Resulting work

Added `.githooks/pre-commit` (mirroring each target's own CI gates) to mcpify itself and to all 5 generated-project templates, activated via `git config core.hooksPath .githooks`.

### Doc gap: release-build LTO setting untested against the actual CI toolchain

`docs/prd.md` REQ-2.3.7 requires a `cargo-dist`-based release pipeline, and `Cargo.toml.tera`'s `[profile.dist]` set `lto = "thin"` from the start — a reasonable-looking optimization choice that was never actually exercised end-to-end on every platform the release matrix targets. GitHub's macOS runner ships an Xcode/libLTO too old to parse ThinLTO bitcode from the pinned rustc's LLVM version, which fails the `aarch64-apple-darwin` dist build at link time ("could not parse bitcode object file ... Unknown attribute kind"). This didn't surface in mcpify's own CI/release history — it was discovered downstream, in `sqlserver-mcp-rs` (`v0.1.3`, 2026-07-17), which patched it locally by disabling LTO in `[profile.dist]`, but the fix was never propagated back upstream to the shared template, leaving every other mcpify-generated Rust project (and mcpify's own release build) still exposed to the same failure mode.

### Resulting work

Set `lto = false` in `[profile.dist]` (with a comment explaining why, so a future contributor doesn't just re-enable it as an obvious-looking optimization), matching the fix `sqlserver-mcp-rs` already applied locally.

### Doc gap: unbounded dependency version ranges on a third-party SDK the generated project's own code imports from directly

`pyproject.toml.tera` pinned `"mcp>=1.12"` with no upper bound. Neither `docs/prd.md` nor `docs/architecture.md` states a policy for dependencies whose public API the generated code imports from directly (as opposed to dependencies used only behind a stable, narrow interface) — the implicit assumption was that any version satisfying a lower bound would keep working. The official `mcp` SDK's `2.0.0` release renamed and moved `FastMCP` to `mcp.server.mcpserver.MCPServer`, breaking the `from mcp.server.fastmcp import Context, FastMCP` import both `core/mcp_server.py.tera` and `http/server.py.tera` rely on — every fresh Python-target generation resolved to `mcp==2.0.0` and failed at import time (`ModuleNotFoundError: No module named 'mcp.server.fastmcp'`), including in the generated project's own e2e test collection.

### Resulting work

Pinned `"mcp>=1.12,<2.0"` rather than migrating to the 2.0 API surface, since that surface hasn't been audited for other breaking changes beyond the `FastMCP`→`MCPServer` rename. A fresh generation now resolves to `mcp==1.29.0` and the e2e test collects and passes again.

## [0.11.14] - 2026-07-26

### Doc gap: multi-arch container images not specified

`docs/prd.md` REQ-2.3.7 ("Packaging & delivery") requires a multi-stage Dockerfile and a CI/CD pipeline with automated publishing, matching the pattern already proven by the two reference servers — but never states an architecture-coverage requirement (amd64 **and** arm64) for the images that pipeline publishes, and never flags that QEMU-emulated cross-architecture Docker builds are unreliable in CI. Both surfaced only after single-arch images were already being published from every generated project. Earlier touchpoints: v0.11.12, v0.11.13 (below).

### Resulting work

`fix: build multi-arch GHCR images on native runners instead of QEMU emulation` — QEMU emulation proved too slow/flaky, requiring a second design iteration after v0.11.12's first pass.

### Doc gap: self-contained schema/`$ref` handling took three iterations → see full write-up under [0.5.12] below

### Resulting work

`fix(openapi): fully inline $ref instead of localizing into $defs` — the third and final correction, replacing `$defs` localization with full inlining.

## [0.11.13] - 2026-07-26

### Doc gap: multi-arch container images not specified → see full write-up under [0.11.14] above

### Resulting work

Follow-up snapshot-test fix to keep the generator's own golden tests in sync with the v0.11.12 template change.

## [0.11.12] - 2026-07-26

### Doc gap: multi-arch container images not specified → see full write-up under [0.11.14] above

### Resulting work

`fix: publish multi-arch (amd64+arm64) GHCR images from every generated project` — first pass, built via QEMU emulation.

## [0.11.11] - 2026-07-26

### Doc gap: Windows-specific file-locking/timing semantics unaddressed for the embedded store → see full write-up below

### Resulting work

`fix: skip the disk-lock test on GitHub Actions' Windows runners, not just retry longer` — after five prior attempts to fix the race itself, the eventual resolution was to stop asserting the behavior on that CI platform at all. This is the release where the saga (v0.8.2 onward) was finally closed out.

### Doc gap: Windows-specific file-locking/timing semantics unaddressed for the embedded store

`docs/architecture.md` §2 ("Data Layer") describes `mcp_store.db` extraction/embedding without ever mentioning concurrency or platform-specific file-handle semantics. Nothing in `docs/prd.md` or `docs/architecture.md` states that store extraction/rename must be safe under concurrent calls, or that Windows releases file handles on a different timeline than POSIX systems (making naive rename/delete-then-recreate patterns flaky specifically on Windows CI runners). At least nine separate, narrowly-scoped fixes were needed before this stabilized, spanning v0.8.2 through this release — see the pointers under each touched release below.

### Resulting work

Nine fixes total: `fix(rust): extract mcp_store.db atomically to avoid concurrent-call races` (v0.8.2) → `fix(rust): eliminate Windows-only race in resolve_store_path` (v0.10.5) → `fix(rust): retry the store rename on a cross-process race` (v0.11.1) → `fix(rust): recover from mutex poisoning on the shared store connection` (v0.11.2) → `fix: retry remove_file in the store lock-release test for Windows handle-release timing` (v0.11.8) → HOME test-lock race fix, wider Windows retry (v0.11.9) → widened Windows store-lock retry to 60s (v0.11.10) → the CI-skip fix above (v0.11.11).

## [0.11.10] - 2026-07-26

### Doc gap: Windows-specific file-locking/timing semantics unaddressed for the embedded store → see full write-up under [0.11.11] above

### Resulting work

Widened Windows store-lock retry to 60s.

## [0.11.9] - 2026-07-26

### Doc gap: Windows-specific file-locking/timing semantics unaddressed for the embedded store → see full write-up under [0.11.11] above

### Resulting work

HOME test-lock race fix, wider Windows retry.

## [0.11.8] - 2026-07-26

### Doc gap: Windows-specific file-locking/timing semantics unaddressed for the embedded store → see full write-up under [0.11.11] above

### Resulting work

`fix: retry remove_file in the store lock-release test for Windows handle-release timing`.

## [0.11.7] - 2026-07-26

### Doc gap: self-contained schema/`$ref` handling took three iterations

`v1-implementation-plan.md` made a deliberate, documented decision to embed a full standalone copy of the component-schema library into every operation's `inputSchema`/`outputSchema` — reasoned as "simpler and more robust than cross-schema `$ref` resolution, at the cost of some file size" — but that cost was never bounded or checked against real specs. `docs/prd.md` REQ-2.4.1 and `docs/architecture.md` §2 ("Data Layer") both praise `mcp_store.db` as "self-contained" without ever specifying what self-contained schema output should look like at scale (reachable-only vs. whole-library, inlined `$ref` vs. localized `$defs`). This is the release where the saga (v0.5.12 onward) was finally closed out — see [0.11.14] above for the intermediate v0.11.5 touchpoint.

### Resulting work

`fix(openapi): fully inline $ref instead of localizing into $defs` — the third and final correction, replacing `$defs` localization with full inlining. Started at v0.5.12 (`embed only reachable schema definitions`, after 404 operations were found each embedding a byte-identical ~143 KB copy of the entire component-schema library — ≈115 MB of pure duplication), continued at v0.11.5 (`embed $defs into the literal schemas the get tool returns`, since "reachable-only" still left external `$ref` pointers unresolved in some cases).

## [0.11.5] - 2026-07-21

### Doc gap: self-contained schema/`$ref` handling took three iterations → see full write-up under [0.11.7] above

### Resulting work

`fix(openapi): embed $defs into the literal schemas the get tool returns` — the "reachable-only" fix from v0.5.12 still left external `$ref` pointers unresolved in some cases.

## [0.11.2] - 2026-07-21

### Doc gap: Windows-specific file-locking/timing semantics unaddressed for the embedded store → see full write-up under [0.11.11] above

### Resulting work

`fix(rust): recover from mutex poisoning on the shared store connection`.

## [0.11.1] - 2026-07-19

### Doc gap: Windows-specific file-locking/timing semantics unaddressed for the embedded store → see full write-up under [0.11.11] above

### Resulting work

`fix(rust): retry the store rename on a cross-process race`.

## [0.10.5] - 2026-07-19

### Doc gap: Windows-specific file-locking/timing semantics unaddressed for the embedded store → see full write-up under [0.11.11] above

### Resulting work

`fix(rust): eliminate Windows-only race in resolve_store_path`.

## [0.10.1] - 2026-07-18/19

### Doc gap: filesystem paths must not depend on launch-time CWD → see full write-up under Unreleased above

### Resulting work

`fix(generator): resolve project name from the real working directory when output is "."`.

## [0.9.0] - 2026-07-17

### Doc gap: OAuth2 setup wizard incompleteness

`docs/prd.md` REQ-1.6 ("Guided Setup Wizard") describes the wizard only in terms of *where* collected values get persisted (`.env`, `config.json`, or a printed CLI invocation) and never addresses what collecting OAuth2 credentials specifically requires — a PKCE code challenge/verifier, scopes derived from the spec, and a complete authorization-code exchange. REQ-1.2 (auth strategy generation) is similarly silent on OAuth2-specific mechanics beyond "emit one auth strategy module per scheme."

### Resulting work

`fix: complete the OAuth2 code exchange and send Content-Length on empty bodies, across all 5 targets` (the exchange was previously incomplete, and some HTTP servers reject bodyless requests missing that header) landed alongside `feat: PKCE OAuth2 setup wizard with spec-derived scopes, across all 5 targets` — a retrofit of both a bug fix and a missing wizard capability in the same release, across all 5 targets simultaneously.

### Doc gap: native/runtime dependency self-containment inconsistent across targets → see full write-up under [0.1.0] below

### Resulting work

`fix(go): auto-download the ONNX Runtime shared library instead of requiring it pre-installed` — the same category of gap recurred for a different target and a different native dependency five weeks after v0.1.0.

## [0.8.2] - 2026-07-16

### Doc gap: Windows-specific file-locking/timing semantics unaddressed for the embedded store → see full write-up under [0.11.11] above

### Resulting work

`fix(rust): extract mcp_store.db atomically to avoid concurrent-call races` — the first fix in what became a nine-release saga.

## [0.8.1] - 2026-07-16

### Doc gap: output-schema mismatch handling not specified up front

Neither `docs/prd.md` nor the original `docs/architecture.md` "call Pipeline" description distinguished input-validation strictness from output-validation strictness — the implicit assumption was that a schema mismatch of either kind should be treated as fatal. Real-world OpenAPI specs are frequently wrong about response shape, so failing every mismatch denied callers real data over a documentation bug rather than an actual failure.

### Resulting work

`fix: don't reject a call on output-schema mismatch, surface validation errors across all 5 targets` — output-schema mismatches now log a warning but still return live data; input-guard validation (under the caller's control) stays fatal. Notably, `docs/architecture.md` §"The `call` Pipeline" *was* updated after this fix to describe the new behavior explicitly — one of the few gaps in this list where the doc was brought back in sync with the code.

## [0.5.12] - 2026-07-08

### Doc gap: self-contained schema/`$ref` handling took three iterations → see full write-up under [0.11.7] above

### Resulting work

`fix(openapi): embed only reachable schema definitions` (originally bumped as the untagged `0.5.11`) — the fix documented in `v9-implementation-plan.md`, after real-world inspection found 404 operations each embedding a byte-identical ~143 KB copy of the entire component-schema library (≈115 MB of pure duplication across one generated project). The first of three iterations; see [0.11.7] above for how it was finally closed out.

## [0.5.10] - 2026-07-06

### Doc gap: stdio vs HTTP logging stream → see full write-up under Unreleased above

### Resulting work

`fix(rust,csharp,typescript): write logs to stderr instead of stdout` — corrected the original placeholder-style logging (which likely wrote to stdout, corrupting stdio-mode JSON-RPC framing) by hardcoding stderr unconditionally across three targets. This was directionally correct for `stdio` mode but wrong for `http` mode, where stdout is the conventional log destination and nothing reserves it — not fully closed out until this session (see Unreleased above).

## [0.5.6] - 2026-07-06

### Doc gap: filesystem paths must not depend on launch-time CWD → see full write-up under Unreleased above

### Resulting work

Further store-path hardening (`VERSION_STORE_FILES`, always-refresh extraction).

## [0.5.5] - 2026-07-06

### Doc gap: filesystem paths must not depend on launch-time CWD → see full write-up under Unreleased above

### Resulting work

Two follow-up fixes moving from a path-fallback chain to embedded-only store resolution.

## [0.5.4] - 2026-07-06

### Doc gap: filesystem paths must not depend on launch-time CWD

No document — not `docs/architecture.md`, not `docs/prd.md`, not `docs/v2-implementation-plan.md` through `v5-implementation-plan.md` — ever states a blanket constraint that anything a generated project writes or reads from disk (store DB paths, embedding-model caches, project-name inference) must resolve from an explicit absolute base rather than the process's current working directory. This is the first of a long series of touchpoints not fully closed out until this session — see Unreleased above.

### Resulting work

`fix(rust): generate store db path resolution with exe-dir fallback` — the first fix in the saga.

## [0.3.1] - 2026-07-05

### Doc gap: HTTP-mode credential forwarding not in the original auth model

`docs/prd.md` §1.2/§1.3 describes a single active auth strategy selected via `auth_method` config, and §1.4 describes `stdio`/`http` transports, but never connects the two: in HTTP transport mode, a multi-tenant deployment plausibly needs to forward the *caller's own* request credentials rather than always falling back to the process's local config. The original implementation read local config unconditionally regardless of transport.

### Resulting work

`feat(auth-profile): capture header/query/cookie location metadata for auth schemes` plus five per-target `feat(<target>): forward HTTP request credentials instead of local config over HTTP transport` commits (Rust, Python, TypeScript, Go, C#) — retrofitted in the same release, across all 5 targets simultaneously, once the gap was identified.

## [0.1.0] - 2026-07-04

### Doc gap: native/runtime dependency self-containment inconsistent across targets

`docs/product-brief.md`'s "enterprise-grade from the first generated file" pillar and `docs/prd.md` §2.5 ("Quality Bar," zero-placeholder generation) never state that a generated project must run without the operator manually installing a native runtime dependency. Each target that needed one (a native SQLite extension, an ONNX Runtime shared library) had to independently discover and fix this gap. See [0.9.0] above for the second occurrence.

### Resulting work

`fix(csharp-target): auto-download the sqlite-vec native extension on first use` — landed within the initial release itself, meaning the gap was caught before ship for C#.
