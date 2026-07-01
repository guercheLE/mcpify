# Product Requirements Document (PRD): mcpify

## 1. Functional Requirements

### 1.1 CLI Guardrails & Target Setup

* **REQ-1.1.1:** The CLI must accept an OpenAPI source locator via `-i`/`--input`, supporting both a local file path (JSON or YAML) and a remote `http`/`https` URL (e.g. `mcpify -i https://developers.example.com/swagger/spec.yaml -o ./my-mcp`).
* **REQ-1.1.2:** The CLI must accept an output directory via `-o`/`--output`.
* **REQ-1.1.3:** If the output directory exists and is non-empty, the tool must abort with a warning and instruct the user to pass `--force` to overwrite.
* **REQ-1.1.4:** The generator itself is implemented in Rust and distributed as a single native binary (`cargo install mcpify`, with a Homebrew tap planned). The `-l`/`--language` flag selects the output stack; only `typescript` is implemented in v1, and passing an unimplemented value (`rust`, `python`, `csharp`, `go`) is rejected with a clear "not yet supported" message pointing at the roadmap.
* **REQ-1.1.5:** `typescript`, `rust`, `python`, `csharp`, and `go` are all in scope as output targets for mcpify, implemented in that priority order across releases (v1 = TypeScript; v2 = Rust; v3 = Python; v4 = C#; v5 = Go). Every target must satisfy the same `McpServerTargetGenerator` trait (§ architecture.md) and reach feature parity with the v1 TypeScript output — 3 tools, dual client/server role, single-strategy auth, config cascade, and the full enterprise-first bar (§2.3) — before it ships. See `architecture.md` § "Target Language Roadmap" for the concrete toolchain planned per language.

### 1.2 Authentication Autodiscovery & Strategy Generation

* **REQ-1.2.1:** The tool must inspect the OpenAPI `components.securitySchemes` object to identify every auth scheme the API declares.
* **REQ-1.2.2:** For each distinct scheme found (Basic, API Key, OAuth2, OAuth1-style extensions, Bearer/PAT), the generator emits one auth strategy module, mirroring the strategy-pattern approach already proven in `bitbucket-dc-mcp`/`jira-dc-mcp` (`src/auth/strategies/*`).
* **REQ-1.2.3:** At runtime, the generated server selects exactly **one** active strategy per deployment, chosen via the `auth_method` configuration value. There is no runtime engine for resolving AND/OR combinations of multiple simultaneously-required schemes — this was evaluated and intentionally dropped from scope, since neither reference implementation ever required it in production.
* **REQ-1.2.4:** If `components.securitySchemes` is missing or ambiguous, the generator falls back to an interactive CLI prompt asking the user to pick/describe the auth mechanism, rather than silently guessing.

### 1.3 Dual Runtime Topology (Client + Server)

The generated project ships a single package that can run in two distinct modes:

* **Mode A — Terminal Client (default):** Runs as an interactive MCP client / CLI proxy. Exposes `search`, `get`, and `call` as direct CLI subcommands (plus `setup`, `test-connection`, `version`, `config`, `help`) that a developer can invoke directly from a shell, in addition to acting as a genuine MCP client capable of connecting outward when needed.
* **Mode B — Harness Server (`--server` / `start`):** Boots a persistent MCP server loop that exposes the same three tools over a transport, for consumption by agent harnesses (Claude Desktop, Cursor, custom orchestrators).

### 1.4 Transport Configuration

When running in Server mode, the generated tool supports two transports, selectable and parameterized via the standard config cascade (§2.2):

* **`stdio`** (default): JSON-RPC over standard input/output.
* **`http`**: HTTP transport with configurable `--port`, `--host`, and CORS origin (`--cors-allow`), including the localhost-vs-network binding distinction proven useful in the reference servers (looser auth requirements when bound to localhost only).

### 1.5 Core Capabilities — The 3 Universal Tools

| Tool | Responsibility | Backed by |
| --- | --- | --- |
| `search` | Semantic similarity lookup to match a natural-language query against candidate API operations. | Virtual vector table (`vec0` via `sqlite-vec`) inside `mcp_store.db`. |
| `get` | Returns the literal schema, path, method, and documentation for a specific `operationId`. | Relational `endpoints` table inside `mcp_store.db`. |
| `call` | Validates arguments against the input schema, injects the active auth strategy's credentials, executes the live HTTP request, and validates the response against the output schema before returning it. | Live async HTTP client. |

Tool naming (`search`/`get`/`call` as designed, vs. the `search_ids`/`get_id`/`call_id` naming used in the two reference servers) is an open naming decision to resolve during implementation — functionally equivalent either way.

### 1.6 Guided Setup Wizard

* **REQ-1.6.1:** The generated project ships a `setup` CLI command that interactively prompts for every parameter the chosen auth strategy and API base URL require.
* **REQ-1.6.2:** At the end of the wizard, the user is asked how to persist the collected values, with three mutually exclusive outputs:
  1. Write a `.env` file.
  2. Write a `config.json` file.
  3. Print a ready-to-run, fully parameterized CLI invocation (e.g. `jira-dc --username ... --password ...`) without writing anything to disk.
* **REQ-1.6.3:** These three options map directly onto the top three tiers of the configuration cascade (§2.2) — the wizard is the single entry point for populating any of them, so the operator never has to hand-edit config files to get started.

## 2. Non-Functional Requirements

### 2.1 Asynchronous Runtime

* **REQ-2.1.1:** The generator (Rust) uses `tokio` as its async runtime throughout — spec ingestion (including remote HTTP fetches), directory checks, database assembly, and template synthesis are all non-blocking.
* **REQ-2.1.2:** All generated TypeScript/Node.js code uses Node.js's native async/event-loop model throughout (no synchronous blocking I/O in the request path).

### 2.2 Configuration Resolution Cascade

All runtime configuration values (API URL, credentials, transport settings, etc.) resolve via a strict, stop-at-first-match priority chain:

1. CLI flags / direct tool-call parameters.
2. Environment variables.
3. Local working-directory config file (`./<tool>.config.yml` or `./.env`).
4. User home-directory config file (`~/.<tool>/config.yml`).
5. System-wide config file (`/etc/<tool>/config.yml`).
6. Tool installation-directory config file (co-located with the installed binary/package).
7. Built-in defaults.

This reconciles the 4-tier cascade originally sketched (CLI → env → local file → install-dir file) with the tiers actually needed in production (which additionally include a home-directory and a system-wide file) into one unambiguous chain.

### 2.3 Embedded Structural Quality ("Enterprise-First")

Every one of the following is present in the generated project from the very first file mcpify writes — new tool files are never revisited later to retrofit these capabilities:

* **REQ-2.3.1 Logging:** Structured JSON logging with automatic redaction of sensitive fields (password, token, secret, etc.).
* **REQ-2.3.2 Observability:** OpenTelemetry instrumentation exporting traces (Jaeger-compatible) and metrics (Prometheus-compatible) — request counts/durations, MCP operation counts/durations, auth attempt/failure counts.
* **REQ-2.3.3 Resilience:** Circuit breaker, retry with backoff, and rate limiting around outbound calls to the target API, all configurable.
* **REQ-2.3.4 Health checks:** A component health registry distinguishing critical vs. optional components, supporting degraded-mode startup and a `/healthz`-style check plus a `test-connection` CLI command.
* **REQ-2.3.5 Credential storage:** Secrets resolved through the config cascade are persisted, when the operator opts in, to the OS-native credential store (macOS Keychain, Windows Credential Manager, Linux Secret Service) with an encrypted-file fallback and in-memory caching.
* **REQ-2.3.6 Testing:** A generated test suite (unit + integration + e2e) using the target language's proven tooling (e.g. Vitest for TypeScript, matching the reference servers), covering the 3 tools, the config resolver, and the active auth strategy at minimum. These tests are not merely scaffolded — mcpify installs dependencies and **runs them to completion as the final step of every generation**, and a generation run that produces code whose tests fail (or don't run) is not considered successful. There is no separate "does it build" check: a target's test runner cannot execute tests against code that fails to compile/type-check, so a passing test run is the single, sufficient proof of both build correctness and functional correctness (see `architecture.md` §1, `run_generated_tests`).
* **REQ-2.3.7 Packaging & delivery:** A multi-stage Dockerfile, a `docker-compose.yml` covering both stdio and HTTP modes, and a CI/CD pipeline (GitHub Actions) with automated versioning/publishing (semantic-release), matching the delivery pattern already proven by both reference servers.

### 2.4 Data Storage

* **REQ-2.4.1:** All generated structural and semantic data lives in a single embedded SQLite database, `mcp_store.db`, created and pre-populated at generation time — one relational `endpoints` table (path, method, summary, description, input/output JSON Schema, resolved auth scheme reference) and one virtual vector table `semantic_endpoints` (via the `sqlite-vec` extension) holding the embeddings used by `search`. This is a deliberate simplification relative to the three-file split (`embeddings.db` + `operations.json` + `schemas.json`) used in the two reference servers, favoring a single self-contained artifact per generated project.

### 2.5 Quality Bar

* **REQ-2.5.1:** Zero-placeholder generation — a freshly generated project must pass its own generated test suite, unedited, immediately after generation (see REQ-2.3.6). Passing tests is the acceptance signal; there is no separate manual or automated "does it compile" check to maintain.
* **REQ-2.5.2:** Feature parity with the reference servers on every axis in this document (auth, transports, observability, resilience, testing, packaging) is the acceptance bar for v1.

### 2.6 Generator Test Coverage

* **REQ-2.6.1:** The mcpify generator itself (the Rust codebase) has its own automated test suite (`cargo test`), independent of and in addition to the generated-project tests in REQ-2.3.6 — covering OpenAPI ingestion (local + remote, JSON + YAML), directory-guard logic, auth-scheme profiling, `mcp_store.db` assembly, and, per output-language target, golden/snapshot tests asserting `execute()` against fixture specs produces the expected file tree.
* **REQ-2.6.2:** The generator's test suite runs in CI on every commit and must pass before merge; no release of mcpify ships with a failing suite.
* **REQ-2.6.3:** These two test suites are complementary, not redundant: REQ-2.6.1 proves the generator behaves correctly against known fixtures; REQ-2.3.6/REQ-2.5.1 prove that a specific real generation run, against a specific real OpenAPI spec, actually produced working code.
