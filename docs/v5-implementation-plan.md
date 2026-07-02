# mcpify: v5 Go Target Implementation Plan

> **Status: Complete (2026-07-02).** All of G1–G9 are implemented, committed, and verified — `-l go` is registered in `targets::build_registry()` and reachable from the CLI. Every story beyond `cargo test` on mcpify's own suite was checked against real generated output, not just compiled-in-theory: **the embeddings decision (open decision #5) was re-derived mid-implementation** after the spike found `all-minilm-l6-v2-go` broken as a normal dependency (its `go:embed`-bundled `model.onnx` is a Git LFS pointer file the Go module proxy never resolves to real content, verified: 133 bytes instead of ~90MB) — the plan's own pre-approved fallback (hand-composing `onnxruntime_go` + `sugarme/tokenizer` directly, downloading `Xenova/all-MiniLM-L6-v2` from Hugging Face at runtime instead of embedding it at build time) was confirmed working instead, with real cosine-similarity ordering verified on a hand-picked fixture. **G3 and G7 both hit the same real gofmt bug** — Tera can't predict the longest identifier in a dynamically-discovered set ahead of rendering, so grouped `const (...)` blocks and multi-entry map literals column-align incorrectly; fixed both by emitting ungrouped statements instead, a pattern this target didn't need to think about until it actually ran `gofmt -l` against real output. **G8, the launch milestone, was validated with a genuine end-to-end run of `GoTargetGenerator::execute()`** (`tests/e2e_generation.rs::generates_a_go_project_and_passes_its_own_test_suite`) against the same fixture spec the other four targets' e2e tests use — real `go mod tidy`, a real ~90MB ONNX model download, a real ONNX Runtime inference session, and `go test -tags=integration` all passing in under 25 seconds, plus a direct smoke test of the `mcpify` CLI binary itself (`-l go`). G9 adds golden/snapshot regression coverage (`tests/golden_go.rs`), a fast non-ignored `tests/go_build_smoke.rs` sanity check (added specifically because two of this target's real bugs were only catchable by actually compiling generated output), and CI additions: the fast job now installs Go too (unlike every prior target, since `go_build_smoke.rs` needs the toolchain), and the slow `e2e` job downloads/caches the ONNX Runtime shared library — a new, Go-specific CI risk analogous to the generator's own `sqlite-vec` extension-loading flakiness.

## Context

Per architecture.md's rollout notes, Go follows C# *"matching demand from... enterprise/.NET or infra... ecosystems."* Go's single-binary, low-footprint deployment story is the strongest of any target for infra/ops audiences — but an earlier version of this plan was withdrawn because the embeddings problem had no solution that didn't sacrifice Go's main selling point: CGo+ONNX bindings with no bundled model, an external embeddings API, or a Python/Node sidecar were the only options on the table, and none of them met the one-code-path/no-drift bar every other target meets cleanly.

That blocker is resolved, though not exactly as first proposed — this paragraph originally pinned the plan on `github.com/clems4ever/all-minilm-l6-v2-go`'s `go:embed`-bundled model+tokenizer, but the G6 spike found that bundling broken as a normal dependency (see open decision #5, resolved 2026-07-02). The underlying approach still holds: Go computes 384-dim embeddings on-device through `github.com/yalue/onnxruntime_go` (CGo bindings to the ONNX Runtime C library) + `github.com/sugarme/tokenizer` (a pure-Go WordPiece tokenizer — no second CGo dependency) — just composed directly by `services/embedding.go` rather than through the broken wrapper, downloading the model/tokenizer files at runtime instead of embedding them at build time. Paired with `github.com/philippgille/chromem-go` — a pure-Go, zero-third-party-dependency embeddable vector database with pluggable embedding functions and optional disk persistence — Go still gets a clean, single-code-path embeddings story: one `Embed(text string) ([]float32, error)` function is handed to `chromem-go` and reused for both the populate-time indexing step and the live `search` tool's query embedding, exactly like `embedding-service.ts` does for v1.

**The one remaining real constraint:** ONNX Runtime itself is a native C shared library (`libonnxruntime.so`/`.dylib`/`.dll`, one per OS/arch) that `onnxruntime_go` loads dynamically at runtime — it is not statically linked into the Go binary. So the generated project is no longer a single dependency-free static binary in the strictest sense; it's a Go binary plus one bundled native library. That's the same shape every other target's ONNX story already has (`@xenova/transformers` bundles a WASM runtime for TS, `Microsoft.ML.OnnxRuntime` ships native binaries via NuGet for C#, `sentence-transformers` pulls native torch wheels for Python) — it is real packaging work, not a design problem, and should be documented as a prerequisite/bundled build step rather than treated as a blocker.

**Deliberate divergence from architecture.md's roadmap table:** §3 lists Go's "DB driver + vector ext." as `mattn/go-sqlite3` + `sqlite-vec`, mirroring every other target's design of storing vectors directly in `mcp_store.db`'s `semantic_endpoints` vec0 table. This plan departs from that: Go reads the shared `mcp_store.db`'s relational `endpoints` table via `mattn/go-sqlite3` (a mature, widely-used CGo driver — low risk) but does **not** load the `sqlite-vec` extension at runtime at all. Instead, the populate step embeds each operation's reconstructed text and loads it into a `chromem-go` collection persisted to disk alongside `mcp_store.db`, and the `search` tool queries that collection directly. `mcp_store.db`'s `semantic_endpoints` vec0 table is still created by the shared pipeline (Story 5, identical for every target) but stays empty for the Go target — Go simply never consumes it. This trades "one universal vector-store format across all five targets" for "one fewer fragile CGo extension-load path" in the target whose whole value proposition is deployment simplicity. Flag this explicitly during review — it is a deliberate choice, not an oversight, and worth a one-line update to architecture.md §3's table once implementation confirms it holds up.

**What's identical to every other target, and does not need to be re-planned:** the shared 4-step pipeline (ingest, directory guard, auth profiling, `mcp_store.db` assembly) is target-agnostic and already built. `-l go` only needs a new `GoTargetGenerator` implementing the 6 per-target trait methods and registering itself in `targets::build_registry()`.

## Toolchain

| Concern | Choice |
| --- | --- |
| Async model | goroutines + channels |
| MCP SDK | community Go MCP SDK — confirm the current best-maintained package at implementation time (this ecosystem is younger than TS/Python's) |
| DB driver (relational only) | `mattn/go-sqlite3`, reading `mcp_store.db`'s `endpoints` table — no `sqlite-vec` extension load |
| Vector store + embeddings | `philippgille/chromem-go` (pure Go, zero deps, optional disk persistence) fed by a hand-rolled `services/embedding.go` composing `yalue/onnxruntime_go` (CGo inference) + `sugarme/tokenizer` (pure-Go WordPiece, stock upstream — no fork needed) directly, downloading `Xenova/all-MiniLM-L6-v2`'s `onnx/model.onnx` + `tokenizer.json` from Hugging Face at first run and caching them locally (open decision #5) |
| HTTP client (outbound) | `net/http` |
| HTTP/transport server | `net/http` (stdlib `ServeMux`), or a light router if middleware composition proves awkward — see open decision #4 |
| Schema validation | `gojsonschema` |
| Structured logging | `zap` (JSON) |
| Tracing/metrics | `go.opentelemetry.io/otel` (+ Prometheus/OTLP exporters) |
| Generated test tooling | `go test` |
| CLI invocation of output | `./<binary> --server`, or `go run .` in dev |

## Open decisions to resolve during implementation

1. **MCP SDK package.** `mark3labs/mcp-go` is currently the most visible community option — confirm its maturity/API stability before committing, same "verify before templating" discipline applied to every other target's SDK choice.
2. **ONNX Runtime native library packaging.** Decide the canonical way `bootstrap_project`/the Dockerfile bundle the correct-platform `libonnxruntime.so`/`.dylib`/`.dll`. Recommend: bundle it into the Dockerfile's build stage as the default (matching every other target's "Docker is the blessed path" precedent), with a documented manual-install fallback in the generated README for non-Docker/bare-metal use.
3. **`chromem-go` persistence lifecycle — RESOLVED (2026-07-02).** Verified with a real process-restart simulation: `NewPersistentDB(dir, false)` writes gob files under `<dir>/<collection-hash>/*.gob`; a brand-new `DB` instance pointed at the same directory (`GetCollection`, no re-`Add`) sees the same document count and returns correct query results. Persisted collection lives at `mcp_store.chromem/` sibling to `mcp_store.db`, as originally proposed.
4. **HTTP router — RESOLVED (2026-07-02).** Stdlib `net/http`. `ServeMux` plus a plain `func(http.Handler) http.Handler` middleware chain (`internal/http/middleware.go`) handled the auth-gate/CORS chain with no friction; `/metrics` is `promhttp.Handler()` (real Prometheus text format against the G3 OTel meter provider's registered reader). No `chi` or other router needed.
5. **`all-minilm-l6-v2-go`'s maturity — RESOLVED (2026-07-02), dependency dropped.** The spike (run ahead of G6, per this section's own advice) found two real problems, not just maturity risk: (1) its `go:embed`-bundled `all_minilm_l6_v2/model.onnx` is stored via Git LFS in its GitHub repo; the Go module proxy fetches a zip snapshot of the repo tree and never invokes `git lfs pull`, so `go get`/`go mod download` silently installs a 133-byte LFS *pointer* file instead of the real ~90MB model — `NewModel()` then fails at `Compute()` time with `"Failed to load model because protobuf parsing failed"`. This is unfixable from the consuming side (a `replace` directive can swap the module, not fix bytes already wrong inside it) and reproduces on a clean `go get` with no special setup. (2) Separately, its `go.mod` also carries a `replace github.com/sugarme/tokenizer => github.com/clems4ever/tokenizer ...` that consuming modules never inherit (Go ignores `replace` directives in dependencies), so even the tokenizer half breaks with `undefined: tokenizer.NewRawInputSequence` unless the *consuming* project duplicates that replace itself. Given finding (1) alone is fatal, the plan's own pre-approved fallback was taken: `services/embedding.go` composes `yalue/onnxruntime_go` (verified: loads a real ONNX Runtime `.dylib`/`.so`, runs a real BERT-shaped session) + stock upstream `sugarme/tokenizer` v0.3.0 (verified: `pretrained.FromFile` loads a Hugging Face `tokenizer.json` and tokenizes correctly with **no fork or replace directive needed** — the `NewRawInputSequence` gap only affected all-minilm-l6-v2-go's own internal call path, not the tokenizer's public `Tokenizer.EncodeSingle` API this target uses instead) directly, with the model (`onnx/model.onnx`) and tokenizer config (`tokenizer.json`) downloaded from `Xenova/all-MiniLM-L6-v2` on Hugging Face at first run and cached under `~/.<project>/models/` — the same "download once, cache, one-code-path `Embed()` function" shape `EmbeddingService.cs` already uses for C#, just with a hand-rolled inference/pooling step instead of a turnkey wrapper. Verified end-to-end: real session load + mean-pooling + L2-normalization on macOS/arm64 produced correctly-ordered cosine similarities on a hand-picked fixture (a "list widgets" query scored 0.74 against a widgets-listing sentence, vs. -0.12/-0.08 against unrelated orders/users sentences) — the same numerical-sanity bar Story 9/14 applies to every other target. `onnxruntime_go` (MIT) and `sugarme/tokenizer` (MIT) are both still used and both still fine on the license/maintenance front; only the thin, broken convenience wrapper around them was dropped.

## Story breakdown

Target-local numbering (G1, G2, ...), mirroring v1's Story 7→14 shape.

---

### G1 — Target scaffolding & template engine
**Goal:** `src/targets/go/mod.rs`: `GoTargetGenerator`, `name() -> "go"`, 6 stubbed methods. `GoTemplateContext` (mirrors the other targets' context structs). `naming.rs` for Go's conventions (`PascalCase` exported identifiers, `camelCase` unexported — a two-case system, same shape as Rust's/Python's, with Go's own exported/unexported visibility rule layered on top). Own embedded `templates/` + `tera`/`rust-embed` render/emit pair, same pattern as every other target.

**Depends on:** v1 Stories 0–6 (reused).

---

### G2 — `bootstrap_project`
**Goal:** `go.mod.tera` (module path, Go version, dependency list from the toolchain table), project skeleton (`cmd/<binary>/`, `internal/{auth,cli,core,data,http,services,tools,validation}/` — Go convention favors `internal/` to prevent accidental external imports of implementation packages), `.gitignore`, `README.md` with the ONNX Runtime native-library prerequisite called out explicitly per open decision #2.

**Depends on:** G1.

---

### G3 — `generate_enterprise_scaffolding`
**Goal:** The ~17 core-module equivalents as Go packages: `logger` (`zap` JSON config + a redaction core), `tracing` (`go.opentelemetry.io/otel` wiring), `config` (the REQ-2.2 7-tier cascade — no first-party layered-config primitive as clean as .NET's `IConfiguration`, so this is closer to v1's hand-rolled cascade than v4's; consider `spf13/viper` if the cascade proves awkward to hand-roll, but confirm it doesn't fight the CLI framework chosen in G5 first), `circuitbreaker` (`sony/gobreaker` is a well-established, small dependency — prefer it over hand-rolling, same "take the already-solved dependency" reasoning as v4's `Polly` recommendation), `credentialstorage` (`zalando/go-keyring` is Go's direct equivalent of Node's `keytar`/Python's `keyring` — same OS-keychain-with-fallback shape), `healthcheck`, `ratelimiter`, `cache`, `mcpserver` (wraps the SDK from open decision #1), plus `Dockerfile.tera` (multi-stage: a build stage that also stages the correct-platform `libonnxruntime.so` per open decision #2, producing a minimal final image — Go's static-binary compilation makes this the leanest Dockerfile of any target apart from the one bundled native library), `docker-compose.yml.tera`, and the three GitHub Actions workflow templates (`gofmt -l`/`go vet` replacing `format:check`/`lint`, `go build ./...` then `go test ./...`).

**Depends on:** G2.

---

### G4 — `generate_auth_strategies`
**Goal:** Same 5 strategies as every target, expressed as a Go `AuthStrategy` interface with one implementing struct per discovered scheme, and an `authmanager` package dispatching by `auth_method` via a `map[string]AuthStrategy` (Go's idiomatic equivalent of v1's TS object-literal lookup, v2's Rust `match`, v3's Python dispatch dict). OAuth1 RSA-SHA1/HMAC-SHA1 signing via `dghubble/oauth1` (a maintained, widely-used Go OAuth1 client library) or hand-rolled signature-base-string construction if its coverage doesn't fit; OAuth2 PKCE + refresh via `golang.org/x/oauth2` (the de facto standard, first-party-adjacent).

**Depends on:** G3.

---

### G5 — `generate_transports_and_roles`
**Goal:** Dual-role entry point via `spf13/cobra` (the de facto standard Go CLI framework, with native subcommand support matching the 9-subcommand shape every other target emits: `setup`, `search`, `get`, `call`, `start`, `http`, `test-connection`, `config`, `version`) dispatching between Terminal Client and Harness Server. `net/http`-based HTTP transport translating v1's localhost-detector/auth-extractor/metrics concerns into Go middleware (`func(http.Handler) http.Handler` chaining) — resolve open decision #4 here if stdlib composition proves too bare.

**Depends on:** G3, G4.

---

### G6 — `generate_mcp_tools`
**Goal:** The core value-delivery step, and where the embeddings decision actually gets proven. `data/store.go` — relational queries against `mcp_store.db`'s `endpoints` table via `mattn/go-sqlite3` (no `sqlite-vec` load, per the deliberate divergence above). `services/embedding.go` — wraps `all-minilm-l6-v2-go`, exposing one `Embed(text string) ([]float32, error)` function; run the maturity spike from open decision #5 here before building on top of it. `services/vectorstore.go` — wraps a `chromem-go` collection (persisted per open decision #3), populated by a `cmd/populate-embeddings` step reconstructing `method + path + summary + description` per `endpoints` row (mirroring every other target's populate step exactly) and queried by the `search` tool using the same `services/embedding.go` function — this is the file where the one-code-path guarantee is structurally enforced, since both callers import the same package. `services/apiclient.go` — generic `net/http`-based operation dispatcher reading parameter locations from the resolved schema, same design as every other target. `tools/{search,get,call}.go` + tool registration, names `search`/`get`/`call` per the resolved naming decision. `validation/validator.go` (`gojsonschema`-based) against the same co-located JSON asset pattern every target uses.

**Depends on:** G3, G4, G5.

---

### G7 — `generate_setup_wizard_and_tests`
**Goal:** Interactive setup wizard via `AlecAivazis/survey` (Go's closest equivalent to `inquirer`/`questionary`'s prompt UX), base-URL + connectivity check, auth-method prompt driven by discovered schemes only. Generated `go test` suite: table-driven unit tests per package (idiomatic Go, not a 1:1 file-per-module port of the other targets' structure), integration/e2e tests gated by a build tag (e.g. `//go:build integration`) rather than a separate config file the way `vitest.config.ts`/`pytest.ini` do it — same "skip slow/networked tests by default" requirement, expressed the Go-native way. Same hard requirement as every target: conditionally emit auth-strategy tests only for discovered schemes, or an unused import breaks the build outright (Go's compiler treats unused imports as a hard error, making this even less forgiving than the other targets' lint-level version of the same rule).

**Depends on:** G3, G4, G5, G6.

---

### G8 — `run_generated_tests` + registration
**Goal:** Shell out to `go mod download` (or rely on `go build`'s implicit fetch) → `go build ./...` (the compile step other targets either skip or fold into their test runner — Go's static typing makes this a real, separate, valuable gate) → `go run ./cmd/populate-embeddings` (must precede tests, since the `chromem-go` collection starts empty — same sequencing requirement v1's `populate-embeddings.ts` has relative to `npm test`) → `go test ./...`. Confirm the ONNX Runtime shared library is resolvable in the test environment (open decision #2 resolved concretely here, not just documented). Capture stdout/stderr into `anyhow::Error` context on failure, wrapped in a `tokio::time::timeout` (the ONNX model load + first-inference warmup is a real, if smaller, analog of v1's `@xenova/transformers` first-run download risk — bound it). Register `GoTargetGenerator` in `targets::build_registry()` only once this is real and green.

**Depends on:** G7. **Treat this as the v5 launch milestone.**

---

### G9 — Golden/snapshot tests + CI additions
**Goal:** `tests/golden_go.rs`, same pattern as the other targets, reusing the shared OpenAPI fixtures. Extend `.github/workflows/ci.yml`'s fast job to install Go (`actions/setup-go`) and, unlike every prior target, also install/cache the platform-specific ONNX Runtime shared library on CI runners — flag this as a new, Go-specific CI risk analogous to the `sqlite-vec` extension-loading flakiness already flagged for the generator's own Story 5 tests. Add a slow-job step for a Go-target `e2e_generation`-equivalent test.

**Depends on:** G8.

## Sequencing

G1 → G2 → G3 → G4 → (G5 ‖ G6 once G4 lands) → G7 → G8 → G9. Run open decision #5's maturity spike (`all-minilm-l6-v2-go`) as early as possible — ideally before G3, definitely before G6 depends on it structurally — since a negative result changes G6's scope from "wrap a library" to "compose two lower-level libraries directly," which is more work and worth knowing about early. Resolve open decision #1 (MCP SDK) before G5/G6, which are the stories that actually consume it.

## Verification

Same shape as every other target: per-story `cargo test` on mcpify's own suite, golden/snapshot regression tests, and the real gate — an `#[ignore]`-by-default Rust test running `GoTargetGenerator::execute()` against a fixture spec, asserting the generated project's own `go build ./...` and `go test ./...` pass for real, with the ONNX Runtime shared library available in the test environment. Also manually verify a semantic-search query against a tiny fixture returns sane, correctly-ordered results once during G6/G8 development — this is where the embeddings decision's correctness actually gets proven for this target, same as every other target's Story 9/14-equivalent check.
