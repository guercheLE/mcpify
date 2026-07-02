# mcpify: v3 Python Target Implementation Plan

> **Status: Not started.** This plan covers adding the Python output target (`-l python`) to the mcpify generator, per `docs/architecture.md`'s "Target Language Roadmap" (§3). It assumes `docs/v1-implementation-plan.md` (and ideally `docs/v2-implementation-plan.md`) are complete — the shared pipeline, CLI, `GeneratorContext`, `McpServerTargetGenerator` trait, rollback, and `TargetRegistry` from v1 are reused as-is. This plan only covers the new, Python-specific per-target work.

## Context

Per architecture.md's rollout notes: Python follows Rust *"matching demand from AI/ML tooling."* That demand cuts both ways here: Python is also, notably, the **native home** of the embedding model both other targets have had to work around — `sentence-transformers` (or the plain `transformers` library) runs `all-mpnet-base-v2` directly, no ONNX/`fastembed`-style indirection needed. This is the one target where the embeddings decision (see below) is genuinely simpler than v1's or v2's, not just differently-shaped.

**What's identical to every other target, and does not need to be re-planned:** the shared 4-step pipeline (ingest, directory guard, auth profiling, `mcp_store.db` assembly) is target-agnostic and already built. `-l python` only needs a new `PythonTargetGenerator` implementing the 6 per-target trait methods and registering itself in `targets::build_registry()`.

## Toolchain (architecture.md §3)

| Concern | Choice |
| --- | --- |
| Async runtime | `asyncio` |
| MCP SDK | `mcp` (the official Python SDK) — confirm current version/API shape on PyPI at implementation time |
| DB driver + vector ext. | `sqlite3` (stdlib) or `aiosqlite` (if the async ecosystem benefits enough to justify the extra dependency) + `sqlite-vec` |
| HTTP client (outbound) | `httpx` (async-native, unlike `requests`) |
| HTTP/transport server | `fastapi` + `uvicorn` |
| Schema validation | `jsonschema` package, or `pydantic` if models are built from schemas rather than validated against them directly — pick one and be consistent, don't mix both for the same job |
| Structured logging | `structlog` (JSON) |
| Tracing/metrics | `opentelemetry-sdk` (+ the Jaeger/OTLP and Prometheus exporter packages) |
| Generated test tooling | `pytest` (+ `pytest-asyncio` for the async test suite) |
| CLI invocation of output | `python main.py --server` (inside a project-managed venv) |

## Open decisions to resolve during implementation

1. **Packaging tool.** `pyproject.toml` is a given; the build backend (`setuptools`, `hatchling`, `poetry-core`) and dependency/venv workflow (`pip` + `venv`, `poetry`, `uv`) is an open choice. `uv` is worth serious consideration given its speed and growing adoption — but pick whichever keeps `run_generated_tests`' "install then test" step simplest to shell out to and reason about failures from.
2. **`jsonschema` vs `pydantic` for validation.** `jsonschema` validates arbitrary JSON Schema documents directly — closer to v1's Ajv-based design (schemas are data, generated once via `serde_json`/Python-`json`, not re-derived as code). `pydantic` would mean *generating Python model classes* from each operation's schema, a meaningfully bigger code-generation surface (and a second schema representation to keep in sync with `mcp_store.db`'s stored schemas). Recommend `jsonschema` for parity with v1's design and to avoid that duplication, unless FastAPI's own request/response typing conventions make `pydantic` models a hard requirement for the HTTP transport — investigate this specifically before committing.
3. **Embeddings — the easy case.** Use `sentence-transformers` (or `transformers` directly) with `all-mpnet-base-v2` — the same model family v1 and v2 target, run natively without an ONNX detour. Since the *tool* (`search`) and the *populate script* are both Python here, the "one code path, both callers" principle from v1 is trivially satisfied by importing the same embedding module from both places — no cross-language indirection to design around at all. This is worth calling out precisely because it's simpler than the other three remaining targets, not despite it.
4. **OAuth1 RSA-SHA1 signing.** `requests-oauthlib`/`oauthlib`, or hand-rolled via the `cryptography` package directly (mirrors v1's and v2's approach of composing a signature base string and signing it, rather than depending on a full OAuth client library). Pick based on how much of the PKCE/OAuth2 flow (also needed) the same library covers, to avoid pulling in two different OAuth libraries for two different auth kinds.

## Story breakdown

Target-local numbering (P1, P2, ...), mirroring v1's Story 7→14 shape.

---

### P1 — Target scaffolding & template engine
**Goal:** `src/targets/python/mod.rs`: `PythonTargetGenerator`, `name() -> "python"`, 6 stubbed methods. `PyTemplateContext` (mirrors `TsTemplateContext`/`RsTemplateContext`). `naming.rs` for Python's conventions (`snake_case` modules/functions, `PascalCase` classes — closer to Rust's two-case system than TS's four-case one). Own embedded `templates/` + `tera`/`rust-embed` render/emit pair, same pattern as the other two targets.

**Depends on:** v1 Stories 0–6 (reused).

---

### P2 — `bootstrap_project`
**Goal:** `pyproject.toml.tera` (package metadata, the dependency list from the toolchain table, entry-point script registration for the CLI), project skeleton (`src/<package>/{auth,cli,core,data,http,services,tools,validation}/`, each an `__init__.py`-bearing package), `.gitignore`, `README.md`.

**Depends on:** P1.

---

### P3 — `generate_enterprise_scaffolding`
**Goal:** `logger.py` (`structlog` configuration with redaction processors — Python's structlog has first-class support for this via processor chains, arguably the cleanest of any target's logging setup), `tracing.py` (`opentelemetry-sdk` wiring), `config.py` (the same 7-tier REQ-2.2 cascade, using `pydantic-settings` or plain `os.environ`/`yaml` — decide alongside open decision #2 since a `pydantic`-based config doesn't imply `pydantic`-based schema validation, they're separable choices), `circuit_breaker.py`, `credential_storage.py` (the `keyring` package is Python's direct equivalent of Node's `keytar` — same OS-keychain/encrypted-file-fallback shape), `health_check.py`, `rate_limiter.py`, `cache.py`, `mcp_server.py` (wraps the `mcp` SDK), plus `Dockerfile.tera` (a `python:X-slim` base, `pip install`/`uv sync` layer caching), `docker-compose.yml.tera`, and the three GitHub Actions workflow templates (`ruff`/`black --check` replacing `lint`/`format:check`, `pytest` replacing `npm test`).

**Depends on:** P2.

---

### P4 — `generate_auth_strategies`
**Goal:** Same 5 strategies as every other target (Basic, PAT, OAuth1 RSA-SHA1, OAuth2 PKCE+refresh, stub), expressed as a Python `Protocol` or `ABC` for the shared `AuthStrategy` shape, and an `auth_manager.py` dispatch dict keyed by the discovered `auth_method` (Python's `dict`-of-classes is the natural equivalent of v1's TS object-literal lookup and v2's Rust `match`).

**Depends on:** P3.

---

### P5 — `generate_transports_and_roles`
**Goal:** Dual-role entry point (a `click` or `typer` CLI — either is a defensible, well-maintained choice; pick one and use it consistently for both the Terminal Client subcommands and any wizard-adjacent prompting) dispatching between the Terminal Client and Harness Server. `fastapi`+`uvicorn` HTTP transport, translating v1's `localhost-detector`/`auth-extractor`/`metrics` into FastAPI dependencies/middleware idiomatically.

**Depends on:** P3, P4.

---

### P6 — `generate_mcp_tools`
**Goal:** `search`/`get`/`call`, a `sqlite3`/`aiosqlite` + `sqlite-vec` data-access module (Python's `sqlite3` stdlib module supports loading extensions via `enable_load_extension`/`load_extension` — confirm `sqlite-vec`'s Python-loadable artifact/wheel packaging at implementation time, since this is the one target where the vector-search extension isn't loaded through a dedicated first-party binding the way `better-sqlite3`'s npm package or `rusqlite`'s Cargo feature handle it), an `httpx`-based API client (generic operation dispatcher, parameter locations read from the resolved schema — same design as every other target), `jsonschema`-based validation against the same kind of co-located JSON asset the other targets use, and the embedding module from open decision #3 — imported by both the `search` tool and the populate-embeddings script/module.

**Depends on:** P3, P4, P5.

---

### P7 — `generate_setup_wizard_and_tests`
**Goal:** Interactive setup wizard (via whichever CLI library P5 chose — `click`/`typer` both have prompt helpers, or pair with `questionary` for richer prompts closer to Node's `inquirer`), and the generated `pytest` suite (`pytest-asyncio` for the async paths), conditionally emitting auth-strategy tests only for discovered schemes — same hard requirement as every other target: an emitted test importing an undiscovered strategy module breaks `run_generated_tests`, not just the test itself.

**Depends on:** P3, P4, P5, P6.

---

### P8 — `run_generated_tests` + registration
**Goal:** Shell out to the chosen packaging tool's install step (`pip install -e .`/`uv sync`) then `pytest`. Unlike v1 (no separate build/compile step needed because vitest's transform IS the type-check-adjacent step) and v2 (`cargo test` compiles as part of running), Python has no compile step at all — `pytest` collecting and running the suite is already the full proof of "the code at least imports and runs," which is the same "one signal, not two" principle restated for a dynamically-typed target. If the embeddings library needs a model download on first use, sequence that the same way v1 sequenced `populate-embeddings` before `npm test`. Register `PythonTargetGenerator` in `targets::build_registry()` only once this is real and green.

**Depends on:** P7. **Treat this as the v3 launch milestone.**

---

### P9 — Golden/snapshot tests + CI additions
**Goal:** `tests/golden_python.rs`, same file-tree + curated-content-snapshot pattern as the other two targets, reusing the already-shared OpenAPI fixtures. Extend `.github/workflows/ci.yml`'s fast job to install Python (`actions/setup-python`) alongside the existing Rust/Node toolchains, and add a slow-job step for a Python-target `e2e_generation`-equivalent test.

**Depends on:** P8.

## Sequencing

P1 → P2 → P3 → P4 → (P5 ‖ P6 once P4 lands) → P7 → P8 → P9. Resolve open decisions #1 and #2 (packaging tool, validation library) before P2/P6 respectively, since both are foundational choices other stories build directly on top of — changing them mid-implementation means re-touching most files already written.

## Verification

Same shape as the other targets: per-story `cargo test` on mcpify's own suite, golden/snapshot regression tests, and the real gate — an `#[ignore]`-by-default Rust test running `PythonTargetGenerator::execute()` against a fixture spec, asserting the generated project's own `pytest` run passes for real. Also manually verify a semantic-search query against a tiny fixture returns sane, correctly-ordered results once during P6/P8 development.
