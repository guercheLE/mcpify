# Architectural Specification Document: mcpify

This document describes the two-layer architecture of mcpify: the **Compile-Time Generator** (the `mcpify` CLI itself, implemented in **Rust**) and the **Runtime Target Architecture** (the shape of every project it emits). The generator's internal interface is built around the Strategy Pattern: one `McpServerTargetGenerator` implementation per output language. Five output languages are planned, shipped in priority order — **TypeScript** (v1), **Rust** (v2), **Python** (v3), **C#** (v4), **Go** (v5) — see § "Target Language Roadmap" below. Only the target-language implementation changes across these releases; the generator's own implementation language (Rust) does not.

---

## 1. Compile-Time Generator Framework (Rust)

### Core Interface

```rust
use async_trait::async_trait;
use anyhow::Result;
use std::path::PathBuf;

pub struct GeneratorContext {
    pub openapi_input: String,       // local path or URL
    pub output_dir: PathBuf,
    pub force: bool,
    pub output_dir_preexisted: bool, // true if output_dir already had content before this run (via --force)
    pub auth_schemes: Vec<AuthSchemeDescriptor>, // discovered from components.securitySchemes
}

/// The blueprint every output-language target must satisfy.
/// Each method corresponds 1:1 to a step of the Compile-Time Lifecycle below.
#[async_trait]
pub trait McpServerTargetGenerator: Send + Sync {
    fn name(&self) -> &'static str; // e.g. "typescript"
    async fn bootstrap_project(&self, ctx: &GeneratorContext) -> Result<()>;
    async fn generate_enterprise_scaffolding(&self, ctx: &GeneratorContext) -> Result<()>;
    async fn generate_auth_strategies(&self, ctx: &GeneratorContext) -> Result<()>;
    async fn generate_transports_and_roles(&self, ctx: &GeneratorContext) -> Result<()>;
    async fn generate_mcp_tools(&self, ctx: &GeneratorContext) -> Result<()>;
    async fn generate_setup_wizard_and_tests(&self, ctx: &GeneratorContext) -> Result<()>;
    /// Installs dependencies and executes the generated project's own test suite
    /// (the one emitted by `generate_setup_wizard_and_tests`) to completion.
    /// There is deliberately no separate "verify it builds" step: a target-language
    /// test runner cannot execute tests against code that doesn't compile/type-check,
    /// so a passing test run is both the build proof and the correctness proof —
    /// enforcing the zero-placeholder quality bar (PRD REQ-2.5.1) with one signal
    /// instead of two. A run that generates code but whose tests fail (or don't
    /// run) is not a successful `execute()`.
    async fn run_generated_tests(&self, ctx: &GeneratorContext) -> Result<()>;

    async fn execute(&self, ctx: &GeneratorContext) -> Result<()> {
        let result = async {
            self.bootstrap_project(ctx).await?;
            self.generate_enterprise_scaffolding(ctx).await?;
            self.generate_auth_strategies(ctx).await?;
            self.generate_transports_and_roles(ctx).await?;
            self.generate_mcp_tools(ctx).await?;
            self.generate_setup_wizard_and_tests(ctx).await?;
            self.run_generated_tests(ctx).await
        }
        .await;

        // Roll back a failed run so it doesn't leave a broken, half-generated
        // project blocking the next attempt — but only when mcpify created
        // output_dir fresh. Never delete a pre-existing directory the user
        // pointed --force at.
        if result.is_err() && !ctx.output_dir_preexisted {
            let _ = tokio::fs::remove_dir_all(&ctx.output_dir).await;
        }
        result
    }
}
```

Only `TypeScriptTargetGenerator` ships in v1; the registry that dispatches on `--language` is a `HashMap<String, Box<dyn McpServerTargetGenerator>>`, so `RustTargetGenerator`, `PythonTargetGenerator`, `CSharpTargetGenerator`, and `GoTargetGenerator` are registered in later releases without touching the orchestration pipeline below (§ "Target Language Roadmap").

### Generator Toolchain (Rust crates)

* **Async runtime:** `tokio`, `async-trait`.
* **CLI parsing:** `clap`.
* **OpenAPI ingestion:** `reqwest` (remote URL fetch), `tokio::fs` (local file read), `serde_json` / `serde_yaml` (parsing), `url` (input-kind detection).
* **Database assembly:** `rusqlite` with the `sqlite-vec` extension.
* **Template synthesis:** `tera` (or plain string/file emitters) to render the TypeScript project files.
* **Error handling:** `anyhow`.

### Compile-Time Lifecycle

Steps 1–4 are shared Rust code in the generator, run once before dispatching to any target — they populate `GeneratorContext` and `mcp_store.db`, and are identical regardless of `--language`. Steps 5–11 are the per-target trait methods (§ Core Interface), invoked in this order by the default `execute`:

1. **Ingest & parse** *(shared)*. Load the OpenAPI spec asynchronously — from a local file path or by fetching a remote `http`/`https` URL — and parse JSON or YAML into a normalized in-memory document.
2. **Directory guard** *(shared)*. Non-blocking check of `output_dir`; abort with a warning unless it is empty or `--force` is set. Records whether the directory pre-existed (`output_dir_preexisted`), so a failed run later knows whether it's safe to roll back.
3. **Auth profiling** *(shared)*. Walk `components.securitySchemes`, classify each scheme (Basic / API Key / Bearer-PAT / OAuth2 / OAuth1-style), and build one `AuthSchemeDescriptor` per scheme. If none are declared or the declaration is ambiguous, fall back to an interactive CLI prompt.
4. **Database assembly** *(shared)*. Create `mcp_store.db` (SQLite + `sqlite-vec`) directly in the output directory:
   * `endpoints` table — one row per operation: `operation_id`, `path`, `method`, `summary`, `description`, `input_schema` (JSON Schema), `output_schema` (JSON Schema), `auth_scheme_ref`.
   * `semantic_endpoints` virtual table (`vec0`) — one embedding per operation, computed from `method + path + summary + description`, keyed by `operation_id`.
5. **`bootstrap_project`** *(per-target)*. Initialize the project skeleton (`npm init`-equivalent, manifest/dependency files, base directory layout).
6. **`generate_enterprise_scaffolding`** *(per-target)*. Before any tool-specific code is written, emit the shared enterprise modules every later file will depend on: structured logger (with redaction), OpenTelemetry setup (tracing + metrics exporters), circuit breaker/retry/rate-limit wrapper, health-check registry, credential-storage adapter (OS keychain + encrypted-file fallback), config resolver, Dockerfile + docker-compose, and the CI workflow. This ordering is what guarantees files generated later never need to be revisited to "add" enterprise features — they're already available to import.
7. **`generate_auth_strategies`** *(per-target)*. Emit one auth-strategy module per `AuthSchemeDescriptor` discovered in step 3, plus the auth-manager that selects the single active strategy from config at runtime.
8. **`generate_transports_and_roles`** *(per-target)*. Emit the Terminal Client and Harness Server entry points and the stdio/HTTP transport wiring.
9. **`generate_mcp_tools`** *(per-target)*. Emit the three tool modules (`search`/`get`/`call`) against `mcp_store.db` and the target-API HTTP client.
10. **`generate_setup_wizard_and_tests`** *(per-target)*. Emit the interactive `setup` command and the generated test suite (unit/integration/e2e) exercising steps 5–9.
11. **`run_generated_tests`** *(per-target)*. Install dependencies and run the test suite emitted in step 10 to completion; the run only counts as a success once those tests pass. No separate build/compile step exists — running the tests already requires the project to build (or, for interpreted targets, to type-check/import cleanly), so a green test run is the single source of truth for the zero-placeholder quality bar (PRD REQ-2.5.1). On failure at any of steps 5–11, and only if `output_dir_preexisted` is `false`, `execute` removes `output_dir` before returning the error, so a failed run never leaves a broken half-generated project on disk blocking the next attempt.

`generate` always seeds a version ledger (§5) as an implicit step 12 after `execute()` succeeds, recording the just-ingested spec under a label (`"default"` unless `--api-version` was passed). This is the only change v8 makes to the lifecycle above — everything else about `generate` is unchanged.

### `add-version`: a lighter, separate lifecycle

`mcpify add-version` (§5) extends an *already-generated* project with another spec version, without running steps 2–3 or 5–11 above: no directory guard (the project already exists), no auth re-profiling (auth strategies are version-independent), and none of `bootstrap_project` through `run_generated_tests` re-run. It only reuses step 1 (ingest & parse) and a parameterized form of step 4 (store assembly, targeting a version-suffixed path instead of the hardcoded `mcp_store.db`), then re-renders a small, fixed set of "version-aware" files per target (§5) — never auth strategies, enterprise scaffolding, transports, or tests.

---

## 2. Runtime Target Architecture (v1: TypeScript/Node.js)

### Component Stack

```
                    Generated mcpify Project
        ┌─────────────────────────────────────────────┐
        │  Terminal Client (MCP Client / CLI proxy)    │
        │  search | get | call | setup | test-connection│
        └───────────────────────┬───────────────────────┘
                                 │
        ┌────────────────────────────────────────────┐
        │  Harness Server (MCP Server: stdio | http)  │
        └───────────────────────┬──────────────────────┘
                                 ▼
                    Configuration Resolver
        (CLI flags → env → local file → home file →
          system file → install-dir file → defaults)
             includes `api_version` (v8) alongside
                    `auth_method` — one active
                 version/strategy each, per process
                                 ▼
                     Unified Facade Engine
                  ┌──────────────┴──────────────┐
                  ▼                              ▼
    mcp_store[_v<label>].db (sqlite-vec)  Auth Strategy (active)
         [search: vector match]          [Basic|PAT|OAuth1|OAuth2]
         [get: relational lookup]                 │
                  │                                │
                  └───────────────┬────────────────┘
                                   ▼
                    call: validate → inject auth →
                    dispatch (circuit breaker/retry) →
                    validate response
                                   ▼
                         [ Target Enterprise API ]

  Cross-cutting (always present): structured logger, OpenTelemetry
  tracing/metrics, health-check registry, credential storage.
```

### Folder Structure

```
generated-project/
├── mcp_store.db              # embedded relational + vector store
├── src/
│   ├── auth/
│   │   ├── strategies/       # one file per discovered auth scheme
│   │   └── auth-manager.ts   # selects the active strategy per config
│   ├── cli/                  # setup, search, get, call, test-connection, version, config, help
│   ├── core/                 # logger, tracing, config resolver, health-check, circuit-breaker, credential-storage, mcp-server
│   ├── data/                 # mcp_store.db repository (relational + vector queries)
│   ├── http/                 # HTTP transport (server + metrics endpoint)
│   ├── services/             # target-API HTTP client
│   ├── tools/                # search, get, call tool implementations
│   ├── validation/           # input/output JSON Schema validation
│   ├── index.ts              # Harness Server entry point (stdio/http)
│   └── cli.ts                # Terminal Client entry point
├── tests/                    # generated unit/integration/e2e scaffold
├── Dockerfile                # multi-stage build
├── docker-compose.yml        # stdio + http service variants
├── .github/workflows/        # CI/CD (build, test, release)
└── README.md
```

This mirrors the folder shape already validated by `bitbucket-dc-mcp` and `jira-dc-mcp` (`src/{auth,cli,core,data,http,services,tools,validation}`), with the addition of the Terminal Client's genuine MCP-client role, which the two reference servers did not implement.

### The `call` Pipeline

1. **Input guard validation** — incoming arguments checked against `input_schema` from `mcp_store.db`; failures short-circuit before any network call.
2. **Credential injection** — the active auth strategy (selected via `auth_method` in the resolved config) attaches the required header/query/basic credentials.
3. **Dispatch** — the request goes out through the circuit-breaker/retry/rate-limit wrapper to the target API, fully async.
4. **Output guard validation** — the response is checked against `output_schema`; a mismatch is surfaced as a structured error rather than silently returned, protecting the calling agent from API drift.

Every step above emits structured logs and OpenTelemetry spans/metrics by default.

### Dual-Role Execution

* **Terminal Client mode (default):** invoking the generated binary/package directly runs it as an interactive CLI — `search`, `get`, `call` are first-class subcommands a human can run in a shell, in addition to the tool being able to open a genuine outbound MCP client connection when configured to do so.
* **Harness Server mode (`--server` / `start` subcommand):** boots the persistent MCP server loop, transport selected via the config cascade (`stdio` default, `http` with `--port`/`--host`/CORS), for consumption by an agent harness.

### Data Layer

Contrary to the split-file approach used in the two reference servers (`embeddings.db` + `operations.json` + `schemas.json`), mcpify emits a **single `mcp_store.db`** containing both the relational `endpoints` table and the `sqlite-vec` virtual table `semantic_endpoints`. This keeps every generated project self-contained in one artifact, at the cost of losing the ability to regenerate embeddings independently of relational data — an acceptable trade-off given generation is cheap and re-runnable end to end.

**v8 multi-version support** extends this with one store *per version* rather than a version column inside a shared store: the default version keeps the exact `mcp_store.db` path above; every additional version (added via `mcpify add-version`, §5) gets its own `mcp_store_v<label>.db` sibling, selected at runtime by the `api_version` config field exactly the way `auth_method` already selects one active auth strategy from several discovered ones. The per-target "generated schemas" JSON asset (§1 step 9, used by input/output validation) follows the same one-file-per-version pattern.

---

## 3. Target Language Roadmap

Every target below implements the same `McpServerTargetGenerator` trait, produces the same folder shape (§2), the same single `mcp_store.db`, the same dual client/server role, the same single-strategy auth model, and the same enterprise-first bar (logging, tracing/metrics, circuit breaker, health checks, credential storage, tests, Docker, CI) — only the concrete toolchain changes. Each target must reach parity with the v1 TypeScript output before it ships.

| Priority | Language | Async model | MCP SDK | DB driver + vector ext. | HTTP client (outbound) | HTTP/transport server | Schema validation | Structured logging | Tracing/metrics |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| v1 | **TypeScript / Node.js** | Node.js event loop / Promises | `@modelcontextprotocol/sdk` | `better-sqlite3` + `sqlite-vec` | `axios` | `express` (stdio via SDK transport) | `Ajv` | `winston`/`pino`-style JSON logger | `@opentelemetry/*` (Jaeger + Prometheus exporters) |
| v2 | **Rust** | `tokio` | official Rust MCP SDK / `async-trait` handlers | `rusqlite` + `sqlite-vec` | `reqwest` | `axum` | `jsonschema` crate | `tracing` + `tracing-subscriber` (JSON) | `tracing-opentelemetry` |
| v3 | **Python** | `asyncio` | `mcp` (Python SDK) | `sqlite3`/`aiosqlite` + `sqlite-vec` | `httpx` | `fastapi` + `uvicorn` | `jsonschema` / `pydantic` | `structlog` (JSON) | `opentelemetry-sdk` |
| v4 | **C# (.NET)** | `Task`/`async`-`await` | community/official .NET MCP SDK | `Microsoft.Data.Sqlite` + `sqlite-vec` | `HttpClient` | Kestrel (`WebApplication`) | `JsonSchema.Net` | `Serilog` (compact JSON) | `OpenTelemetry.Extensions.Hosting` |
| v5 | **Go** | goroutines + channels | community Go MCP SDK | `mattn/go-sqlite3` + `sqlite-vec` | `net/http` client | `net/http` server | `gojsonschema` | `zap` (JSON) | `go.opentelemetry.io/otel` |

Rollout notes:

* **Ordering rationale.** TypeScript ships first because it is the only stack already validated end-to-end by `bitbucket-dc-mcp`/`jira-dc-mcp`. Rust ships second so the generator can eventually dogfood its own output language. Python, C#, and Go follow in that order, matching demand from AI/ML tooling (Python) and enterprise/.NET or infra (C#, Go) ecosystems respectively.
* **Shared generator code.** All six per-target trait methods (`bootstrap_project` through `run_generated_tests`) are re-implemented per target; OpenAPI ingestion, auth profiling, and `mcp_store.db` assembly (§1, steps 1–4) are entirely shared Rust code in the generator, run once regardless of `--language`.
* **CLI invocation of generated output** differs per target and must be documented in each generated project's own `README.md`: `node dist/index.js` (TypeScript, after `npm run build`), `./<binary> --server` (Rust/Go, static binaries, no runtime needed), `python main.py --server` (Python, inside a venv), `dotnet <assembly>.dll --server` or a self-contained executable (C#).
* **Generated test tooling per target**, run by `run_generated_tests`: `vitest` (TypeScript, as already proven by the two reference servers), `cargo test` (Rust), `pytest` (Python), `dotnet test` (C#), `go test` (Go).
* **v8 multi-version support** (§5) is tracked as its own phased rollout in `docs/v8-implementation-plan.md`, in the same TypeScript→Rust→Python→C#→Go order as the original target rollout above, layered on top of it rather than renumbering it.

---

## 5. Multi-Version Spec Support (v8)

Lets an operator layer additional OpenAPI spec versions onto an already-generated project via `mcpify add-version --project <dir> --version <label> --input <file-or-url> [--set-default] [--force]`, without regenerating the project. See `docs/v8-implementation-plan.md` for the full implementation history; this section is the authoritative reference for the design itself.

### Ledger

Every generated project carries a generator-only bookkeeping file, `.mcpify/versions.json`:

```json
{
  "schema_version": 1,
  "language": "typescript",
  "display_name": "Jira Software",
  "project_name": "jira-mcp",
  "default_version": "11.3",
  "versions": {
    "11.3": { "db_file": "mcp_store.db", "schemas_file": "src/validation/generated-schemas.json", "source": "...", "added_at": 1700000000 },
    "11.2": { "db_file": "mcp_store_v11.2.db", "schemas_file": "src/validation/generated-schemas_v11.2.json", "source": "...", "added_at": 1700000100 }
  }
}
```

`language`/`display_name`/`project_name` are written once by `generate` and never re-derived from a later spec. **The generated project's own runtime code never reads this file** — it exists purely so the stateless `mcpify` process can recover a project's version state across separate `add-version` invocations, deliberately avoiding a JSON-manifest-parsing dependency in 5 different languages.

### Version-aware regions

Each target has a small, fixed set of generated files (config schema, the data-layer store-path resolver, the validator's schemas-file resolver, the setup wizard's version prompt, and a `versions` CLI subcommand) with exactly one marker-delimited region each:

```
// mcpify:versions:begin
... a small, pure-data code literal (a label→file map, or a list of choices) ...
// mcpify:versions:end
```

`generate` renders these regions the normal way, via Tera, using the ledger's initial single-entry state. `add-version` re-renders **only** these regions — via direct Rust string patching (`add_version::marker_region::patch_marked_region`), not Tera — without reconstructing the full original rendering context (auth schemes, project name, etc.), since the region is always pure data. Everything else in the project (auth strategies, enterprise scaffolding, transports, tests) is version-independent and untouched by `add-version`.

Compile-time-embedding targets (Rust's `include_str!`, C#'s embedded resources, Go's `go:embed`) require **a rebuild after `add-version`** before a newly added version actually works, since the schemas asset is baked into the binary. TypeScript and Python read their schemas asset from disk at runtime, so `add-version` alone is enough for those two.

### `--set-default`

Promotes a version to become the project's new default: demotes the outgoing default to its own suffixed files (`rename`, not delete — its data is never silently destroyed) unless this is a self-promotion (the label being promoted is already default, which just refreshes it in place), ingests the new spec straight to the canonical (unsuffixed) paths, and re-renders the version-aware regions. Promoting a label that was already `add-version`'d earlier as non-default is handled explicitly: its now-superseded suffixed files are removed only *after* the new data safely lands at the canonical paths.

### `versions` subcommand

Every generated project (all 5 targets) exposes a `versions` subcommand listing every known version, marking which is `default` and which is `active` for the current process (read from the resolved `api_version` config value) — the runtime-visible counterpart to the ledger, useful for an operator or an agent to discover what's available without inspecting `.mcpify/versions.json` directly.

---

## 6. Testing Strategy

Two independent test suites exist and both are required to pass — neither substitutes for the other:

1. **The generator's own test suite** (mcpify's Rust codebase). Covers OpenAPI parsing (local file + remote URL, JSON + YAML), directory-guard logic, auth-scheme profiling, `mcp_store.db` assembly (relational rows + vector embeddings), and — per target — golden/snapshot tests that run a target's `execute()` against fixture OpenAPI specs and assert the emitted file tree matches an expected snapshot. Written with `cargo test` (`tokio::test` for async cases) and run in mcpify's own CI on every commit; a release cannot ship with a red suite.
2. **The generated project's test suite**, emitted by `generate_setup_wizard_and_tests` and executed by `run_generated_tests` (§1, step 11) as part of every single `mcpify` invocation, not just in the generator's own CI. This is what proves a specific, real generation run — against a specific OpenAPI spec, on a specific machine — actually produced working code, rather than relying solely on the generator's own fixture-based snapshot tests (which can drift from real-world specs over time).

This closes the loop end-to-end: the generator is tested against known-good fixtures, and every real generation run is separately tested against its own actual output, so "the generator's tests pass" and "this particular generated project works" are never conflated.
