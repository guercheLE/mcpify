# mcpify: v4 C# (.NET) Target Implementation Plan

> **Status: In progress (2026-07-02).** C1–C4 are implemented, committed, and verified (`cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`, plus a real `dotnet restore && dotnet build` and `dotnet format --verify-no-changes` against the scaffolded output covering all 5 auth-strategy kinds together — all green). C4's OAuth1 RSA-SHA1 signing was additionally verified functionally (not just compiled): a throwaway RSA keypair signed and verified a base string with the exact `RSA.SignData`/`RSASignaturePadding.Pkcs1` primitives the generated `OAuth1Signer` uses, confirming both a correct signature and correct rejection of a tampered base string. `-l csharp` is **not yet** registered in `targets::build_registry()`; per this plan, that happens only once C8 is real and green. This plan covers adding the C#/.NET output target (`-l csharp`) to the mcpify generator, per `docs/architecture.md`'s "Target Language Roadmap" (§3). It assumes `docs/v1-implementation-plan.md` is complete — the shared pipeline, CLI, `GeneratorContext`, `McpServerTargetGenerator` trait, rollback, and `TargetRegistry` are reused as-is. This plan only covers the new, C#-specific per-target work.

## Context

Per architecture.md's rollout notes, C# follows Python *"matching demand from... enterprise/.NET... ecosystems."* .NET's own conventions differ from every target planned so far in one structurally significant way worth flagging up front: **the ecosystem strongly favors a DI (dependency-injection) container** (`Microsoft.Extensions.DependencyInjection`, and the generic `Host`/`WebApplication` builder patterns built on it) for wiring together exactly the kind of enterprise scaffolding this project generates — logging, configuration, health checks, and OpenTelemetry all have first-party `Microsoft.Extensions.*` packages built around DI registration, not the more ad-hoc "import a module-level singleton" pattern the other three targets use. Templates for this target should lean into that idiom rather than fight it (register `AuthManager`, the circuit breaker, the store repository, etc. as services in a builder's `IServiceCollection`), even though it means this target's `core/` module shape will look meaningfully different in *structure* from v1/v2/v3's, despite delivering the identical capability set.

**What's identical to every other target, and does not need to be re-planned:** the shared 4-step pipeline (ingest, directory guard, auth profiling, `mcp_store.db` assembly) is target-agnostic and already built. `-l csharp` only needs a new `CSharpTargetGenerator` implementing the 6 per-target trait methods and registering itself in `targets::build_registry()`.

## Toolchain (architecture.md §3)

| Concern | Choice |
| --- | --- |
| Async runtime | `Task`/`async`-`await` |
| MCP SDK | Community/official .NET MCP SDK — confirm current NuGet package name/API shape at implementation time |
| DB driver + vector ext. | `Microsoft.Data.Sqlite` + `sqlite-vec` |
| HTTP client (outbound) | `HttpClient` (via `IHttpClientFactory` for the resilience/pooling behavior the DI-first idiom expects) |
| HTTP/transport server | Kestrel, via `WebApplication`/minimal APIs |
| Schema validation | `JsonSchema.Net` |
| Structured logging | `Serilog` (compact JSON sink) |
| Tracing/metrics | `OpenTelemetry.Extensions.Hosting` |
| Generated test tooling | `dotnet test` |
| CLI invocation of output | `dotnet <assembly>.dll --server`, or a self-contained/AOT-published executable |

## Open decisions to resolve during implementation

1. **MCP SDK package — RESOLVED (2026-07-02).** [`ModelContextProtocol`](https://www.nuget.org/packages/ModelContextProtocol) (+ `ModelContextProtocol.AspNetCore` for the Kestrel/HTTP transport) — the official .NET MCP SDK (`csharp.sdk.modelcontextprotocol.io`), verified on NuGet: 16M+ total downloads, stable `1.4.0` release (not prerelease), first-party `Microsoft.Extensions.DependencyInjection` hosting extensions built in, matching this target's DI-first idiom directly. Production-ready; the hand-rolled-JSON-RPC fallback risk did not materialize.
2. **Embeddings.** `Microsoft.ML.OnnxRuntime` (ONNX Runtime for .NET) + a tokenizer package (`Microsoft.ML.Tokenizers`, or a BERT-family tokenizer implementation if that package's coverage doesn't include the target model's tokenizer) is the direct C# analog of v2 Rust's `ort`/`fastembed-rs` path. Same one-code-path principle as every other target: the module/service that embeds text must be the exact one both the populate-step and the live `search` tool inject via DI, not two independent implementations of "call ONNX Runtime." Both packages are already referenced in the C2 `.csproj.tera` (versions `1.27.0` / `2.0.0`); the actual embedding-service *implementation* is still open, deferred to C6.
3. **Test framework — RESOLVED (2026-07-02).** `xunit` (`2.9.3`, + `xunit.runner.visualstudio` `3.1.5`, + `Microsoft.NET.Test.Sdk` `18.7.0`), per the plan's own "more common modern default" steer. To be wired into a generated `<Namespace>.Tests.csproj` in C7.
4. **Native AOT vs. framework-dependent publish — RESOLVED (2026-07-02).** Framework-dependent (standard `dotnet build`/`dotnet publish`), per the plan's own "simpler, safer default" framing — Native AOT's reflection constraints are a poor fit for this target's DI-heavy, reflection-touching dependency set (Serilog, OpenTelemetry, the MCP SDK's own hosting extensions). `Project.csproj.tera` targets `net10.0` (current LTS; every even-numbered .NET release is LTS) with no `PublishAot` property set. This resolves which MCP SDK/logging/JSON libraries are viable — confirmed C2's full toolchain package set actually restores and builds via a real `dotnet build` against the scaffolded skeleton (see C2 below).

## Story breakdown

Target-local numbering (C1, C2, ...), mirroring v1's Story 7→14 shape.

---

### C1 — Target scaffolding & template engine ✅ Done
**Goal:** `src/targets/csharp/mod.rs`: `CSharpTargetGenerator`, `name() -> "csharp"`, 6 stubbed methods. `CsTemplateContext` (mirrors the other targets' context structs). `naming.rs` for C#'s conventions (`PascalCase` for types/methods/properties, `camelCase` for locals/parameters — a two-case system like Rust's, but with the case assignment flipped by convention). Own embedded `templates/` + `tera`/`rust-embed` render/emit pair.

**Depends on:** v1 Stories 0–6 (reused).

---

### C2 — `bootstrap_project` ✅ Done
**Goal:** `<ProjectName>.csproj.tera` (target framework, package references from the toolchain table, resolved per open decision #4), `Program.cs.tera` skeleton, project folder skeleton (`Auth/`, `Cli/`, `Core/`, `Data/`, `Http/`, `Services/`, `Tools/`, `Validation/` — C# convention capitalizes folder/namespace names, unlike the lowercase folders every other target uses), `.gitignore`, `README.md`. Given the DI-first idiom, this step should also emit the skeleton `IServiceCollection` registration method (e.g. `AddMcpifyServices(this IServiceCollection services)`) that later steps (C3–C6) each extend, rather than each step wiring its own ad-hoc singleton.

**Depends on:** C1.

---

### C3 — `generate_enterprise_scaffolding` ✅ Done
**Goal:** The ~17 core-module equivalents, each registered as a DI service rather than a bare module: `Logging` (Serilog configuration + a redaction enricher), `Tracing` (`OpenTelemetry.Extensions.Hosting` builder extensions), `Config` (the REQ-2.2 7-tier cascade via `IConfiguration`'s own layered-provider model, which already natively supports exactly this kind of cascade — `AddCommandLine`, `AddEnvironmentVariables`, `AddJsonFile` at multiple paths, `AddInMemoryCollection` for defaults — likely the cleanest REQ-2.2 implementation of any target, worth confirming this maps cleanly before assuming it), `CircuitBreaker` (or adopt `Polly`, a mature, widely-used .NET resilience library covering circuit-breaker/retry/rate-limiting in one dependency — strongly consider this over hand-rolling three separate modules, since it's exactly the kind of "already-installed dependency solves it" case worth taking), `CredentialStorage` (evaluate whether a maintained OS-keychain-wrapping NuGet package exists; if not, this is the one target where hand-rolling per-OS keychain access via P/Invoke may be unavoidable — flag as real implementation risk), `HealthChecks` (ASP.NET Core's own `Microsoft.Extensions.Diagnostics.HealthChecks` is a first-party fit here, again likely cleaner than a hand-rolled registry), `McpServer` (wraps the chosen SDK), plus `Dockerfile.tera` (multi-stage `mcr.microsoft.com/dotnet/sdk` → `mcr.microsoft.com/dotnet/aspnet` or a Native AOT single-stage build per open decision #4), `docker-compose.yml.tera`, and the three GitHub Actions workflow templates (`dotnet format --verify-no-changes` replacing `format:check`, `dotnet build`/`dotnet test`).

**Depends on:** C2.

---

### C4 — `generate_auth_strategies` ✅ Done
**Goal:** Same 5 strategies as every target, expressed as an `IAuthStrategy` interface and DI-registered implementations, with the auth-manager resolving the active one via a keyed-service lookup (`.NET 8+`'s keyed DI services map onto "select one active strategy by a config-driven key" unusually well — investigate before falling back to a manual dictionary/switch the way the other targets do, since this may be a case where the platform's own primitive is a better fit than replicating the other targets' pattern verbatim).

**Depends on:** C3.

---

### C5 — `generate_transports_and_roles`
**Goal:** Dual-role entry point — `Program.cs` branching on a CLI argument/subcommand (`System.CommandLine` is the modern first-party choice for this) between Terminal Client and Harness Server modes. Kestrel/minimal-API HTTP transport with middleware for the localhost-detection/auth-extraction/metrics concerns v1 hand-rolled in `node:http` — ASP.NET Core middleware is the idiomatic translation.

**Depends on:** C3, C4.

---

### C6 — `generate_mcp_tools`
**Goal:** `search`/`get`/`call`, a `Microsoft.Data.Sqlite` + `sqlite-vec` repository service, an `HttpClient`-based API client (generic operation dispatcher reading parameter locations from the resolved schema — same design as every other target, via `IHttpClientFactory` for the pooling/resilience the DI-first idiom expects, plausibly composed with `Polly` from C3's circuit breaker), `JsonSchema.Net`-based validation against the same co-located JSON asset pattern, and the embedding service from open decision #2 — DI-injected into both the `search` tool and the populate-embeddings step/service so they share one implementation.

**Depends on:** C3, C4, C5.

---

### C7 — `generate_setup_wizard_and_tests`
**Goal:** Interactive setup wizard (`Spectre.Console` is the strong modern default for rich .NET CLI prompts — closer to `inquirer`'s UX than a bare `Console.ReadLine()` loop) and the generated test suite in the framework chosen at open decision #3, conditionally emitting auth-strategy tests only for discovered schemes — same hard requirement as every target.

**Depends on:** C3, C4, C5, C6.

---

### C8 — `run_generated_tests` + registration
**Goal:** Shell out to `dotnet restore` (if not folded into build) then `dotnet test`, which itself compiles as a prerequisite — same "one signal proves both build and functional correctness" principle as every other target. If Native AOT was chosen (open decision #4), confirm `dotnet test` still works cleanly against the same project structure the AOT-published binary uses, since AOT publish and the standard test host don't always share every constraint. Register `CSharpTargetGenerator` in `targets::build_registry()` only once this is real and green.

**Depends on:** C7. **Treat this as the v4 launch milestone.**

---

### C9 — Golden/snapshot tests + CI additions
**Goal:** `tests/golden_csharp.rs`, same pattern as the other targets, reusing the shared OpenAPI fixtures. Extend `.github/workflows/ci.yml`'s fast job to install a .NET SDK (`actions/setup-dotnet`) and add a slow-job step for a C#-target `e2e_generation`-equivalent test.

**Depends on:** C8.

## Sequencing

C1 → C2 → C3 → C4 → (C5 ‖ C6 once C4 lands) → C7 → C8 → C9. Resolve open decision #4 (Native AOT vs. framework-dependent) before C2, since it constrains every later library choice (MCP SDK, JSON library, DI container usage patterns); resolve open decision #1 (MCP SDK) before C5/C6, which are the stories that actually consume it.

## Verification

Same shape as every other target: per-story `cargo test` on mcpify's own suite, golden/snapshot regression tests, and the real gate — an `#[ignore]`-by-default Rust test running `CSharpTargetGenerator::execute()` against a fixture spec, asserting the generated project's own `dotnet test` passes for real. Also manually verify a semantic-search query against a tiny fixture returns sane, correctly-ordered results once during C6/C8 development.
