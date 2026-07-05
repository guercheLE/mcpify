# HTTP transport: source credentials from request headers, not config

## Context

mcpify generates MCP servers (5 targets: rust, python, typescript, go, csharp) that always support both stdio and HTTP transport — transport is a runtime choice, never a generation-time one. Before this change, **regardless of transport**, the active auth strategy's real secret material (username/password, API key, PAT, OAuth2 token, OAuth1 consumer key/secret) was resolved by the 7-tier config cascade (CLI flags → env vars → local/home/system/install-dir config files → built-in defaults) plus OS-keychain/encrypted-file storage, and held by a process-wide `AuthManager` that decorated every outbound call to the facaded API.

Separately, HTTP transport already had an auth-gate middleware requiring an `Authorization` header from non-localhost callers — but that was only a presence check gating access to the MCP server itself. It was disconnected from the outbound credentials: whatever the server was configured with locally (`.env`, keychain, etc.) is what actually got forwarded to the backend API, no matter who called it or what they sent.

This was wrong for HTTP deployments: a server exposed over HTTP (localhost or remote) may be called by different MCP clients (Claude Code, Claude Desktop, custom agents) who each have their own identity/credentials for the backend API. The fix: **when transport is HTTP, credentials come only from each incoming request's headers and are forwarded per-request to the facaded API; local config/.env/keychain never supplies the actual secret for that path.** Non-credential config (backend URL, host, port, log level, `auth_method` itself as a discriminant) is unaffected and keeps using the existing cascade in both transports.

Scoping decisions made up front, confirmed with the user before implementation:
- **Scope**: only credential material is affected, not general config.
- **OAuth1 is out of scope for HTTP**: OAuth1 signs each request against the destination URL using a secret the caller would have to hand over in full (consumer secret, token secret) — a generated server refuses to start in HTTP mode if the active `auth_method` is OAuth1, with a clear error pointing at stdio instead.
- **Per-request scope is required**: concurrent HTTP requests with different headers must not cross-contaminate credentials — this replaces the singleton-holds-the-credentials model with per-request extraction on the HTTP path only (stdio is untouched).
- **The bundled CLI** (`cli/call.*.tera`, `get`, `search`) is a direct local API proxy — it never speaks MCP protocol or connects to `/mcp` at all; it just calls `AuthManager` + the backend directly using local config. It needed no code changes.
- **Localhost bypass** stays for the auth *gate* (still no 401 for `127.0.0.1` without a header — preserves quick local-testing ergonomics), but the *credential-forwarding* path requires a per-request override on HTTP transport unconditionally — a localhost caller with no header gets a clear "no credentials for HTTP transport" error on `call`, never a silent fallback to local/keychain secrets. This closes the actual leak: previously a localhost curl with no header would still succeed by leaking the operator's configured secret to the backend.
- The setup wizard previously generated zero guidance for how an external MCP client (or a curl test) should connect. This plan adds it, tied to whichever transport the operator actually selects.

## Design and outcome, by area

### 1. Header/query/cookie location metadata (`src/auth_profile`)

`AuthSchemeDescriptor` only carried `{ name, kind }` — no `in`/`name` (header vs. query vs. cookie, and the header's name), even though `openapiv3::SecurityScheme::APIKey` already exposes `location`/`name`. Added `AuthSchemeLocation { Header { name }, Query { name }, Cookie { name } }` and a `location: Option<AuthSchemeLocation>` field on `AuthSchemeDescriptor`, populated in `classify.rs::classify_one`: `APIKey` uses its declared location/name; HTTP basic/bearer and OAuth2 default to `Header("Authorization")`; OAuth1 gets `None` (unused, per the out-of-scope decision). `prompt.rs`'s interactive fallback path uses a new `default_location_for()` helper for the same defaulting.

This metadata is what every downstream language uses to know which header (or query/cookie param) a given auth scheme actually expects, both for extracting it from an incoming request and for the setup wizard's example client config.

### 2. Template context plumbing (`src/targets/*/context.rs`)

Each target's `*AuthSchemeView` struct gained `header_location`/`header_name` fields, and the deduped `*AuthMethodView` (the per-kind lookup used when multiple schemes collapse to one at generation time) mirrors them via a `first_of_kind` lookup. TypeScript needed a new `TsAuthMethodLocationView` + `auth_method_locations` field, since TypeScript has no per-kind named-enum view struct the other four languages have. Mechanical, near-identical edit across all five targets — no generated behavior changed yet at this point.

### 3. New "request credentials" type + shared-files registration (per language)

Added a small type representing "credentials extracted from one incoming HTTP request" — e.g. Rust's `RequestCredentials` enum (`Basic(String) | Bearer(String) | ApiKey{header_name, value}`) — with a conversion into the existing `Credentials` map shape (`authorization_header`/`api_key` keys) so the pre-existing header-building tail logic in `apply_auth_headers` is reused unchanged, not duplicated. One new template file per language (`auth/request_credentials.rs.tera`, `auth/request_credentials.py.tera`, `auth/request-credentials.ts.tera`, `internal/auth/requestcredentials.go.tera`, `Auth/RequestCredentials.cs.tera`), added to each target's `steps/auth.rs` unconditional shared-files list (like `errors.*.tera` today), since the type is needed regardless of which specific schemes were discovered.

### 4. `apply_auth_headers` refactor (one call site, all five languages)

Each language's `auth_manager.*`/`AuthManager.*` gained two parameters on the existing header-building method: the current transport (already resolved by config) and an optional per-request override. Body, same shape in all five:
- OAuth1 guard first (bail if transport is HTTP).
- `(Http, Some(override))` → build the credentials map from the override, skip config/keychain entirely.
- `(Http, None)` → a clear "missing credentials" error — never fall back to config/keychain on HTTP.
- `(Stdio, _)` → unchanged, resolves from config/keychain as before.

Stdio call sites (`cli/call.*.tera`) pass `Transport::Stdio, None` and are otherwise untouched.

### 5. Per-request credential propagation — the part with no shortcut

Each `http/auth_extractor.*` was extended from "check `Authorization` is present" into "extract the right header for the API's active `auth_method`," reused by both the gate and the credential path. Threading the extracted value from the request-handling entry point down to the outbound API call needed a different, empirically verified mechanism per language, because each MCP SDK decouples session/worker handling from the originating HTTP request differently:

- **Rust (rmcp 2.1.0)**: `RequestContext::extensions`, populated fresh per-message from `http::request::Parts`. A `tokio::task_local!` approach was tried first and confirmed **not** to work — rmcp spawns a decoupled session worker task (`WorkerTransport::spawn`), so task-local state set in the Axum middleware never reaches the tool-call handler. Root-caused by reading rmcp's own source; `extensions` is the SDK's documented mechanism for this.
- **Python (`mcp` SDK)**: `Context.request_context.request` (a Starlette `Request`), populated per-message via `ServerMessageMetadata.request_context`. This field only exists from `mcp>=1.12`; `pyproject.toml.tera`'s constraint was bumped from `mcp>=1.0`.
- **TypeScript (`@modelcontextprotocol/sdk`)**: `extra.requestInfo.headers` in the tool callback's second parameter, populated fresh per-message inside the SDK's `webStandardStreamableHttp.js`.
- **Go (`mark3labs/mcp-go`)**: `server.WithHTTPContextFunc(...)`, the SDK's own sanctioned per-request (not per-session) context-injection hook, read back via `ctx.Value(...)`.
- **C# (`ModelContextProtocol`/ASP.NET Core)**: `IHttpContextAccessor`, backed by ASP.NET Core's standard per-request DI scope — this one needed no workaround, since `MapMcp()` is ordinary ASP.NET Core middleware routing, unlike the custom session/worker models the other four SDKs use internally.

### 6. Auth-gate: keep localhost bypass for the gate, but always attempt extraction

Non-localhost requests: the gate now does real per-scheme validation (missing/malformed header → 401), using the extractor from §5. Localhost requests: the gate still lets the request through with no header, but extraction still runs — if nothing is found, the per-request override passed into `apply_auth_headers` is `None`, which (per §4) means any `call` tool invocation fails with a clear error rather than silently reaching the backend with the operator's local secret. "Bypass the gate" and "have credentials to forward" are now two independent facts, closing the original leak.

### 7. OAuth1 + HTTP: fail fast at startup

Right after the config cascade resolves `transport` and `auth_method`, each language's bootstrap (`main.rs.tera`, `cli/__init__.py.tera`, `index.ts.tera`, `internal/cli/roles.go.tera`, `Cli/Roles.cs.tera`) checks: if `transport == Http && auth_method == OAuth1`, exit with a message explaining OAuth1 needs stdio (its signature must be recomputed per destination URL, incompatible with a simple header relay). This is dead code (compiled out via `{% if "oauth1" in auth_method_keys %}`) for projects that never discovered an OAuth1 scheme.

### 8. Config cascade: no changes

`auth_method` stayed a cascade-resolved discriminant in both transports (both the gate and `AuthManager::new` need to know which scheme is active). The actual behavior change lives entirely in `apply_auth_headers` (§4): the method that touches env/file/keychain for the secret is simply never called on the HTTP path once a transport+override pair is threaded through. `config_manager.rs.tera`/`config.py.tera`/`config-manager.ts.tera`/`config.go.tera`/`Config.cs.tera` are untouched by this feature (one unrelated pre-existing test-fixture bug in `config_manager.rs.tera` was fixed along the way — see Outcome).

### 9. Setup wizard: transport prompt + matching MCP client config snippet

Added a `prompt_transport()` step (stdio vs. http) to every `cli/setup_wizard.*`, persisted alongside the existing env/config output. Based on that selection, the wizard now prints **only the matching** client config entry, following explicit correction from the user through several iterations (first drafted as HTTP-only guidance, then briefly as both transports together, before landing on "only the transport actually selected, shown once"):
- **stdio selected** → the standard `"command"/"args"` MCP client entry, illustrating both ways a client can supply credentials the config cascade still accepts for stdio: an `env` block, and the equivalent all-CLI-args invocation printed alongside it.
- **http selected** → `{"mcpServers": {"<project>": {"url": "...", "headers": {"<header-name>": "<example>"}}}}`, reusing the §1/§5 header-name metadata so the example never drifts from what the server actually expects — headers are the only credential input shown, consistent with HTTP mode never reading env/config/CLI for secrets. If the active `auth_method` is OAuth1, the wizard refuses this combination too (consistent with §7), printing the same "use stdio for OAuth1" explanation instead of an unusable http snippet.

## Pre-existing bugs found and fixed along the way

Verifying each target's HTTP mode end-to-end (real generated project, built, run, and curled against a live echo backend) surfaced bugs unrelated to this feature that fully blocked verification. All were minimal, scoped fixes, not refactors:

- **Rust**: `core/config_manager.rs.tera` had a hardcoded `"basic"` in a test fixture that broke zero-auth-scheme fixtures; replaced with a Tera conditional.
- **Python**: `Mount()` doesn't cascade a sub-app's own `lifespan`, so `mcp.streamable_http_app()`'s `session_manager.run()` never started, and every `/mcp` request 500'd with "Task group is not initialized" — fixed with an explicit `lifespan` on the parent FastAPI app. Separately, FastMCP's default `streamable_http_path="/mcp"` combined with the project's own `app.mount("/mcp", ...)` doubled the path — fixed with `streamable_http_path="/"`.
- **TypeScript**: `tsc` never copies non-`.ts` files, so the compressed schema asset never reached `dist/`, breaking every build — fixed via a `postbuild` npm script step. Separately (found during this work, but architecturally out of scope for this change and flagged/delegated instead): `startHttpServer` only ever supported one MCP session per process. That fix was completed independently in a parallel session and is folded into the same TypeScript commit here since it landed in the same two files and was verified together with this change (two concurrent sessions, independent credentials, no cross-talk).
- **Go**: the `start`/`http` cobra commands never set `Transport` based on which one ran (`CLIOverrides` has no `Transport` field at all) — fixed via an explicit `os.Setenv(...)` per subcommand, matching the pattern the other four languages already use.
- **C#**: four separate bugs, all pre-existing and all blocking HTTP-mode verification: `RunHttpHarnessAsync` never called `.WithHttpTransport()`; neither harness role ever set `Transport` in configuration (same category as Go's bug, fixed the same way); `IHttpClientFactory` was never registered, breaking the `call` tool under any transport; `IKeyedServiceProvider` was never registered as an injectable dependency, breaking `AuthManager` resolution.

## Verification

- Generator-side Rust unit tests extended: `classify.rs` (location derivation for header/query/cookie apiKey + basic/bearer/oauth2 defaults, oauth1-has-no-location), each target's `context.rs` (`from_context` asserts the new fields), each target's `steps/auth.rs` (the new shared file is always emitted).
- All 25 golden/snapshot tests (`cargo insta`) re-recorded and reviewed for the new template output.
- Manual, per-language, using real generated projects (not just golden snapshots): generated a sample project from a fixture spec with an `apiKey`-in-header scheme, ran with HTTP transport, pointed it at a small echo-backend script, and confirmed with concurrent `curl` requests carrying two different header values that each token reached the echo backend correctly and independently (no cross-contamination between concurrent sessions, verified for all five languages including TypeScript's newly multi-session-capable transport). Also confirmed a header-less localhost curl still passes the gate but the `call` tool fails with the new explicit "missing credentials" error rather than silently leaking local config, and that a spec with only an OAuth1 scheme refuses to start under `--transport http` with the documented message while `--transport stdio` is unaffected.
- Full mcpify test suite green: 335 lib tests, 25 golden tests, rollback tests, multi-version tests.

## Outcome

Implemented across all 5 targets (Rust as the reference implementation, then Python, TypeScript, Go, C#), each independently verified end-to-end via real generated projects exercised with live HTTP requests against a local echo backend, with per-request credential isolation confirmed under concurrency for every language. Nine pre-existing, unrelated bugs were discovered and fixed as minimal patches while verifying HTTP mode (1 in Rust, 2 in Python, 1 in TypeScript, 1 in Go, 4 in C#). One additional pre-existing architectural limitation — TypeScript's HTTP transport supporting only a single MCP session per process — was judged too large for this change's scope, flagged separately, completed independently, and verified compatible with this feature's changes before both landed together.
