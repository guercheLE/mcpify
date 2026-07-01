# Product Brief: mcpify

## Executive Summary

Modern AI development relies heavily on the Model Context Protocol (MCP) to bridge Large Language Models (LLMs) with external tools and enterprise APIs. Converting a large, enterprise-scale REST API (described via OpenAPI/Swagger) into a fully compliant, production-grade MCP server is currently a manual, weeks-long engineering effort — one that has to be repeated, nearly from scratch, for every new API.

**mcpify** is a Rust CLI generator that turns any OpenAPI (JSON/YAML) specification — local file or remote URL — into a deployment-ready, enterprise-grade MCP server project. The generator itself is a fast, single-binary Rust tool; the projects it emits are TypeScript-first in v1. Given an API spec, mcpify emits a complete Node.js/TypeScript workspace exposing exactly three universal tools (`search`, `get`, `call`) backed by a local semantic database, with authentication, observability, resilience, testing, containerization, and CI/CD already wired in — not as an afterthought, but baked into the very first file mcpify writes.

## Core Problem Statement

This brief is grounded in a concrete, lived case: the same author independently hand-built two MCP servers — `bitbucket-dc-mcp` (Bitbucket Data Center) and `jira-dc-mcp` (Jira Data Center) — using the BMAD methodology. Both are excellent, production-ready TypeScript servers. And both took real, duplicated effort:

* **Duplicated engineering.** Every enterprise capability — structured logging, OpenTelemetry tracing/metrics, circuit breakers, health checks, OS-keychain credential storage, the semantic search database, the CI/CD pipeline, the Docker packaging — was designed and implemented twice, once per API, because no shared generator existed. The second repo largely re-derived the architecture of the first by hand.
* **High friction translating OpenAPI to MCP.** Both APIs ship large OpenAPI specs; turning hundreds of operations into a token-efficient, LLM-friendly tool surface (rather than dumping the whole spec into context) required custom tooling (embedding generation, `sqlite-vec` population, schema extraction) built from scratch each time.
* **Brittle call execution without contract enforcement.** Without automated input/output schema validation around every live API call, upstream API drift can silently break agent workflows.
* **Inconsistent auth handling across APIs.** Each API exposes a different mix of auth schemes (Basic, PAT, OAuth1, OAuth2); wiring each one — including token refresh and secure OS-native credential storage — was rebuilt independently for each server.

## Target Audience

* **AI Solutions Architects & Engineers** building multi-agent systems that need live access to enterprise infrastructure (ITSM, VCS, issue trackers, internal platforms).
* **Enterprise DevSecOps Teams** who need to expose existing service portfolios safely to AI systems (Claude Desktop, Cursor, custom agent harnesses) without hand-rolling a new MCP server for every internal API.

## Key Pillars & Value Proposition

1. **Rust generator, five output languages on a staged roadmap.** The generator itself is written in Rust — a single, dependency-free binary, distributed independently of the ecosystems it targets. Rather than shipping all five output languages at once, mcpify proves the model once (TypeScript, v1 — the exact shape of server already validated in production by `bitbucket-dc-mcp` and `jira-dc-mcp`) and then rolls out the remaining targets in priority order: **Rust** (v2), **Python** (v3), **C#** (v4), **Go** (v5). Every target implements the same Strategy Pattern trait and must reach feature parity with the TypeScript output before it ships — see `architecture.md` § "Target Language Roadmap" for the per-language toolchain.
2. **Dual-role by design.** Every generated project is both an interactive **MCP Client (terminal proxy)** for local developers and an automated **MCP Server (harness proxy)** for agent hosts — a single binary/package serving two operational runtimes, selectable via CLI flags/subcommands.
3. **Token-efficient by construction.** Exactly 3 universal tools (`search`, `get`, `call`) backed by a single embedded `mcp_store.db` (SQLite + `sqlite-vec`), so LLMs never need the full OpenAPI spec in context.
4. **Resilient contract enforcement.** Every `call` invocation validates input and output against the operation's JSON Schema before and after the live HTTP request.
5. **Enterprise-grade from the first generated file.** Structured logging with secret redaction, OpenTelemetry tracing and metrics, circuit breaker/retry/rate-limiting, health checks, OS-keychain credential storage, a generated test suite, multi-stage Docker builds, and a CI/CD pipeline are part of the default template — not bolt-ons added after the fact. This directly closes the gap observed in the two hand-built reference servers, where these capabilities existed but were expensive to build and impossible to reuse.
6. **Flexible auth, simply resolved.** mcpify auto-discovers the auth schemes declared in `components.securitySchemes`, generates one strategy per scheme (mirroring the proven Basic/PAT/OAuth1/OAuth2 pattern), and lets the operator select a single active strategy per deployment via config — the same simple model validated in production by both reference servers, without the added complexity of a runtime AND/OR policy engine that neither server ever needed.
7. **Guided setup.** A generated `setup` wizard interactively collects the parameters a deployment needs (API URL, chosen auth scheme, credentials) and, at the end, lets the operator choose how to persist them: as a `.env` file, a `config.json` file, or simply as a ready-to-run parameterized CLI invocation — mapping directly onto the top three tiers of the configuration cascade.

## Success Criteria

* Generating a fully working TypeScript workspace from an OpenAPI spec (local file or remote URL) completes in seconds.
* Generated code contains **zero structural placeholders** — proven not by assumption but because mcpify runs the generated project's own test suite as the last step of every generation, and a run isn't successful unless those tests pass. The three tools, the chosen auth strategy, logging, tracing/metrics, circuit breaker, health checks, and CI/Docker packaging all work on first generation, matching the quality bar already achieved by hand in `bitbucket-dc-mcp` and `jira-dc-mcp`, without the manual effort. The generator itself carries its own independent test suite, run in CI on every commit.
* A new internal API can go from "OpenAPI spec in hand" to "MCP server running in Claude Desktop / Cursor" without writing custom integration code.
