# mcpify ⚡

Turn any OpenAPI/Swagger specification into an enterprise-grade Model Context Protocol (MCP) server in seconds.

`mcpify` is a Rust CLI generator that emits TypeScript-first MCP projects. Point it at an OpenAPI spec — a local file or a remote URL — and it emits a complete, production-ready Node.js/TypeScript MCP project: three token-efficient tools backed by an embedded semantic database, authentication already wired up, and enterprise capabilities (observability, resilience, testing, packaging) built in from the very first generated file.

See [product-brief.md](product-brief.md), [prd.md](prd.md), and [architecture.md](architecture.md) for the full rationale and specification.

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

`mcpify` is a native Rust binary — no Node.js required to run the generator itself (Node.js is only needed to run the *projects it generates*).

```bash
# Via Cargo
cargo install mcpify

# Via Homebrew (coming soon)
brew install guercheLE/tap/mcpify
```

## Quick Start

```bash
# From a remote hosted OpenAPI specification URL
mcpify -i https://developers.example.com/swagger/spec.yaml -o ./my-api-mcp

# From a local file
mcpify -i ./specs/enterprise-api.yaml -o ./my-api-mcp
```

### CLI Flags

```text
Options:
  -i, --input <PATH_OR_URL>  Path or remote URL to the source OpenAPI specification (JSON/YAML)
  -o, --output <PATH>        Destination directory where the project will be generated
  -l, --language <LANG>      Target stack (v1: "typescript" only; reserved for future targets)
  -f, --force                Overwrite the destination folder if it already contains files
  -h, --help                 Print help information
```

If the output directory is non-empty, `mcpify` aborts with a warning unless `--force` is passed.

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

### The 3 Universal Tools

1. **`search(query)`** — semantic similarity lookup against the vector table to find candidate operations for an ambiguous natural-language request.
2. **`get(operationId)`** — returns the literal schema, path, method, and documentation for a specific operation.
3. **`call(operationId, arguments)`** — validates arguments, injects the active auth strategy's credentials, executes the live request, and validates the response.

### Running the Generated Project

```bash
# Terminal Client mode (default) — direct CLI usage
my-api-mcp search "create an issue"
my-api-mcp get createIssue
my-api-mcp call createIssue --project ABC --summary "..."

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

## Configuration

Values resolve through a strict, stop-at-first-match cascade:

```text
CLI flags → env vars → local file (./) → home file (~/.<tool>/) →
system file (/etc/<tool>/) → install-dir file → built-in defaults
```

## License

Distributed under the MIT License. See `LICENSE` for more information.
