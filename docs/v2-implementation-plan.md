# mcpify: v2 Rust Target Implementation Plan

> **Status: Complete (2026-07-02).** All of R1–R9 are implemented, committed, and verified — `-l rust` is registered in `targets::build_registry()` and reachable from the CLI. Every story was checked beyond `cargo check`/`cargo test` on mcpify's own suite: each of R3–R7 was validated by actually generating output and running `cargo check`/`test`/`clippy`/`fmt` against it as a real crate (hand-stubbing only the not-yet-written later-story modules), and R8 was validated with a genuine end-to-end run of `mcpify -i ... -o ... -l rust` against a real 4-auth-scheme fixture — including the `fastembed` model download, real vector population, and the generated project's own 59-test suite passing. R9 adds golden/snapshot regression coverage (`tests/golden_rust.rs`) and a `#[ignore]`-by-default e2e acceptance test alongside the TypeScript target's. This plan covers adding the Rust output target (`-l rust`) to the mcpify generator, per `docs/architecture.md`'s "Target Language Roadmap" (§3). It assumes `docs/v1-implementation-plan.md` is complete — everything in that plan (Stories 0–6: the shared pipeline, CLI, `GeneratorContext`, `McpServerTargetGenerator` trait, rollback, `TargetRegistry`) is reused as-is here, unmodified. This plan only covers the new, Rust-specific per-target work (the equivalent of v1's Stories 7–14), plus the small additions Stories 15–17 need for a second target. The MCP SDK crate and embeddings crate (see "Open decisions" below) are confirmed and locked in.

## Context

Per architecture.md's rollout notes: *"Rust ships second so the generator can eventually dogfood its own output language."* mcpify's own generator is written in Rust — this is the one target where the generator and the generated output share a language, which is both an opportunity (real code reuse as template source material, not just pattern inspiration) and a trap (it's tempting to over-couple the generator's own internals to what the Rust target template happens to need; resist that — the generator's `src/db/schema.rs` etc. are implementation details of mcpify itself, not a library the generated project depends on).

**What's identical to every other target, and does not need to be re-planned:** the shared 4-step pipeline (OpenAPI ingest, directory guard, auth profiling, `mcp_store.db` schema+relational-row assembly) is target-agnostic Rust code that already exists and runs once regardless of `--language`. `-l rust` only needs a new `RustTargetGenerator` implementing the same `McpServerTargetGenerator` trait's 6 per-target methods (`bootstrap_project` through `run_generated_tests`) and registering itself in `targets::build_registry()`.

## Toolchain (architecture.md §3)

| Concern | Choice |
| --- | --- |
| Async runtime | `tokio` |
| MCP SDK | **Resolved (2026-07-02):** [`rmcp`](https://docs.rs/rmcp) v2.0.0, published by `modelcontextprotocol/rust-sdk` (the official org). 14.3M downloads, actively maintained (last release 2026-06-27), `edition = "2024"` matching mcpify's own. Handler pattern is **macro-based** (`#[tool_router]`/`#[tool_handler]`/`#[prompt_handler]`), not raw `async-trait` as originally guessed here — update R1/R6 template design accordingly. Transports are feature-gated: `transport-io` for stdio, `transport-streamable-http-server` for HTTP (built on `tower-service`, composes into an axum router since `axum::Router` is itself a `tower::Service` — axum choice below still holds). Also ships an `auth` feature wrapping the `oauth2` crate, worth reusing for R4's OAuth2 PKCE strategy instead of hand-rolling it. |
| DB driver + vector ext. | `rusqlite` + `sqlite-vec` — identical crates to what mcpify's own generator already depends on |
| HTTP client (outbound) | `reqwest` — same crate mcpify's own generator already depends on |
| HTTP/transport server | `axum` (rmcp's streamable-http transport is `tower`-based, nests into an axum `Router`, not a native axum integration) |
| Schema validation | `jsonschema` crate |
| Structured logging | `tracing` + `tracing-subscriber` (JSON) |
| Tracing/metrics | `tracing-opentelemetry` |
| Generated test tooling | `cargo test` |
| CLI invocation of output | `./<binary> --server` (static binary, no runtime needed) |

## Open decisions to resolve during implementation

1. **MCP SDK crate — RESOLVED (2026-07-02).** [`rmcp`](https://docs.rs/rmcp) v2.0.0 (`github.com/modelcontextprotocol/rust-sdk`), confirmed via crates.io/docs.rs/crate-source inspection: official org, 14.3M downloads, actively maintained, `edition = "2024"`. See the toolchain table above for the handler-pattern and transport-feature details this changes from the original assumption (macro-based handlers, not `async-trait`; `tower`-based HTTP transport composed into axum rather than a native axum integration). No JSON-RPC-from-scratch fallback needed.
2. **Embeddings, again — same underlying constraint as v1, different mechanics — RESOLVED (2026-07-02).** v1's decision was "compute embeddings in TypeScript, both at generation time and at live-query time, so the two can't drift." For a Rust *output* target, deferring to a Node/TS script would be a strange dependency for a native Rust MCP server to carry, so this target needs a Rust-native embedding path — but the *same* one-code-path principle applies: whatever computes vectors for `scripts`/`populate-embeddings`-equivalent must be the exact function the live `search` tool calls, or cosine similarity breaks silently. **Chosen: `fastembed` (fastembed-rs) v5.17.2** — confirmed it still supports `all-mpnet-base-v2` natively (768-dim output, exact parity with Story 5's schema — see decision 3 below), 1.05M recent downloads, actively maintained (2026-06-15). Caching UX matches the desired mirror of v1's `@xenova/transformers` behavior: downloads on first use to `.fastembed_cache` (overridable via `FASTEMBED_CACHE_DIR`/`HF_HOME`), loads from cache afterwards, not bundled into the generated binary. `ort`+`tokenizers` and `candle` remain documented fallbacks if `fastembed` proves inadequate during R6 implementation, but are no longer the default path.
3. **Model/dimension parity with `mcp_store.db`'s schema — RESOLVED (2026-07-02), no schema change needed.** Story 5 (already built, shared) hard-codes `FLOAT[768]` in the `semantic_endpoints` vec0 table to match `all-mpnet-base-v2`. Since `fastembed` supports `all-mpnet-base-v2` directly at its native 768 dimensions, this is exact parity — Story 5's schema is unchanged, and threading a per-generation vec0-dimension parameter through `GeneratorContext` is not needed.

**Bonus finding, R4-relevant (not one of the three tracked decisions above, but surfaced by the same research pass):** the dedicated `oauth1-request` crate is stale (last release 2024-06) — use the doc's fallback instead, hand-rolled OAuth1 RSA-SHA1 signing via `rsa` (0.9.10) + `sha1` (0.11.0), both actively maintained. `keyring` v4.1.2 (updated 2026-06-21) is confirmed current and suitable for R3's `credential_storage.rs`.

## Story breakdown

Mirrors v1's Stories 7–14 exactly in shape; only the toolchain and target-specific naming change. Story numbers below are target-local (start at 1), not global — this is a second target's plan, not a continuation of v1's numbering.

---

### R1 — Target scaffolding & template engine ✅ Done
**Goal:** `src/targets/rust/mod.rs`: `RustTargetGenerator`, `name() -> "rust"`, 6 methods stubbed. Own `RsTemplateContext` (mirrors `TsTemplateContext`: project_name, package_name/crate_name, display_name, client type name, auth_schemes view, operations view). Own `naming.rs` (Rust identifiers are `snake_case`/`PascalCase`/`SCREAMING_SNAKE_CASE` — no camelCase convention to support, simpler than v1's four-case naming module). Own `templates/` directory + `RustEmbed` + `render.rs`/`emit.rs`, following the exact v1 Story 7 pattern (same `tera` + `rust-embed` combination, just a second, independent template set).

**Depends on:** v1 Stories 0–6 (reused, not re-implemented).

---

### R2 — `bootstrap_project` ✅ Done
**Goal:** `Cargo.toml.tera` (crate name, dependencies from the toolchain table above), project skeleton (`src/{auth,cli,core,data,http,services,tools,validation}/mod.rs` or a flatter Rust-idiomatic module layout — decide once R1's context model is settled whether to mirror v1's folder-per-concern shape exactly, since Rust's module system doesn't require one file per type the way TS's does), `.gitignore`, `README.md`.

**Depends on:** R1.

---

### R3 — `generate_enterprise_scaffolding` ✅ Done
**Goal:** The ~17 core-module equivalents, in Rust idiom: `logger.rs` (tracing-subscriber JSON layer setup), `tracing.rs` (tracing-opentelemetry wiring), `config.rs` (the same REQ-2.2 7-tier cascade — this logic already exists once, in mcpify's own generator, as a design pattern to mirror, not code to copy verbatim since the generator's config concerns and the generated project's are different problems), `circuit_breaker.rs`, `credential_storage.rs` (OS keychain — evaluate the `keyring` crate, the Rust ecosystem's rough equivalent of Node's `keytar`), `health_check.rs`, `rate_limiter.rs`, `cache.rs`, `mcp_server.rs` (wraps the chosen MCP SDK), plus `Dockerfile.tera` (Rust's multi-stage build is typically *leaner* than the Node one — a `FROM rust:X AS builder` stage producing a static-ish binary, then a minimal runtime stage like `debian:slim` or `scratch`/`distroless` copying just the binary + `mcp_store.db`), `docker-compose.yml.tera`, and the three GitHub Actions workflow templates (`cargo fmt --check`/`cargo clippy`/`cargo test` replacing the TS target's `lint`/`format:check`/`build`/`test`).

**Depends on:** R2.

---

### R4 — `generate_auth_strategies` ✅ Done
**Goal:** Same 5 strategies (Basic, PAT, OAuth1 RSA-SHA1, OAuth2 PKCE+refresh, stub) as v1, same shared `AuthStrategy` trait shape (an actual Rust `trait`, appropriately, rather than a TS `interface`), same auth-manager dispatch pattern (a `match` on the discovered `auth_method` instead of the TS target's object-literal lookup table). RSA-SHA1 signing in Rust: `rsa` + `sha1` crates, or a dedicated OAuth1 crate if one with adequate maintenance exists — verify at implementation time rather than assume.

**Depends on:** R3.

---

### R5 — `generate_transports_and_roles` ✅ Done
**Goal:** Dual-role entry points — a Rust binary's `main.rs` dispatching on a CLI flag/subcommand (via `clap`, already a dependency mcpify's own generator uses, so the pattern is directly familiar) between Terminal Client mode and Harness Server mode. `axum` router for the HTTP transport (mirrors v1's `localhost-detector`/`auth-extractor`/`metrics` modules, translated to `axum` middleware/extractors idiomatically rather than the hand-rolled `node:http` handler v1 used).

**Depends on:** R3, R4.

---

### R6 — `generate_mcp_tools` ✅ Done
**Goal:** `search`/`get`/`call`, a `rusqlite`+`sqlite-vec` data-access module (this one genuinely can lift the *shape* of mcpify's own `src/db/schema.rs`/`open.rs`/`populate.rs` almost directly, since both the generator and the generated project use the identical DB crate pair), a `reqwest`-based API client (generic operation dispatcher, same design as v1's — read parameter locations from the resolved input schema rather than one method per operation), `jsonschema`-crate-based validation loading the same kind of co-located JSON schema asset v1 uses (built directly via `serde_json`, not templated, for the same large-spec-scalability reason), and the embedding service resolved per the open decision above — imported by both the tool and the generated populate-embeddings binary/script, so they can't drift.

**Depends on:** R3, R4, R5, and the embeddings decision.

---

### R7 — `generate_setup_wizard_and_tests` ✅ Done
**Goal:** Interactive setup (candidate crate: `inquire`, which mcpify's own generator already depends on for its own auth-scheme prompt fallback — same UX library, reused as a pattern) and the generated test suite using `cargo test` (unit tests inline per module, matching this whole codebase's own convention) — conditionally emitting auth-strategy tests only for discovered schemes, exactly like v1's Story 13 requirement (an emitted test importing a non-generated module is a hard `run_generated_tests` failure, not just a logical one).

**Depends on:** R3, R4, R5, R6.

---

### R8 — `run_generated_tests` + registration ✅ Done
**Goal:** Shell out to `cargo build` + `cargo test` (or just `cargo test`, which also compiles — mirroring v1's "a passing test run is the single proof of both build and functional correctness" framing, since `cargo test` cannot run against code that fails to compile either). No `npm install`-equivalent step: Cargo resolves and builds dependencies as part of `cargo test` itself, so this target's `run_generated_tests` is actually *simpler* than v1's (no separate install command, no embedding-model-download step unless the embeddings library needs one at test time — flag if `fastembed-rs`'s model download becomes a similar critical-path network dependency as v1's `@xenova/transformers` one). Register `RustTargetGenerator` in `targets::build_registry()` only once this step is real and green — same "don't register a target whose tests can't actually prove anything" discipline v1 followed.

**Depends on:** R7. **Treat this as the v2 launch milestone**, exactly as Story 14 was v1's.

---

### R9 — Golden/snapshot tests + CI additions ✅ Done
**Goal:** Extend the existing `tests/golden_typescript.rs` pattern with a `tests/golden_rust.rs` (same file-tree-shape + curated-content-snapshot approach, new fixture combinations if needed, but the *fixtures themselves* — `tests/fixtures/openapi/*.yaml/json` — are already shared and reusable, no target-specific specs needed). Extend `.github/workflows/ci.yml`'s fast job to also install a Rust toolchain for the *generated* project's test run (already present, since mcpify itself is Rust — no new toolchain install needed in CI, unlike v3/v4/v5) and add a slow-job step analogous to v1's `e2e_generation.rs` test but targeting `RustTargetGenerator`.

**Depends on:** R8.

## Sequencing

R1 → R2 → R3 → R4 → (R5 ‖ R6 once R4 lands) → R7 → R8 → R9, mirroring v1's Story 7→14 order for the same architecturally-mandated reason (enterprise scaffolding before tool-specific code). Spike the embeddings-library decision (open decision #2) early and in parallel with R1–R4, the same way v1 flagged `sqlite-vec`-via-`rusqlite` loading as the story to de-risk first — here the equivalent highest-uncertainty item is the embedding crate choice, not the DB crate (which is already proven, being identical to mcpify's own).

## Verification

Same shape as v1's: per-story `cargo test` on mcpify's own suite, golden/snapshot tests for template-drift regression, and — the real gate — an `#[ignore]`-by-default Rust test that runs `RustTargetGenerator::execute()` against a fixture spec and asserts the *generated* project's own `cargo test` passes for real. Additionally verify: a hand-picked semantic-search query against a tiny fixture returns sane, correctly-ordered results (proves the embeddings decision's correctness the same way v1's manual model-download-and-query check did) — do this once during R6/R8 development, not as a standing automated check if it meaningfully slows CI.
