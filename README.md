# mcpify ⚡

[![CI](https://github.com/guercheLE/mcpify/actions/workflows/ci.yml/badge.svg)](https://github.com/guercheLE/mcpify/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/mcpify.svg)](https://crates.io/crates/mcpify)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Turn any OpenAPI/Swagger specification into an enterprise-grade Model Context Protocol (MCP) server in seconds.

`mcpify` is a Rust CLI generator. Point it at a Swagger 2.0, OpenAPI 3.0, or OpenAPI 3.1 spec — a local file or a remote URL, in JSON or YAML — and it emits a complete, production-ready MCP server project in the language of your choice: three token-efficient tools backed by an embedded semantic database, authentication already wired up, and enterprise capabilities (observability, resilience, testing, packaging) built in from the very first generated file.

Five target languages ship with full feature parity, each validated end-to-end in CI: **TypeScript**, **Rust**, **Python**, **C#**, and **Go**.

See [docs/product-brief.md](docs/product-brief.md), [docs/prd.md](docs/prd.md), and [docs/architecture.md](docs/architecture.md) for the full rationale and specification.

---

## Why mcpify?

Hand-building an MCP server for a large enterprise API means dropping a giant OpenAPI spec into the LLM's context, or reimplementing the same enterprise scaffolding — auth strategies, logging, tracing, resilience, tests, Docker, CI — from scratch for every new API. `mcpify` exists because that work was done twice, by hand, for two different APIs, before this generator did:

* **Token efficiency via semantic search.** Instead of exposing dozens or hundreds of raw endpoints, every generated server exposes exactly **3 universal tools**: `search`, `get`, and `call`.
* **One embedded database.** At generation time, `mcpify` parses your OpenAPI spec and writes a single self-contained `mcp_store.db` (SQLite + `sqlite-vec`) into the output directory — a relational `endpoints` table plus a vector table for semantic search. No external services required at runtime.
* **Resilient by default.** Every `call` invocation validates arguments against the input schema *before* the request and the response against the output schema *after* — catching upstream API drift before it breaks an agent.
* **Dual-role runtime.** Every generated project is both an interactive **Terminal Client** (MCP client / CLI proxy for developers) and a **Harness Server** (MCP server for agent hosts like Claude Desktop or Cursor).
* **Enterprise-grade from file one.** Structured logging with secret redaction, OpenTelemetry tracing and metrics, circuit breaker/retry/rate-limiting, health checks, OS-keychain credential storage, a generated test suite, multi-stage Docker builds, and CI/CD are part of the default template.

---

## Installation

`mcpify` is a native Rust binary — no other toolchain is required to run the generator itself. Running a *generated* project needs the runtime for whichever `--language` you chose (Node.js for TypeScript, Cargo for Rust, `uv`/Python for Python, .NET for C#, or Go).

```bash
# Via Cargo
cargo install mcpify

# Via shell installer (macOS, Linux)
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/guercheLE/mcpify/releases/latest/download/mcpify-installer.sh | sh

# Via PowerShell installer (Windows)
powershell -ExecutionPolicy Bypass -c "irm https://github.com/guercheLE/mcpify/releases/latest/download/mcpify-installer.ps1 | iex"
```

Prebuilt binaries for macOS (Intel/Apple Silicon), Linux (x86_64/arm64), and Windows are also available directly on the [Releases page](https://github.com/guercheLE/mcpify/releases).

## Quick Start

```bash
# From a remote hosted OpenAPI specification URL
mcpify -i https://developers.example.com/swagger/spec.yaml -o ./my-api-mcp

# From a local file
mcpify -i ./specs/enterprise-api.yaml -o ./my-api-mcp

# Reproducibly synchronize a multi-version project with overlays
mcpify sync --manifest ./mcpify.yaml
```

### CLI Flags

```text
Options:
  -i, --input <PATH_OR_URL>   Path or remote URL to the source OpenAPI specification (JSON/YAML)
  -o, --output <PATH>         Destination directory where the project will be generated
  -l, --language <LANG>       Target stack: "typescript" (default), "rust", "python", "csharp", or "go"
  -f, --force                 Overwrite the destination folder if it already contains files
      --publish-registry     Emit a registry-publish step in the generated release workflow
      --license <SPDX>       Package license (default: MIT)
      --repository <URL>     Source repository; required with --publish-registry
      --author <NAME>        Package author/organization (repeatable)
      --keyword <VALUE>      Package keyword (repeatable)
      --category <VALUE>     Package category (repeatable)
      --exclude <GLOB>       Package exclusion pattern (repeatable)
      --default-header <N=V> Static target-API request header (repeatable)
      --package-size-limit-mb <MB>  Maximum generated package size
      --api-version <LABEL>  Label for the spec version ingested by this run (default: "default")
  -h, --help                  Print help information
```

If the output directory is non-empty, `mcpify` aborts with a warning unless `--force` is passed.

If you expect to layer more spec versions onto this project later (see [Multi-Version OpenAPI Specs](#multi-version-openapi-specs) below), pass `--api-version` explicitly at generate time (e.g. `--api-version 11.3`) rather than relying on the default sentinel.

For repeatable generation, use a project manifest. It can select versions,
supplement incomplete auth declarations, set default request headers, run
argument-safe preprocessors, configure publication metadata, and enforce a
package-size ceiling. See [Project Manifest](docs/project-manifest.md).

---

## What Gets Generated

```text
my-api-mcp/
├── mcp_store.db          # Embedded relational + vector database
├── src/
│   ├── auth/              # One strategy per auth scheme discovered in the OpenAPI spec
│   ├── cli/                # setup, search, get, call, test-connection, version, config, help
│   ├── core/               # logger, tracing, config resolver, health-check, circuit-breaker, credential-storage
│   ├── data/                # mcp_store.db repository (relational + vector queries)
│   ├── http/                # HTTP transport
│   ├── services/            # Target API HTTP client
│   ├── tools/                 # search, get, call tool implementations
│   ├── validation/            # Input/output JSON Schema validation
│   ├── index.ts                # Harness Server entry point (stdio | http)
│   └── cli.ts                   # Terminal Client entry point
├── tests/                  # Generated unit/integration/e2e scaffold
├── Dockerfile              # Multi-stage build
├── docker-compose.yml      # stdio + http service variants
└── .github/workflows/      # CI/CD (build, test, release)
```

The tree above is the TypeScript (`-l typescript`) layout. The other four targets follow the same conceptual structure, adapted to each ecosystem's own conventions — e.g. Go's `internal/{auth,cli,core,data,http,services,tools,validation}` packages, Python's `auth/`, `cli/`, `core/`, `tools/` package under `pyproject.toml`, C#'s `Auth/`, `Cli/`, `Core/`, `Tools/` folders under a `.csproj`, and Rust's `src/{auth,cli,core,data,http,services,tools,validation}` modules under `Cargo.toml`.

### The 3 Universal Tools

1. **`search(query)`** — semantic similarity lookup against the vector table to find candidate operations for an ambiguous natural-language request.
2. **`get(operationId)`** — returns the literal schema, path, method, and documentation for a specific operation.
3. **`call(operationId, arguments)`** — validates arguments, injects the active auth strategy's credentials, executes the live request, and validates the response.

### Running the Generated Project

```bash
# Terminal Client mode (default) — direct CLI usage
my-api-mcp search "create an issue"
my-api-mcp get createIssue
my-api-mcp call createIssue --args '{"project":"ABC","summary":"..."}'

# Harness Server mode — for agent hosts
my-api-mcp start                              # stdio transport (default)
my-api-mcp http --host 127.0.0.1 --port 3000  # HTTP transport
```

### Guided Setup

```bash
my-api-mcp setup
```

Interactively collects the API URL and the credentials needed for your chosen auth strategy, then asks how to persist them:

1. Write a `.env` file
2. Write a `config.json` file
3. Print a ready-to-run, fully parameterized CLI invocation (nothing written to disk)

---

## Authentication

`mcpify` auto-discovers the auth schemes declared in the OpenAPI spec's `components.securitySchemes` and generates one strategy per scheme (Basic, API Key/PAT, OAuth1-style, OAuth2). At runtime, the operator selects **one** active strategy per deployment via the `auth_method` config value — the same simple, proven model used in production, without a runtime engine for resolving multiple simultaneous auth requirements.

When an enterprise spec is incomplete, the manifest's `auth` list supplements
the discovered schemes instead of replacing them. This is useful when a spec
declares Basic auth but the deployed product also supports a PAT.

## Configuration

Values resolve through a strict, stop-at-first-match cascade:

```text
CLI flags → env vars → local file (./) → home file (~/.<tool>/) →
system file (/etc/<tool>/) → install-dir file → built-in defaults
```

---

## Multi-Version OpenAPI Specs

Some APIs ship one spec forever (e.g. a Bamboo server); others publish a new spec per release (Jira, Bitbucket, Confluence often have 10+ historical versions). `mcpify` supports both without regenerating the project from scratch.

### Adding a version

```bash
mcpify add-version --project ./my-api-mcp --version 11.2 -i ./specs/api-v11.2.yaml
```

This ingests the spec, writes an extra, independently-queryable store (`mcp_store_v11.2.db` alongside the existing `mcp_store.db`), and re-renders only the handful of version-aware files (config, data layer, validator, setup wizard, `versions` command) — auth strategies, enterprise scaffolding, transports, and tests are untouched. For Rust/C#/Go, whose schemas are compiled into the binary, rebuild the project afterward for the new version's schemas to take effect; TypeScript/Python read schemas from disk, so no rebuild is required.

### Promoting a version to default

```bash
mcpify add-version --project ./my-api-mcp --version 11.3 -i ./specs/api-v11.3.yaml --set-default
```

`--set-default` promotes the new version to be the project's default/latest. The version it replaces is never destroyed — it's demoted to its own sibling store file (e.g. `mcp_store_v11.2.db`), so its data stays queryable under its old label.

Every version's bookkeeping (labels, file paths, which one is default) lives in a generator-only `.mcpify/versions.json` ledger inside the generated project — it's never read by the generated runtime code itself.

### Removing a version

```bash
mcpify remove-version --project ./my-api-mcp --version 11.2
```

Deletes that version's store/schema files, drops it from the ledger, and re-renders every version-aware file so the project's code, setup wizard, and `versions` command stop mentioning it. Refuses to remove the current default version — promote a different version first with `add-version --set-default`.

### Selecting a version in the generated project

When a project has more than one version, its interactive `setup` wizard prompts for which one to use (defaulting to the default/latest); a single-version project skips this prompt entirely. The generated CLI also gains a `versions` subcommand listing every version and marking the active/default one:

```bash
my-api-mcp setup      # prompts for an API version when more than one exists
my-api-mcp versions    # lists all versions, e.g. "11.3 (default, active)", "11.2", "10.7"
```

## License

Distributed under the MIT License. See `LICENSE` for more information.
