> **Status: Completed.** This is the implementation plan used to build the v1 TypeScript target of the mcpify Rust generator (Stories 0–17). All 18 stories described below have been implemented, committed, and verified end to end — including a real `npm install && npm test` run against a generated project. It's kept here as a historical record of the design decisions made along the way, and as the reference pattern the v2–v5 target plans (`v2-implementation-plan.md` through `v5-implementation-plan.md`) build on.

# mcpify: Rust Generator Implementation Plan

## Context

`mcpify` (repo: `/Users/lucianoguerche/Documents/GitHub/mcpify`) is currently docs-only — `docs/{product-brief,prd,architecture}.md` and a `README.md`, no `Cargo.toml`, no source. This plan covers building the **Rust generator itself**: the CLI tool that reads an OpenAPI spec and emits a complete TypeScript/Node.js MCP server project. It does not cover hand-writing the TS output directly — it covers the Rust code that programmatically synthesizes that output from templates.

The design is fully specified in `docs/architecture.md` (the `McpServerTargetGenerator` trait, `GeneratorContext`, the shared 4-step ingestion pipeline, the per-target 7-step lifecycle) and `docs/prd.md` (every `REQ-*` requirement). Two hand-built sibling repos — `/Users/lucianoguerche/Documents/GitHub/bitbucket-dc-mcp` and `/Users/lucianoguerche/Documents/GitHub/jira-dc-mcp` — are structurally identical and are the ground-truth source for what the TypeScript templates must reproduce (parameterized): 5 auth strategies, ~17 core enterprise modules, the search/get/call tool pipeline, dual client/server roles, setup wizard, and full test/Docker/CI scaffolding. This was confirmed by direct inspection, not assumption.

**Why this shape:** the reference servers proved the target architecture works in production but required the same enterprise scaffolding to be hand-built twice. mcpify's job is to make that a one-time investment (writing templates once) instead of a recurring one (hand-building per API).

## Decisions made during planning

Three consequential open questions were resolved with the user before finalizing:

1. **Tool naming: `search` / `get` / `call`** (not `search_ids`/`get_id`/`call_id`) — per the PRD's stated design intent.
2. **Embeddings are computed by a generated TypeScript script, not natively in Rust.** The constraint that decided this: the generated project's `search` tool must embed the user's *live* query at runtime using the exact same model + dimensionality used to index operations at generation time, or cosine similarity silently returns garbage. Rather than keeping two independent embedding implementations (Rust ONNX for indexing, `@xenova/transformers` for live query) in permanent lockstep, mcpify emits **one** embedding code path in TypeScript (`Xenova/all-mpnet-base-v2`, the same model both reference servers already use) and reuses it for both jobs: a generated `scripts/populate-embeddings.ts` runs it once per operation to populate `semantic_endpoints` at project-setup time, and the generated `search` tool imports the same embedding service to embed live queries. Rust's role in Story 5 shrinks to creating the `mcp_store.db` schema (including the empty `semantic_endpoints` vec0 table) and the `endpoints` rows — it never computes a vector itself. The tradeoff: `mcp_store.db` isn't fully vector-populated the instant Rust finishes writing it — `run_generated_tests` (Story 14) must run the populate script before `npm test`, since the `search` tool's tests need real vectors to query against.
3. **Enterprise core modules (~17 files) are inline-templated** into every generated project, not shipped as a shared `@mcpify/enterprise-core` npm package — matches both reference servers and the PRD's "present from the very first file" framing. Every generated project stays fully self-contained and hackable.

Two lower-stakes decisions were defaulted (not re-asked) because the recommendation was clear-cut:
- **Templating strategy:** hybrid — `tera` (+ `rust-embed` to compile templates into the binary) for the ~80% of files that are structurally static; plain Rust string-building for the few files with real per-spec structural variance (`validation/generated-schemas` → emitted as a co-located JSON asset, not a giant `.ts` template, to avoid TS-compiler strain on large specs).
- **OpenAPI parsing:** adopt the `openapiv3` crate (handles `$ref`/`allOf` correctly) rather than hand-rolling a narrow parser — this directly affects schema-resolution fidelity for `validation/generated-schemas`.

## Story breakdown

Each story is independently shippable and testable. Dependency order matters — see **Sequencing** at the end.

---

### Story 0 — Repository & workspace bootstrap
**Goal:** Compilable, empty Rust binary crate with the toolchain wired.

- `cargo init --name mcpify`; `Cargo.toml` (edition 2021, `[[bin]] name = "mcpify"`).
- Add deps: `tokio` (full), `async-trait`, `clap` (derive), `reqwest` (json, rustls-tls), `serde`/`serde_json`/`serde_yaml`, `url`, `anyhow`, `thiserror`, `openapiv3`.
- Add generation-time deps: `rusqlite` (bundled + load-extension feature), `sqlite-vec`, `tera`, `rust-embed`.
- `src/main.rs` stub; verify `cargo build`/`cargo run`.
- `rustfmt.toml`, clippy config, crate layout doc: `src/{main.rs, cli.rs, pipeline/, targets/, db/, openapi/, auth_profile/}`.

**Critical files:** `Cargo.toml`, `src/main.rs`

---

### Story 1 — Generator CLI shell & `GeneratorContext`
**Goal:** `mcpify -i <spec> -o <dir> -l typescript [--force]` parses and validates; shared trait/context types exist (empty impls).

- `src/cli.rs`: `clap::Parser` struct with `-i/--input`, `-o/--output`, `-l/--language` (default `typescript`), `--force`.
- `-l` validated against `["typescript"]`; other values → `"{lang} is not yet supported"` + exit 1 (REQ-1.1.4).
- `src/context.rs`: `GeneratorContext { openapi_input, output_dir, force, output_dir_preexisted, auth_schemes }` — matches architecture.md's struct verbatim (shared contract, don't drift).
- `src/targets/mod.rs`: `McpServerTargetGenerator` async-trait (7 methods + default `execute` with rollback), `TargetRegistry: HashMap<&'static str, Box<dyn McpServerTargetGenerator>>`.
- `src/main.rs`: parse → build context → registry lookup → `.execute().await` → map errors to clean stderr + exit code.
- Unit tests: flag parsing edge cases (missing `-i`/`-o`, unknown `-l`, `--force` parsing).

**Critical files:** `src/cli.rs`, `src/context.rs`, `src/targets/mod.rs`
**Depends on:** Story 0.

---

### Story 2 — Shared pipeline: OpenAPI ingest & parse
**Goal:** Load local file or remote URL, JSON or YAML, into a normalized `openapiv3::OpenAPI` document, fully async.

- `src/openapi/source.rs`: `InputSource::{LocalFile, Url}` via `url::Url::parse` scheme sniff.
- `src/openapi/fetch.rs`: `async fn load_raw()` — `tokio::fs::read_to_string` / `reqwest::get(...).text()`.
- `src/openapi/parse.rs`: try `serde_json`, fall back to `serde_yaml`; extension-based fast path.
- `src/openapi/mod.rs`: `pub async fn ingest(input: &str) -> Result<OpenAPI>` (using the `openapiv3` crate's type).
- Tests: local JSON/YAML fixtures, malformed spec (error path), remote fetch via `wiremock` (JSON + YAML, 404/500, redirects).

**Critical files:** `src/openapi/mod.rs`, `src/openapi/parse.rs`
**Depends on:** Story 1. Parallel-safe with Stories 3, 4.

---

### Story 3 — Shared pipeline: directory guard & rollback bookkeeping
**Goal:** Safe output-dir handling; populate `output_dir_preexisted` for the rollback logic in `execute()`.

- `src/pipeline/dir_guard.rs`: `async fn check_output_dir(dir, force) -> Result<bool>` — non-empty + no `--force` → REQ-1.1.3 error; missing → create, return `false`; empty/forced → return `true`.
- Wire into `src/targets/mod.rs`'s default `execute()`: `tokio::fs::remove_dir_all` only when `result.is_err() && !output_dir_preexisted` (architecture.md lines 61-63, verbatim).
- Tests: fresh dir created (`preexisted=false`); empty existing dir (`preexisted=true`); non-empty without `--force` errors; non-empty with `--force` succeeds.
- Integration test with a fake always-failing target generator: fresh dir removed on failure; pre-existing (`--force`) dir survives with partial content intact.

**Critical files:** `src/pipeline/dir_guard.rs`, `src/targets/mod.rs`
**Depends on:** Story 1. Parallel-safe with Story 2.

---

### Story 4 — Shared pipeline: auth scheme profiling
**Goal:** Turn `components.securitySchemes` into `Vec<AuthSchemeDescriptor>` (Basic / ApiKey / BearerPat / OAuth2 / OAuth1), with interactive fallback.

- `src/auth_profile/descriptor.rs`: `AuthSchemeKind` enum + `AuthSchemeDescriptor` struct.
- `src/auth_profile/classify.rs`: map `type: http/scheme: basic` → Basic; `type: apiKey` → ApiKey; `type: http/scheme: bearer` (+ description heuristic for "personal access token") → BearerPat vs generic Bearer; `type: oauth2` → OAuth2; OpenAPI 3 has **no native OAuth1 scheme type** — detect via vendor extension (`x-auth-type: oauth1`) and document this heuristic explicitly as a real ambiguity source.
- `src/auth_profile/prompt.rs`: interactive fallback (via `dialoguer` or `inquire` crate) when schemes are missing/ambiguous; add a `--auth-scheme` non-interactive override flag for CI/scripted use.
- `src/auth_profile/mod.rs`: `pub async fn profile_auth(doc, interactive) -> Result<Vec<AuthSchemeDescriptor>>`.
- Tests: fixture per scheme shape; empty `securitySchemes` triggers prompt path; unrecognized `type` triggers prompt path.

**Critical files:** `src/auth_profile/classify.rs`, `src/auth_profile/prompt.rs`
**Depends on:** Story 2. Parallel-safe with Story 3.

---

### Story 5 — Shared pipeline: `mcp_store.db` assembly (schema + relational rows only)
**Goal:** Create `mcp_store.db` (`endpoints` relational table + an empty `semantic_endpoints` vec0 table) directly from Rust. Embedding *vectors* are deliberately **not** computed here — see the embeddings decision above; that happens later via a generated TypeScript script (Story 9/14), so the same code embeds both indexed operations and live queries.

- `src/db/schema.rs`: DDL — `endpoints(operation_id PK, path, method, summary, description, input_schema TEXT, output_schema TEXT, auth_scheme_ref)` and `CREATE VIRTUAL TABLE semantic_endpoints USING vec0(operation_id TEXT PRIMARY KEY, embedding FLOAT[768])` (768 dims fixed now to match `all-mpnet-base-v2`, since the generated populate script must agree with this schema).
- `src/db/open.rs`: open `rusqlite::Connection`, load the `sqlite-vec` extension in-process (needed only to create the vec0 table with the correct schema — Rust never inserts a vector into it).
- `src/db/populate.rs`: insert one row per operation into `endpoints`, JSON-encoding schemas via `serde_json`. The text later embedded by the TS script (`method + path + summary + description`) is derived from these same columns, so Story 9's `scripts/populate-embeddings.ts.tera` template must read `endpoints` and reconstruct that exact string, not receive it out-of-band.
- `src/db/mod.rs`: `pub async fn assemble_store(ctx, doc) -> Result<PathBuf>`.
- Tests: schema creation idempotency; relational round-trip; vec0 table creation succeeds empty (flag CI risk: `sqlite-vec` extension loading needs to work on macOS + Linux CI runners for this creation step).

**Critical files:** `src/db/schema.rs`, `src/db/open.rs`, `src/db/populate.rs`
**Depends on:** Story 2 (parsed doc), Story 4 (`auth_scheme_ref` values).

---

### Story 6 — Pipeline orchestration wiring
**Goal:** Glue Stories 2–5 into the single shared pre-target pipeline.

- `src/pipeline/mod.rs`: `run_shared_pipeline(cli) -> Result<(GeneratorContext, OpenAPI)>` — ingest → dir_guard → profile_auth → assemble_store, in that order.
- Update `src/main.rs` to call this before target dispatch.
- Extend `GeneratorContext` with `normalized_operations: Vec<NormalizedOperation>` (derived once, reused by every target step instead of re-querying `mcp_store.db` repeatedly) — a deliberate, minor extension beyond architecture.md's literal struct listing, justified by avoiding needless I/O in every downstream step.
- End-to-end test: small fixture spec (3-5 ops, 1 Basic + 1 OAuth2 scheme) → assert `mcp_store.db` row counts, `auth_schemes.len() == 2`, `output_dir_preexisted == false`.

**Critical files:** `src/pipeline/mod.rs`, `src/main.rs`
**Depends on:** Stories 2, 3, 4, 5.

---

### Story 7 — Template engine foundation & TS context model
**Goal:** Stand up `tera` + `rust-embed` infra and the render-context struct, before writing real `.tera` files.

- `src/targets/typescript/mod.rs`: `TypeScriptTargetGenerator`, `name() -> "typescript"`, 6 methods stubbed `Ok(())` so the trait compiles early.
- `src/targets/typescript/context.rs`: `TsTemplateContext` (project_name, package_name, display_name from `info.title`, client_class_name, auth_schemes view, operations view, tool_prefix — `search`/`get`/`call` per the resolved naming decision).
- `src/targets/typescript/naming.rs`: kebab/Pascal/camel/SCREAMING_SNAKE case helpers (used pervasively — every template needs consistent casing).
- `src/targets/typescript/templates/` + `#[derive(RustEmbed)] #[folder = "..."]` so templates compile into the binary (no filesystem dependency at runtime).
- `src/targets/typescript/render.rs`: build one `Tera` instance by iterating `RustEmbed::iter()` and `add_raw_template` for each.
- `src/targets/typescript/emit.rs`: `render_and_write(tera, template_name, ctx, out_path)` — render, `create_dir_all`, `write`, with error context.
- One trivial template (`package.json.tera`) + test proving embed→render→write works end to end.

**Critical files:** `src/targets/typescript/mod.rs`, `src/targets/typescript/render.rs`, `src/targets/typescript/naming.rs`
**Depends on:** Story 1 for plumbing; real data wiring needs Story 6.

---

### Story 8 — `bootstrap_project` (TypeScript)
**Goal:** Project skeleton, manifest, config files.

- `templates/package.json.tera` (parameterized name/bin/description; static dep list mirrored from the reference `package.json`: `@modelcontextprotocol/sdk`, `better-sqlite3`, `sqlite-vec`, `express`, `commander`, `inquirer`, `js-yaml`, `zod`, `keytar`, `oauth-1.0a`, `pino`, `@opentelemetry/*`, `@xenova/transformers`, devDeps `vitest`/`typescript`/`tsx`/`eslint`).
- `templates/tsconfig.json.tera`, `eslint.config.js.tera`, `.gitignore.tera`, `.env.example.tera` (keys derived from `ctx.auth_schemes`).
- `src/targets/typescript/steps/bootstrap.rs`: creates `src/{auth,cli,core,data,http,services,tools,validation}` dirs, writes manifest/config files.
- Confirm `mcp_store.db` (already written by the shared pipeline into `ctx.output_dir`) needs no copy step — verify only.
- `templates/README.md.tera` — generated usage doc mirroring reference structure.
- Snapshot test: file tree + `package.json` content spot-check.

**Critical files:** `src/targets/typescript/steps/bootstrap.rs`, `templates/package.json.tera`
**Depends on:** Story 7, Story 6.

---

### Story 9 — `generate_enterprise_scaffolding` (TypeScript)
**Goal:** Emit all ~17 inline core modules + Docker/CI, before any tool-specific code, per architecture.md's explicit ordering.

- One `.tera` per core module: `logger`, `tracing`, `config-manager`, `config-schema` (auth-method union type templated from `ctx.auth_schemes`, e.g. `'basic' | 'oauth2'`), `health-check-manager`, `circuit-breaker`, `credential-storage`, `mcp-server`, `rate-limiter`, `cache-manager`, `correlation-context`, `sanitizer`, `api-url-builder`, `component-registry`, `shutdown-handler`, `errors`, `log-transport`, `healthcheck.ts`.
- `src/targets/typescript/steps/enterprise.rs`: iterate a static `Vec<(template, out_path)>` manifest.
- `templates/scripts/populate-embeddings.ts.tera` — the generated npm script that reads `endpoints` from `mcp_store.db`, reconstructs `method + path + summary + description` per row, computes vectors via the shared `embedding-service.ts` (built in Story 12, imported here too — this script and the runtime `search` tool are the two consumers of one embedding implementation), and inserts them into `semantic_endpoints`. This is the direct analog of the reference servers' `generate-embeddings`/`populate-db` scripts, adapted to mcpify's single-`mcp_store.db` design.
- `templates/Dockerfile.tera` + `templates/docker-compose.yml.tera` (stdio + http variants) — like the reference servers' `Dockerfile`, the builder stage must run `npm run populate-embeddings` (or equivalent) before `tsc`/tests, since `mcp_store.db` leaves the Rust generator with an empty `semantic_endpoints` table (Story 5). This mirrors, rather than simplifies away, the reference servers' build-time embedding-generation gate.
- `templates/.github/workflows/{ci,docker-build,release}.yml.tera` — static job structure, parameterized package name only.
- Snapshot test: full core/+Docker/CI tree; `config-schema.ts` auth-union correctness for 2-scheme and 4-scheme fixtures.

**Critical files:** `src/targets/typescript/steps/enterprise.rs`, `templates/core/config-schema.ts.tera`, `templates/Dockerfile.tera`, `templates/scripts/populate-embeddings.ts.tera`
**Depends on:** Story 8, Story 7.

---

### Story 10 — `generate_auth_strategies` (TypeScript)
**Goal:** One strategy module per discovered `AuthSchemeDescriptor`, plus the auth-manager.

- `templates/auth/auth-strategy.ts.tera` — shared `Credentials`/`AuthStrategy` interface (always emitted).
- `templates/auth/strategies/{basic,pat,oauth1,oauth2,stub}.ts.tera` — oauth1 with RSA-SHA1 signing (`oauth-1.0a`), oauth2 with PKCE + refresh; `stub` always emitted as safe fallback.
- `templates/auth/errors.ts.tera`.
- `src/targets/typescript/steps/auth.rs`: for each `AuthSchemeDescriptor`, map kind → template, render with per-scheme view data (env var names, OAuth2 authorize/token URLs from the OpenAPI `flows` object).
- `templates/auth/auth-manager.ts.tera` — builds the `auth_method` → strategy selection from `ctx.auth_schemes` only (no dangling imports for undiscovered schemes).
- Snapshot tests: Basic+OAuth2-only fixture (5 files) and all-4-kinds fixture (6 files); assert no dangling imports.

**Critical files:** `src/targets/typescript/steps/auth.rs`, `templates/auth/strategies/oauth2.ts.tera`, `templates/auth/auth-manager.ts.tera`
**Depends on:** Story 9 (imports core modules), Story 4 (scheme data), Story 7.

---

### Story 11 — `generate_transports_and_roles` (TypeScript)
**Goal:** Terminal Client + Harness Server entry points, stdio/HTTP transport wiring.

- `templates/index.ts.tera` — Harness Server bootstrap order: config → logger → registry → shutdown-handler → db → auth → services → McpServer → register tools → pick transport → health checks.
- `templates/cli.ts.tera` — Commander.js entry registering 9 subcommands (`setup`, `search`, `get`, `call`, `start`, `http`, `test-connection`, `config`, `version`).
- `templates/cli/*.ts.tera` — one file per subcommand (thin dispatchers; `setup-wizard.ts` itself is Story 13).
- `templates/http/*.ts.tera` — server, request-handler, auth-extractor, localhost-detector (relaxed auth on localhost), metrics (Prometheus), types.
- `src/targets/typescript/steps/transports.rs`: orchestration. mcpify always emits both stdio and http capability; transport *selection* is runtime config, not generation-time.
- Snapshot test: full `src/http/` + `src/cli/` + root entry-points tree.

**Critical files:** `src/targets/typescript/steps/transports.rs`, `templates/index.ts.tera`, `templates/cli.ts.tera`
**Depends on:** Story 9, Story 10. Parallel-safe with Story 12 once Story 10 is done.

---

### Story 12 — `generate_mcp_tools` (TypeScript): search / get / call
**Goal:** The core value-delivery step — 3 tools, data layer against `mcp_store.db`, target-API HTTP client, validation.

- `templates/data/store-repository.ts.tera` — single-file replacement for the reference's split `operations-repository.ts` + `schema-resolver.ts` (REQ-2.4.1 consolidation): relational queries against `endpoints`, vector similarity queries against `semantic_endpoints`, via `better-sqlite3` + `sqlite-vec` loaded at Node runtime.
- **Shared embedding service (the single source of truth for the embeddings decision):** `templates/services/embedding-service.ts.tera` — wraps `@xenova/transformers`, pinned to `Xenova/all-mpnet-base-v2` (768-dim, matching Story 5's fixed vec0 schema). Exposes one `embed(text: string): Promise<number[]>` function. Two callers import it: the generated `search` tool (embeds the live user query) and `scripts/populate-embeddings.ts` (Story 9, embeds each operation at setup time). Because both go through this one file, there is no separate "keep the models in sync" step to maintain — it's structurally impossible for them to drift.
- `templates/services/api-client.ts.tera` — generic `execute(operationId, params)` dispatcher table (not one method per operation) to stay robust against specs with hundreds of operations and avoid an unreadable loop-heavy Tera file. Wraps `axios` through the circuit-breaker/retry/rate-limiter from Story 9.
- `templates/tools/{search-tool,get-tool,call-tool,register-tools,tool-executor}.ts.tera` — tool names `search`/`get`/`call` per the resolved naming decision; descriptions templated from `ctx.display_name`.
- `templates/validation/validator.ts.tera` (Ajv-based) + schemas emitted as a **co-located JSON asset** (not a giant generated `.ts` file — avoids TS-compiler strain on large specs), loaded via `JSON.parse(readFileSync(...))` at runtime, built from `ctx.operations[].input_schema/output_schema` via plain Rust `serde_json` string-building (not Tera looping).
- `src/targets/typescript/steps/tools.rs`: orchestration over `ctx.normalized_operations`.
- Snapshot tests: tool files exist with correct names; a dedicated schema round-trip test against a spec with `$ref`/`allOf`/nested objects (highest functional risk in this story).

**Critical files:** `src/targets/typescript/steps/tools.rs`, `templates/data/store-repository.ts.tera`, `templates/services/embedding-service.ts.tera`, `templates/tools/call-tool.ts.tera`, `templates/validation/validator.ts.tera`
**Depends on:** Stories 9, 10, 11, 5/6. Largest story — consider splitting into sub-tickets (data-layer, api-client, 3 tools, validation) if executed by a team.

---

### Story 13 — `generate_setup_wizard_and_tests` (TypeScript)
**Goal:** Interactive `setup` wizard + full generated test suite exercising Stories 8–12.

- `templates/cli/setup-wizard.ts.tera` — base-URL + connectivity check, auth-method prompt driven by `ctx.auth_schemes` (only prompts for schemes actually discovered, unlike the reference which always prompts all 4), method-specific credential prompts, then REQ-1.6.2's 3 persistence choices exactly (`.env` / `config.json` / print-CLI-invocation-only).
- `templates/vitest.config.ts.tera` — conditional inclusion via `RUN_INTEGRATION_TESTS`/`RUN_E2E_TESTS`.
- `templates/tests/helpers/*.ts.tera` — mcp-test-client, mock-api-server (built from `ctx.operations` so mocked routes match the real spec), log-capture, skip-integration.
- `templates/tests/unit/**/*.ts.tera` — one file per generated module, manifest-driven loop in `steps/tests.rs` rather than hand-writing 40+ Rust calls.
- `templates/tests/integration/*.ts.tera`, `templates/tests/e2e/e2e-mcp.test.ts.tera`.
- **Correctness requirement, not just cleanliness:** unit-test generation must be conditional on which schemes/tools actually exist — an emitted test importing a non-generated module (e.g. `oauth1-strategy.test.ts` when OAuth1 wasn't discovered) breaks `run_generated_tests` (Story 14) outright.

**Critical files:** `src/targets/typescript/steps/setup_and_tests.rs`, `templates/cli/setup-wizard.ts.tera`
**Depends on:** Stories 9, 10, 11, 12.

---

### Story 14 — `run_generated_tests` + rollback integration
**Goal:** Install deps and run the emitted suite to completion as the final generation step; this is the v1 launch gate.

- `src/targets/typescript/steps/run_tests.rs`: `tokio::process::Command` → `npm install` (fresh, no vendored lockfile, to avoid drift against evolving pinned template deps) → `npm run populate-embeddings` (Story 9's script — required before tests, since `mcp_store.db` leaves Story 5 with an empty `semantic_endpoints` table and the `search` tool's tests need real vectors to query against) → `npm test` (vitest runs directly against `.ts` via `tsx`/vitest's own transform — no separate build/compile step, matching architecture.md's framing exactly).
- The `populate-embeddings` step downloads the `Xenova/all-mpnet-base-v2` model on first run (via `@xenova/transformers`' own HF Hub caching) — this is the same lazy-download UX the reference servers already have, just invoked automatically by `run_generated_tests` instead of a developer running it by hand. Flag as a real timeout/network-dependency risk (see the `tokio::time::timeout` task below) since it's now on the critical path of every `mcpify` invocation, not an optional dev step.
- Capture child-process stdout/stderr into `anyhow::Error` context on failure (actionable CLI error message).
- `tokio::time::timeout` wrapper (generous default, e.g. 5 min) — `npm install` could hang in constrained sandboxes.
- Confirm a forced failure (broken template in a test fixture) triggers Story 3's rollback path.
- Slow, `#[ignore]`-by-default integration test: full real pipeline against a small fixture spec, asserting `mcpify` exits 0 and the emitted project's `npm test` genuinely passes.

**Critical files:** `src/targets/typescript/steps/run_tests.rs`
**Depends on:** Story 13. **Treat completion of this story as the v1 launch milestone.**

---

### Story 15 — Generator's own test suite (cargo test)
**Goal:** REQ-2.6.1 — consolidate fixtures, ensure ingestion/dir-guard/auth-profiling/db-assembly coverage breadth.

- `tests/fixtures/openapi/`: minimal-basic-auth, minimal-oauth2, minimal-multi-scheme, minimal-no-auth-scheme (prompt fallback), malformed (error path), a `$ref`/`allOf`-heavy medium spec (schema-resolution stress test).
- Consolidate multi-module tests under `tests/` (Rust integration-test convention); keep true unit tests inline per-module.
- `wiremock`-based remote-fetch suite: JSON + YAML, 404/500, redirects.
- `cargo-llvm-cov` or `cargo-tarpaulin` as a dev tool + coverage check.

**Critical files:** `tests/fixtures/openapi/`, `tests/remote_ingest.rs`
**Depends on:** Stories 2–6 (grows incrementally alongside them).

---

### Story 16 — Golden/snapshot tests for `execute()`
**Goal:** REQ-2.6.1's golden-file requirement — the strongest regression guard against template drift.

- Adopt `insta` for Rust snapshot testing (`cargo insta review` workflow).
- `tests/golden/typescript_snapshot.rs`: for each fixture spec (from Story 15), run `TypeScriptTargetGenerator::execute()` into a temp dir (`tempfile` crate), then walk the output tree and snapshot either (a) the full file tree + hashes of file contents (cheap, catches any change) or (b) full content snapshots of a curated subset of "interesting" files (config-schema.ts, auth-manager.ts, register-tools.ts, package.json) — recommend (b) for signal-to-noise since full-content snapshots of all ~80-120 emitted files would be noisy and slow to review on every template tweak; keep (a) as a cheap file-tree-shape smoke test that always runs.
- Stub out `run_generated_tests` in the golden-test path (inject a no-op/mock implementation via a test-only flag or trait-object substitution) so these tests don't require `npm install`/network access and stay fast — separate concern from Story 14's slow end-to-end test.
- Golden fixtures for each of the 4 auth-scheme combinations (single Basic, single OAuth2, all 4 combined, zero schemes/prompt-fallback-mocked) to ensure conditional file emission (Story 10, Story 13 task 6) is snapshot-covered.
- Wire `cargo insta test` into CI (Story 17) as a required check; document the snapshot-review workflow for contributors touching templates.

**Critical files:** `tests/golden/typescript_snapshot.rs`, `tests/fixtures/openapi/`
**Depends on:** Story 14's `TypeScriptTargetGenerator` being feature-complete (all 6 methods real, not stubs) — so this story is naturally last among the TS-target work, though snapshot infra (task 1-2) can be scaffolded earlier against partial output.

---

### Story 17 — CI for mcpify itself
**Goal:** REQ-2.6.2 — the generator's own suite runs on every commit and blocks merge on red.

- `.github/workflows/ci.yml`: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` (fast unit/integration/golden), plus a separate slower job for Story 14's real `npm install`+vitest end-to-end test (label-gated or nightly, needs Node in the runner).
- Matrix fast job across macOS + Linux (`sqlite-vec` extension loading is platform-sensitive — flagged flakiness risk); cache `cargo` and `npm`.
- `.github/workflows/release.yml`: `cargo publish` on tag push (Homebrew tap noted as a follow-up, out of scope for v1 code).
- Branch protection requiring green CI (repo setting, document as a follow-up for repo admin).

**Critical files:** `.github/workflows/ci.yml`, `.github/workflows/release.yml`
**Depends on:** Story 15/16 existing; can scaffold early with a trivial job and expand.

---

## Sequencing

1. Story 0 → Story 1 (hard prerequisite for everything).
2. Stories 2, 3, 4 in parallel once Story 1 lands.
3. Story 5 depends on 2+4 — **spike `sqlite-vec`-via-`rusqlite` extension loading early** (schema/table creation only, no in-process vector math) in parallel with 2-4. The embedding computation itself lives entirely in generated TypeScript (Story 9's `populate-embeddings.ts.tera` + Story 12's `embedding-service.ts.tera`), so its risk surface (model download, ONNX inference) is deferred to Story 14's real end-to-end test rather than being a Rust-side unknown.
4. Story 6 merges 2-5.
5. Story 7 (template engine) scaffolds in parallel with 2-6, wired to real data once 6 lands.
6. Stories 8 → 9 → 10 → (11 ‖ 12, both depend on 10) → 13 → 14, following the trait's literal method order (architecturally mandated, not just convenient — architecture.md is explicit that enterprise scaffolding must precede tool generation).
7. Story 15 grows incrementally alongside 2-6; Story 16 needs 14 substantially complete; Story 17 starts early (trivial CI) and expands throughout.

**v1 launch milestone = Story 14 complete and green.**

## Verification

- Per-story: `cargo test` covering that story's module (unit tests inline, integration tests under `tests/`).
- Story 5: verify `sqlite-vec` extension loads and the empty `semantic_endpoints` table is created with the correct schema.
- Story 9/14: verify `npm run populate-embeddings` produces sane cosine-similarity ordering against a tiny known fixture (3-5 hand-picked operations with obviously-different summaries), and that the `search` tool's live-query results agree with that ordering — this is where the embeddings decision's correctness actually gets proven, not Story 5.
- Story 16: `cargo insta test` — any unintended template diff fails CI; intentional changes reviewed via `cargo insta review`.
- Story 14 (the real acceptance gate): run `mcpify -i <fixture-spec> -o /tmp/test-output` end to end, then `cd /tmp/test-output && npm install && npm test` manually and confirm it passes — this is the same check `run_generated_tests` automates, so manually reproducing it once during Story 14 development is the sanity check before trusting the automated version.
- Full v1 acceptance (matches PRD REQ-2.5.2): run mcpify against a spec resembling the actual Bitbucket or Jira Data Center OpenAPI spec (available in the reference repos) and confirm the generated project's tests pass unedited — the closest real-world proof the generator meets parity with the hand-built references.

## What actually shipped (retrospective)

Everything above was implemented as planned, with a few real-world adjustments discovered only by running the generated output for real (Story 14):

- **Dependency versions drift fast.** `better-sqlite3 ^11.3.0` failed to compile its native addon on newer Node versions; the entire `@opentelemetry` package set needed a coordinated version bump (independently-caret-ranged peer packages resolved to mutually incompatible versions); `ajv`'s default import proved unreliable under `"moduleResolution": "NodeNext"` (fixed via the named `{ Ajv }` import); the MCP SDK's `Server` class was swapped for the higher-level `McpServer` class once tool registration (`.tool()`) was actually implemented, since the low-level class doesn't expose it.
- **Lint/format are not optional in practice**, even though they're not part of `run_generated_tests`' own gate: since Story 9 also generates the project's own `ci.yml` running `lint`/`format:check`, `run_generated_tests` (Story 14) now also runs `npm run format` (auto-fixing Prettier drift) so the generated project's own CI is never red on first push.
- **`--force` regeneration into a directory with a stale `mcp_store.db`** hit a `PRIMARY KEY` constraint on re-insert; `assemble_store` (Story 5) now removes a preexisting store file before recreating it.
- **`run_shared_pipeline` (Story 6) needed its own rollback**, mirroring `execute()`'s: a failure in auth profiling (after the directory guard already created `output_dir`) was leaving an empty directory behind, since rollback previously only lived inside the target trait's `execute()`.
- Story 14's `#[ignore]`-gated real end-to-end test (`tests/e2e_generation.rs`) was written to close the loop Story 17's CI needed — a concrete, automated test target for the "real npm install + npm test" acceptance gate, not just a manually-reproduced check.
