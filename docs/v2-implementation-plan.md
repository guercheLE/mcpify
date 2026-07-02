# mcpify: v2 Rust Target Implementation Plan

> **Status: Not started.** This plan covers adding the Rust output target (`-l rust`) to the mcpify generator, per `docs/architecture.md`'s "Target Language Roadmap" (§3). It assumes `docs/v1-implementation-plan.md` is complete — everything in that plan (Stories 0–6: the shared pipeline, CLI, `GeneratorContext`, `McpServerTargetGenerator` trait, rollback, `TargetRegistry`) is reused as-is here, unmodified. This plan only covers the new, Rust-specific per-target work (the equivalent of v1's Stories 7–14), plus the small additions Stories 15–17 need for a second target.

## Context

Per architecture.md's rollout notes: *"Rust ships second so the generator can eventually dogfood its own output language."* mcpify's own generator is written in Rust — this is the one target where the generator and the generated output share a language, which is both an opportunity (real code reuse as template source material, not just pattern inspiration) and a trap (it's tempting to over-couple the generator's own internals to what the Rust target template happens to need; resist that — the generator's `src/db/schema.rs` etc. are implementation details of mcpify itself, not a library the generated project depends on).

**What's identical to every other target, and does not need to be re-planned:** the shared 4-step pipeline (OpenAPI ingest, directory guard, auth profiling, `mcp_store.db` schema+relational-row assembly) is target-agnostic Rust code that already exists and runs once regardless of `--language`. `-l rust` only needs a new `RustTargetGenerator` implementing the same `McpServerTargetGenerator` trait's 6 per-target methods (`bootstrap_project` through `run_generated_tests`) and registering itself in `targets::build_registry()`.

## Toolchain (architecture.md §3)

| Concern | Choice |
| --- | --- |
| Async runtime | `tokio` |
| MCP SDK | Official Rust MCP SDK, `async-trait` handlers — **confirm the current published crate name/API at implementation time** (check crates.io/docs.rs; the ecosystem here is younger and moves faster than this doc can track) |
| DB driver + vector ext. | `rusqlite` + `sqlite-vec` — identical crates to what mcpify's own generator already depends on |
| HTTP client (outbound) | `reqwest` — same crate mcpify's own generator already depends on |
| HTTP/transport server | `axum` |
| Schema validation | `jsonschema` crate |
| Structured logging | `tracing` + `tracing-subscriber` (JSON) |
| Tracing/metrics | `tracing-opentelemetry` |
| Generated test tooling | `cargo test` |
| CLI invocation of output | `./<binary> --server` (static binary, no runtime needed) |

## Open decisions to resolve during implementation

1. **MCP SDK crate.** Search crates.io for the current official/community Rust MCP SDK before writing any template that imports it — do not assume a specific crate name or version from this document. If none is production-ready, evaluate implementing the JSON-RPC wire protocol directly against the MCP spec as a fallback (more work, but the spec itself is stable even where SDK maturity isn't).
2. **Embeddings, again — same underlying constraint as v1, different mechanics.** v1's decision was "compute embeddings in TypeScript, both at generation time and at live-query time, so the two can't drift." For a Rust *output* target, deferring to a Node/TS script would be a strange dependency for a native Rust MCP server to carry, so this target needs a Rust-native embedding path — but the *same* one-code-path principle applies: whatever computes vectors for `scripts`/`populate-embeddings`-equivalent must be the exact function the live `search` tool calls, or cosine similarity breaks silently. Candidates, roughly in order of fit:
   - **`fastembed-rs`** — purpose-built for exactly this ("compute a sentence embedding"), wraps ONNX Runtime, supports the sentence-transformers model family. Best first candidate; confirm it still supports `all-mpnet-base-v2` (or an equivalent-quality model) at implementation time.
   - **`ort`** (ONNX Runtime bindings) + `tokenizers` crate — lower-level, more control, more implementation work, same model file mcpify would otherwise need for `fastembed-rs`.
   - **`candle`** (Hugging Face's Rust ML framework) — can run the model without ONNX at all; heavier dependency, more mature for training than for this narrow inference use case as of this writing.
   Whichever is chosen, model files are downloaded/cached on first use (mirroring the UX of v1's `@xenova/transformers` caching), not bundled into the generated binary.
3. **Model/dimension parity with `mcp_store.db`'s schema.** Story 5 (already built, shared) hard-codes `FLOAT[768]` in the `semantic_endpoints` vec0 table to match `all-mpnet-base-v2`. If the chosen Rust embedding library's best-supported model has a different native dimensionality, either use a compatible variant of the same model family or (bigger change, needs a human call) make the vec0 column dimension a per-generation parameter threaded through `GeneratorContext` instead of a Story-5 constant — flag this explicitly if it comes up, don't silently change Story 5's shared schema for one target's convenience.

## Story breakdown

Mirrors v1's Stories 7–14 exactly in shape; only the toolchain and target-specific naming change. Story numbers below are target-local (start at 1), not global — this is a second target's plan, not a continuation of v1's numbering.

---

### R1 — Target scaffolding & template engine
**Goal:** `src/targets/rust/mod.rs`: `RustTargetGenerator`, `name() -> "rust"`, 6 methods stubbed. Own `RsTemplateContext` (mirrors `TsTemplateContext`: project_name, package_name/crate_name, display_name, client type name, auth_schemes view, operations view). Own `naming.rs` (Rust identifiers are `snake_case`/`PascalCase`/`SCREAMING_SNAKE_CASE` — no camelCase convention to support, simpler than v1's four-case naming module). Own `templates/` directory + `RustEmbed` + `render.rs`/`emit.rs`, following the exact v1 Story 7 pattern (same `tera` + `rust-embed` combination, just a second, independent template set).

**Depends on:** v1 Stories 0–6 (reused, not re-implemented).

---

### R2 — `bootstrap_project`
**Goal:** `Cargo.toml.tera` (crate name, dependencies from the toolchain table above), project skeleton (`src/{auth,cli,core,data,http,services,tools,validation}/mod.rs` or a flatter Rust-idiomatic module layout — decide once R1's context model is settled whether to mirror v1's folder-per-concern shape exactly, since Rust's module system doesn't require one file per type the way TS's does), `.gitignore`, `README.md`.

**Depends on:** R1.

---

### R3 — `generate_enterprise_scaffolding`
**Goal:** The ~17 core-module equivalents, in Rust idiom: `logger.rs` (tracing-subscriber JSON layer setup), `tracing.rs` (tracing-opentelemetry wiring), `config.rs` (the same REQ-2.2 7-tier cascade — this logic already exists once, in mcpify's own generator, as a design pattern to mirror, not code to copy verbatim since the generator's config concerns and the generated project's are different problems), `circuit_breaker.rs`, `credential_storage.rs` (OS keychain — evaluate the `keyring` crate, the Rust ecosystem's rough equivalent of Node's `keytar`), `health_check.rs`, `rate_limiter.rs`, `cache.rs`, `mcp_server.rs` (wraps the chosen MCP SDK), plus `Dockerfile.tera` (Rust's multi-stage build is typically *leaner* than the Node one — a `FROM rust:X AS builder` stage producing a static-ish binary, then a minimal runtime stage like `debian:slim` or `scratch`/`distroless` copying just the binary + `mcp_store.db`), `docker-compose.yml.tera`, and the three GitHub Actions workflow templates (`cargo fmt --check`/`cargo clippy`/`cargo test` replacing the TS target's `lint`/`format:check`/`build`/`test`).

**Depends on:** R2.

---

### R4 — `generate_auth_strategies`
**Goal:** Same 5 strategies (Basic, PAT, OAuth1 RSA-SHA1, OAuth2 PKCE+refresh, stub) as v1, same shared `AuthStrategy` trait shape (an actual Rust `trait`, appropriately, rather than a TS `interface`), same auth-manager dispatch pattern (a `match` on the discovered `auth_method` instead of the TS target's object-literal lookup table). RSA-SHA1 signing in Rust: `rsa` + `sha1` crates, or a dedicated OAuth1 crate if one with adequate maintenance exists — verify at implementation time rather than assume.

**Depends on:** R3.

---

### R5 — `generate_transports_and_roles`
**Goal:** Dual-role entry points — a Rust binary's `main.rs` dispatching on a CLI flag/subcommand (via `clap`, already a dependency mcpify's own generator uses, so the pattern is directly familiar) between Terminal Client mode and Harness Server mode. `axum` router for the HTTP transport (mirrors v1's `localhost-detector`/`auth-extractor`/`metrics` modules, translated to `axum` middleware/extractors idiomatically rather than the hand-rolled `node:http` handler v1 used).

**Depends on:** R3, R4.

---

### R6 — `generate_mcp_tools`
**Goal:** `search`/`get`/`call`, a `rusqlite`+`sqlite-vec` data-access module (this one genuinely can lift the *shape* of mcpify's own `src/db/schema.rs`/`open.rs`/`populate.rs` almost directly, since both the generator and the generated project use the identical DB crate pair), a `reqwest`-based API client (generic operation dispatcher, same design as v1's — read parameter locations from the resolved input schema rather than one method per operation), `jsonschema`-crate-based validation loading the same kind of co-located JSON schema asset v1 uses (built directly via `serde_json`, not templated, for the same large-spec-scalability reason), and the embedding service resolved per the open decision above — imported by both the tool and the generated populate-embeddings binary/script, so they can't drift.

**Depends on:** R3, R4, R5, and the embeddings decision.

---

### R7 — `generate_setup_wizard_and_tests`
**Goal:** Interactive setup (candidate crate: `inquire`, which mcpify's own generator already depends on for its own auth-scheme prompt fallback — same UX library, reused as a pattern) and the generated test suite using `cargo test` (unit tests inline per module, matching this whole codebase's own convention) — conditionally emitting auth-strategy tests only for discovered schemes, exactly like v1's Story 13 requirement (an emitted test importing a non-generated module is a hard `run_generated_tests` failure, not just a logical one).

**Depends on:** R3, R4, R5, R6.

---

### R8 — `run_generated_tests` + registration
**Goal:** Shell out to `cargo build` + `cargo test` (or just `cargo test`, which also compiles — mirroring v1's "a passing test run is the single proof of both build and functional correctness" framing, since `cargo test` cannot run against code that fails to compile either). No `npm install`-equivalent step: Cargo resolves and builds dependencies as part of `cargo test` itself, so this target's `run_generated_tests` is actually *simpler* than v1's (no separate install command, no embedding-model-download step unless the embeddings library needs one at test time — flag if `fastembed-rs`'s model download becomes a similar critical-path network dependency as v1's `@xenova/transformers` one). Register `RustTargetGenerator` in `targets::build_registry()` only once this step is real and green — same "don't register a target whose tests can't actually prove anything" discipline v1 followed.

**Depends on:** R7. **Treat this as the v2 launch milestone**, exactly as Story 14 was v1's.

---

### R9 — Golden/snapshot tests + CI additions
**Goal:** Extend the existing `tests/golden_typescript.rs` pattern with a `tests/golden_rust.rs` (same file-tree-shape + curated-content-snapshot approach, new fixture combinations if needed, but the *fixtures themselves* — `tests/fixtures/openapi/*.yaml/json` — are already shared and reusable, no target-specific specs needed). Extend `.github/workflows/ci.yml`'s fast job to also install a Rust toolchain for the *generated* project's test run (already present, since mcpify itself is Rust — no new toolchain install needed in CI, unlike v3/v4/v5) and add a slow-job step analogous to v1's `e2e_generation.rs` test but targeting `RustTargetGenerator`.

**Depends on:** R8.

## Sequencing

R1 → R2 → R3 → R4 → (R5 ‖ R6 once R4 lands) → R7 → R8 → R9, mirroring v1's Story 7→14 order for the same architecturally-mandated reason (enterprise scaffolding before tool-specific code). Spike the embeddings-library decision (open decision #2) early and in parallel with R1–R4, the same way v1 flagged `sqlite-vec`-via-`rusqlite` loading as the story to de-risk first — here the equivalent highest-uncertainty item is the embedding crate choice, not the DB crate (which is already proven, being identical to mcpify's own).

## Verification

Same shape as v1's: per-story `cargo test` on mcpify's own suite, golden/snapshot tests for template-drift regression, and — the real gate — an `#[ignore]`-by-default Rust test that runs `RustTargetGenerator::execute()` against a fixture spec and asserts the *generated* project's own `cargo test` passes for real. Additionally verify: a hand-picked semantic-search query against a tiny fixture returns sane, correctly-ordered results (proves the embeddings decision's correctness the same way v1's manual model-download-and-query check did) — do this once during R6/R8 development, not as a standing automated check if it meaningfully slows CI.
