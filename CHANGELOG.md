# Changelog

All notable changes to mcpify are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). This project releases via `chore(release): bump version to X.Y.Z` commits, each corresponding to a `vX.Y.Z` git tag.

**A note on the version history below:** this file was reconstructed from `git log` and `git tag` after the fact (no `CHANGELOG.md` existed during v0.1.0–v0.11.14). Two wrinkles worth knowing if you're cross-referencing tags yourself:

- `v0.1.0` and `v0.1.1` were tagged retroactively, before the `chore: bump version to X.Y.Z` commit convention started — they point at commits partway through the initial build-out, not at dedicated "bump" commits.
- Two version bumps (`0.3.0`, `0.5.11`) were committed to `Cargo.toml` but never tagged/released — evidently superseded same-day by the next bump. Their changes are folded into the following tagged release (`v0.3.1`, `v0.5.12` respectively) rather than given their own section.

Internal-only changes (CI plumbing, generator-test hardening, formatting) are generally omitted unless they affect what gets published or how the generated projects behave.

## [Unreleased]

_No changes yet._

## [0.11.14] - 2026-07-26

### Fixed

- Build multi-architecture (amd64+arm64) GHCR images for every generated project on **native runners** instead of QEMU emulation — QEMU cross-arch emulation (introduced one release earlier, in v0.11.12) turned out to be unreliably slow/flaky in CI.

## [0.11.13] - 2026-07-26

### Fixed

- Updated the golden TypeScript-target release-workflow test snapshot to match the multi-arch Docker build fix (keeps the generator's own snapshot tests green after the v0.11.12 template change).

## [0.11.12] - 2026-07-26

### Fixed

- Every generated project's release workflow now publishes multi-architecture (amd64+arm64) GHCR container images, instead of a single-architecture image. (See v0.11.14 — the initial QEMU-based approach here was replaced with native runners two releases later.)

## [0.11.11] - 2026-07-26

### Fixed

- Skip the flaky disk-lock test on GitHub Actions' Windows runners entirely, rather than just retrying with a longer window — closes out the Windows store-lock flakiness saga that spans v0.10.5 through here.

## [0.11.10] - 2026-07-26

### Fixed

- Always emit the `repository` field in generated package manifests when present in the source spec, instead of dropping it conditionally.
- Widened the Windows store-lock retry window to 60 seconds to further reduce flaky store-extraction failures in CI.

## [0.11.9] - 2026-07-26

### Fixed

- Force `dist = true` for unpublished internal crates so `cargo-dist` packages them correctly.
- Fixed a test race caused by multiple tests locking on `$HOME` concurrently.
- Widened Windows retry windows further for store-lock contention.

## [0.11.8] - 2026-07-26

### Fixed

- Switched the `cli_smoke` test's sentinel URL to an ephemeral port to avoid port collisions in CI; reformatted `generation_fixtures.rs`.
- Added a retry around `remove_file` in the store lock-release test to tolerate Windows' slower file-handle release timing.

## [0.11.7] - 2026-07-26

### Fixed

- Fully inline `$ref` pointers in generated/stored schemas instead of localizing them into `$defs` — the third and final iteration of making the schemas the `get` tool returns fully self-contained (see also v0.5.11 and v0.11.5).

## [0.11.6] - 2026-07-22

_Internal only: added generator-level regression tests asserting schemas are self-contained and content-addressed. No user-facing change._

## [0.11.5] - 2026-07-21

### Fixed

- Embed `$defs` directly into the literal schemas the `get` tool returns, instead of leaving external `$ref` pointers unresolved — second iteration of the self-contained-schema fix (see v0.5.11, v0.11.7).

## [0.11.4] - 2026-07-21

### Fixed

- Capped mcpify's own CI test job at a 20-minute timeout, so a hang fails fast instead of running out the default runner limit.

## [0.11.3] - 2026-07-21

### Fixed

- Capped the release workflow's build job at a 45-minute timeout for the same reason as v0.11.4.

## [0.11.2] - 2026-07-21

### Fixed

- The Rust target's generated server now recovers from mutex poisoning on the shared `mcp_store.db` connection, instead of the process becoming permanently unable to serve requests after a single panic.

## [0.11.1] - 2026-07-19

### Fixed

- Stopped the Rust target's setup-wizard smoke test from hanging on GitHub Actions Windows runners.
- Added retry logic around the store-file rename to tolerate a cross-process race during store extraction.

### Changed

- CI now installs `cargo-dist` from a prebuilt binary instead of compiling it from source, speeding up release builds.

## [0.11.0] - 2026-07-19

### Added

- Generated Rust projects now include a sponsorship callout and a `FUNDING.yml`.

### Documentation

- Added a matching sponsorship callout and `FUNDING.yml` to mcpify's own README.

## [0.10.8] - 2026-07-19

### Fixed

- Stopped the Rust target's stdio-transport smoke test from hanging on GitHub Actions Windows runners.

## [0.10.7] - 2026-07-19

### Fixed

- Relaxed a stdio-transport error assertion in generated Rust tests that was too strict/fragile on Windows.

## [0.10.6] - 2026-07-19

_Internal only: closed the remaining generated-project production-coverage gap in the Rust target's test suite. No user-facing change._

## [0.10.5] - 2026-07-19

### Fixed

- Profiling now degrades gracefully instead of failing outright when `samply` can't profile on sandboxed CI runners.
- Eliminated a Windows-only race condition in the generated Rust target's `resolve_store_path` logic.

## [0.10.4] - 2026-07-19

### Fixed

- Moved the `allow-dirty` `cargo-dist` setting to the correct `[dist]` table (it had incorrectly been placed under `[dist.github-custom-runners]`), fixing generated Rust projects' release workflow.

## [0.10.3] - 2026-07-19

### Fixed

- Generated Rust projects' release workflow now allows a dirty git tree by default, matching how the release actually runs in CI.

## [0.10.2] - 2026-07-19

### Fixed

- Preserve an existing project's name when re-syncing/regenerating a Rust project, instead of silently overwriting it.

## [0.10.1] - 2026-07-19

### Fixed

- Adjusted the release workflow so its hand-added test gate passes correctly.
- The generated Rust setup-wizard test no longer requires a real TTY, fixing it in headless CI environments.

## [0.10.0] - 2026-07-18

### Added

- Generated Rust projects now ship with ~85% production test coverage and warm-start profiles.
- Each API-version store is embedded zstd-compressed in the Rust target, reducing binary size.

### Fixed

- Separated generated CPU and heap profiling in the Rust target (previously conflated into one workflow).
- Generated profiling workflows are now self-contained.
- `mcpify` now enforces production release gates on its own generator codebase.
- Fixed a Go-target import-shadowing bug (the `url` package) in generated setup code.
- Resolved the generated project's name from the real working directory when `--output .` is passed, instead of picking up a stale or incorrect name.
- Isolated generated Rust `cli_smoke` tests (both stdio- and HTTP-transport variants) from the developer's real global config, preventing test pollution and false failures.
- Restored cargo-default `.gitignore` entries and the `.mcpify` scratch-file guard in generated Rust projects.
- Installed missing Go lint tools in the perf CI workflow and switched to the canonical `golangci-lint` module.

### Documentation

- Restored the Observability & Resilience, License, and install sections to the generated Rust README (had been dropped).
- Added a "Connect an MCP client" section to the generated Rust README.

## [0.9.1] - 2026-07-17

### Fixed

- Generated Rust `api_client.rs` is now kept `cargo fmt`-clean.

## [0.9.0] - 2026-07-17

### Added

- A PKCE-based OAuth2 setup wizard, with scopes derived from the OpenAPI spec, across all 5 targets.

### Fixed

- Completed the OAuth2 authorization-code exchange flow and ensured `Content-Length` is sent on empty request bodies, across all 5 targets — the exchange was previously incomplete, and some HTTP servers reject bodyless requests missing that header.
- The Go target now auto-downloads the ONNX Runtime shared library instead of requiring it pre-installed on the host.

## [0.8.2] - 2026-07-16

### Fixed

- The Rust target now extracts `mcp_store.db` atomically, avoiding a race when multiple calls trigger extraction concurrently.

## [0.8.1] - 2026-07-16

### Fixed

- The `call` tool no longer rejects a response outright on an output-schema mismatch; it now surfaces the validation error as a warning while still returning the live data, across all 5 targets — real-world OpenAPI specs are frequently wrong about response shape, and treating every mismatch as fatal denied callers real data over a documentation bug.
- Fixed a `gocritic` lint failure and a module-root-relative test-path bug in the Go target.

## [0.8.0] - 2026-07-15

### Fixed

- Hardened auth, credential-handling, and embedding-population logic for parity across all 5 language targets.

## [0.7.0] - 2026-07-14

### Added

- Manifest-driven generator parity: target generation is now driven by a shared manifest, with the Rust target's repository layer hardened to match.

## [0.6.0] - 2026-07-13

### Added

- OpenAPI 3.1 spec support across all targets (previously 3.0 only).

## [0.5.18] - 2026-07-13

### Fixed

- Generated Docker images now correctly configure the MCP transport (stdio/HTTP) at container runtime.

## [0.5.17] - 2026-07-13

### Fixed

- Generated Docker images now package every API-version store, not just the default version.

## [0.5.16] - 2026-07-13

### Fixed

- Copy every version's database into Docker builds (a first pass at the fix completed in v0.5.17); run formatting after `add-version`/`remove-version`.
- Generated `call` examples in docs now correctly use `--args`.

## [0.5.15] - 2026-07-10

### Added

- mcpify is now distributed as prebuilt binaries via `cargo-dist`, in addition to `cargo install`.

## [0.5.14] - 2026-07-08

### Documentation

- Fixed broken documentation links and added the missing LICENSE file.

## [0.5.13] - 2026-07-08

### Fixed

- The Python target's HTTP client now treats non-2xx responses as connection failures, instead of handling them inconsistently.

## [0.5.12] - 2026-07-08

### Fixed

- Trust OS-native root certificates alongside `webpki-roots` for `rustls`-based TLS in the Rust target, fixing TLS failures against internal/enterprise CAs.
- Embed only reachable schema definitions in `mcp_store.db`, instead of the entire spec's schema set — first iteration of the self-contained/minimal-schema fix (see also v0.11.5, v0.11.7). *(Bumped internally as `0.3.0`→`0.5.11`; no `v0.5.11` tag was cut, so this ships as part of v0.5.12.)*
- Normalize generated credentials before use in the Rust target.

### Documentation

- Corrected a stale cross-target embedding-parity claim in the C# target's docs.

## [0.5.10] - 2026-07-06

### Fixed

- Rust, C#, and TypeScript targets now write logs to stderr instead of stdout unconditionally — stdout is reserved for JSON-RPC frames in stdio transport mode. *(This made stderr the destination for every transport mode, not just stdio; see `docs/DOC-GAPS.md` for the transport-mode-aware follow-up.)*

## [0.5.9] - 2026-07-06

### Fixed

- Install the `rustls` crypto provider before first TLS use in the Rust target, fixing a startup panic on some platforms.

## [0.5.8] - 2026-07-06

### Added

- A `remove-version` CLI command to remove a previously added API-spec version from a generated project.

## [0.5.7] - 2026-07-06

### Fixed

- Fixed multi-version embedding population across the TypeScript, Python, C#, and Go targets.

## [0.5.6] - 2026-07-06

### Fixed

- Made `VERSION_STORE_FILES` public, always refresh store extraction, and added an `--all` backfill option in the Rust target's version-store handling.

## [0.5.5] - 2026-07-06

### Fixed

- The Rust target now embeds the store database's bytes directly (not just path-based fallbacks), and resolves the store via embedded-only resolution instead of a path-fallback chain — part of the store-path CWD-independence saga (see `docs/DOC-GAPS.md`).

## [0.5.4] - 2026-07-06

### Fixed

- The Rust target's generated store-path resolution now falls back to the executable's own directory instead of assuming a CWD-relative path — first iteration of the store-path fix (see `docs/DOC-GAPS.md`).

## [0.5.3] - 2026-07-05

### Fixed

- Namespaced the Rust target's generated helper binary to avoid collisions with other binaries.
- Reordered imports in generated Rust code to satisfy `cargo fmt`.

## [0.5.1] - 2026-07-05

### Added

- Generated TypeScript projects' CI now publishes container images on tag pushes.

### Fixed

- `add-version` now shows console progress output during long-running runs, matching `generate`.

## [0.5.0] - 2026-07-05

### Added

- Console progress output during long-running generation steps.
- Enhanced operation-ID normalization to handle duplicate IDs within an OpenAPI spec.

### Fixed

- Resolved `gocritic` lint failures in generated Go templates.

### Documentation

- Added the progress-output implementation plan.

## [0.4.0] - 2026-07-05

### Added

- mcpify itself now publishes a versioned Docker image to GHCR on release.

### Fixed

- Dropped `native-tls` from `fastembed` in the Rust target and matched `ort`'s glibc requirement, fixing embedding generation on some Linux distributions.
- Bumped GitHub Actions across all targets to Node 24-native major versions.

## [0.3.1] - 2026-07-05

### Added

- Auth-scheme profiling now captures header/query/cookie location metadata, threaded into all 5 targets' template context.
- All 5 targets now forward HTTP request credentials from the incoming request instead of reading local config, when running in HTTP transport mode — closes a correctness gap where HTTP-mode servers ignored per-request credentials in favor of the process's own local config.

### Fixed

- Compressed the schemas asset and cache the store in memory across all targets, reducing generated project size and startup I/O.
- Repaired `cargo fmt` drift and Go/Python generated-code lint failures in CI.
- The Python target now closes its cached DB connection so the process actually exits, instead of hanging open.

*(An intermediate `0.3.0` version bump was made mid-release but never tagged; its changes are folded into this v0.3.1 entry.)*

## [0.2.0] - 2026-07-04

### Added

- Multi-version OpenAPI spec support: `mcpify add-version` lets an operator layer additional spec versions onto an already-generated project, across all 5 targets, with a `--set-default` promotion flow.

### Documentation

- Documented multi-version OpenAPI spec support in the README and the v8 implementation plan.

## [0.1.1] - 2026-07-04

### Documentation

- Updated the README's `--language` flag documentation to list all five shipped targets.

## [0.1.0] - 2026-07-04

Initial release.

### Added

- Core generator pipeline: OpenAPI ingestion (local file + remote URL, JSON/YAML), output-directory guard, auth-scheme profiling (Basic/API Key/Bearer-PAT/OAuth2/OAuth1-style), and `mcp_store.db` assembly (relational `endpoints` table + `sqlite-vec` semantic search table).
- All 5 output-language targets implemented and registered: TypeScript, Rust, Python, C#, and Go — each producing a project with the 3 universal tools (`search`/`get`/`call`), dual client/server runtime roles, single-strategy auth, the full config-resolution cascade, and the "enterprise-first" bar (structured logging, OpenTelemetry tracing/metrics, circuit breaker/retry/rate-limiting, health checks, OS-keychain credential storage, generated test suites, multi-stage Docker + CI/CD).
- `run_generated_tests`: every generation run installs dependencies and runs the generated project's own test suite to completion as the final step, enforcing a zero-placeholder quality bar.
- Coverage and profiling tooling for mcpify's own codebase, plus coverage/profiling scaffolding in all 5 generated targets.
- Opt-in package-registry publishing for the Rust, Python, and C# targets.
- Generation-time lint enforcement across all 5 targets: `cargo clippy` (Rust), Roslynator analyzers (C#), `golangci-lint`/gocritic (Go), Biome replacing ESLint+Prettier (TypeScript).

### Fixed

- Signed OAuth1 RSA-SHA1 requests with the real PEM instead of a value mangled by the `oauth-1.0a` library (TypeScript target).
- Omitted `token_type_ids` when the ONNX embedding model doesn't declare it (C# target).
- Auto-download the `sqlite-vec` native extension on first use instead of requiring it pre-installed (C# target).
- Dropped a broken `all-MiniLM-L6-v2-go` dependency and fixed the `cmd/` directory layout in the Go target.
- Installed `golangci-lint` and the Go toolchain in the e2e and release CI jobs.

### Documentation

- Added the product brief, PRD, architecture doc, and an expanded README.
- Authored the v1–v7 implementation plans (TypeScript, Rust, Python, C#, and Go targets; coverage/profiling/publishing; generation-time lint enforcement), each marked complete as its target landed.
