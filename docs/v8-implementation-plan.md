# mcpify: v8 Multi-Version OpenAPI Support Implementation Plan

> **Status: Complete (2026-07-04).** All five targets (TypeScript, Rust, Python, C#, Go) now support layering additional OpenAPI spec versions onto an already-generated project via a new `mcpify add-version` subcommand, without regenerating the whole project. Each version gets its own SQLite store and schemas asset; a generator-only ledger (`.mcpify/versions.json`) tracks what exists; a small set of marker-delimited "version-aware" regions in each target's generated code get re-rendered in place when a version is added or promoted to default. The original single-spec `generate` flow is unchanged for projects that never call `add-version` (e.g. Bamboo, which only ever has one API version).

## Context

Prompted by a concrete need: generating MCP servers for four Atlassian products (Bamboo, Jira, Bitbucket, Confluence), three of which publish many historical API versions as separate OpenAPI documents. mcpify's generator only ever ingested one spec per invocation, with every artifact (`mcp_store.db`, the per-target schemas asset) hardcoded to a single set of paths — there was no way to add a second version's data to an already-generated project without a full, destructive regeneration.

## Design

Mirrors a pattern already proven in this codebase: **auth-strategy selection**. `generate` already discovers N auth schemes and bakes them into a static enum (`auth_method_keys` → `AuthMethodSchema`/`AuthMethod`/etc.), with the generated project picking one active strategy at runtime via the config cascade. Version selection works the same way — N version-store files accumulate on disk over time (one per `generate`/`add-version` call), baked into a static per-target enum/map, with one active version selected at runtime via the same config cascade (`api_version`, mirroring `auth_method`) and an analogous setup-wizard prompt.

**One SQLite store file per version**, not a shared db with a version column — avoids touching `endpoints`/`semantic_endpoints` schema or queries in any target. The default version keeps today's exact hardcoded paths (`mcp_store.db` and each target's existing schemas-asset path), so single-version projects see zero behavioral change. Extra versions get suffixed siblings (`mcp_store_v<label>.db`, `generated-schemas_v<label>.json`), derived generically from the *current default's* file names — `add-version` never needs any per-target/per-project naming knowledge.

**Generator-only ledger** (`.mcpify/versions.json`) tracks `{schema_version, language, display_name, project_name, default_version, versions: {label: {db_file, schemas_file, source, added_at}}}`. `display_name`/`project_name`/`language` are pinned at `generate` time and never re-derived from a later spec, so `add-version` never churns unrelated file headers. **The generated runtime code never parses this file** — avoiding a JSON-manifest-parsing dependency in 5 languages. Instead, each `add-version` call re-renders a handful of **marker-delimited regions** (`// mcpify:versions:begin` / `// mcpify:versions:end`, or `#`-commented for Python) directly via Rust string patching (`add_version::marker_region::patch_marked_region`), without reconstructing the full original Tera rendering context (auth schemes, project name, etc.) — those regions are always small, pure data (a label→file map, or a list of choices), never logic.

**`--set-default`** promotes a version to be the new default: demotes the outgoing default to its own suffixed files (preserving its data — never silently overwritten), ingests the new spec straight to the canonical paths, and re-renders the version-aware regions with the updated list. Promoting a label that was already `add-version`'d earlier as non-default (not brand new) is handled explicitly: its stale suffixed files are cleaned up only *after* the new data lands safely at the canonical paths.

## Part CORE — CLI, ledger, and the add-version pipeline

**Done:**
- `src/cli.rs`: `Cli` gained `#[command(subcommand)] command: Option<Commands>` (`None` = today's flat `generate` invocation, unchanged) and a new `AddVersion { project, version, input, set_default, force }` subcommand. `input`/`output` became `Option` at the clap level (required only for `generate`), validated via a new `Cli::into_generate_args()` — the only touch to pre-existing behavior. `generate` gained an optional `--api-version` flag (named to avoid colliding with clap's auto `-V/--version`), defaulting to the sentinel `"default"`.
- New `src/add_version/` module: `ledger.rs` (schema + atomic read/write of `.mcpify/versions.json`), `marker_region.rs` (the patch primitive, including reindenting a flush-left body to match an indented marker — needed for Python's `config.py`, whose marker sits inside a class body), `demote.rs` (`--set-default`'s file-level demotion logic), `sync.rs` (dispatches to each target's `steps::versions::sync`, plus `version_entries_from_ledger`), `seed.rs` (writes the ledger's first entry right after a successful `generate`), and `mod.rs` (`run()` orchestration: `add_non_default_version` / `promote_to_default`).
- `src/context.rs`: `GeneratorContext` gained `version_label`; new shared `VersionEntryView` (label, db_file, schemas_file, var_suffix) used by every target's template context — `var_suffix` is an identifier-safe form of the label (`"10.2.14"` → `"v10_2_14"`), needed only by Go's `//go:embed` (each directive binds to one var, so labels like `"11.2"` can't be used directly as Go identifiers) but computed once here so `generate`-time Tera rendering and `add-version`'s Rust-side re-rendering always agree.
- `src/db/mod.rs`: `assemble_store` extracted into a reusable `assemble_store_at(path, should_clear, operations)`, called directly by `add-version` for a version-suffixed path.
- New `src/schemas_asset.rs`: the "generated schemas" JSON writer, previously duplicated per target, hoisted into one shared function (`write_schemas_json_at`) — identical shape across all 5 targets, only the destination path differs.
- `main.rs`: dispatches `Some(Commands::AddVersion{..})` to `add_version::run`, `None` to the unchanged `generate` path (which now also calls `seed::seed_ledger_after_generate` on success).

**Verified:** unit tests for the ledger (round-trip, missing-file error, schema-version mismatch, insertion-order preservation), the marker patcher (region replacement, indentation matching, idempotency, missing-marker error), demotion (fresh demotion, self-promotion no-op, re-promoting an already-added label, clobber guard with/without `--force`), and the CLI subcommand parsing.

---

## Part TYPESCRIPT — reference implementation

**Done:** `core/config-schema.ts.tera` (`ApiVersionSchema` enum + `DEFAULT_API_VERSION` const, both marker-wrapped), `core/config-manager.ts.tera` (`api_version` env-cascade entry), `data/store-repository.ts.tera` (`VERSION_STORE_FILES` map, `openStore(apiVersion)`), `validation/validator.ts.tera` (per-version schemas file map, lazy per-`(version, operationId)` Ajv compilation), `cli/setup-wizard.ts.tera` (`promptApiVersion()`, silently returns the one label when there's only one), new `cli/versions-command.ts.tera` registered in `cli.ts.tera`. `steps::versions::sync` re-renders all 6 files' marker regions from a ledger.

**Verified:** full `cargo test --test e2e_generation generates_a_project_and_passes_its_own_test_suite -- --ignored` (real `npm install`/`tsc`/`vitest`); manual `generate` → `add-version` → `add-version --set-default` against the real Jira fixture URLs' shape, confirming `npm run build` stays clean and `versions` reports the right default/active labels; committed as a permanent test in `tests/e2e_multi_version.rs`.

---

## Part RUST — mirror the pattern, compile-time schemas

**Done:** `core/config_schema.rs.tera` (`api_version: String` field — a plain string, not a compile-time enum like `AuthMethod`, since version labels are arbitrary strings that can't cleanly become Rust identifiers), `data/store.rs.tera` (`resolve_store_path`), `validation/validator.rs.tera` (one `include_str!` per known version, keyed by version in a lookup function — compile-time embedding means **a `cargo build` is required after `add-version`** for a new version to take effect), `cli/setup_wizard.rs.tera`, new `cli/versions.rs.tera` registered in `main.rs.tera`. `call_tool.rs.tera`/`get.rs.tera`/`search.rs.tera`/`call.rs.tera` updated to resolve and thread `api_version` through.

**Verified:** `cargo test --test e2e_generation generates_a_rust_project_and_passes_its_own_test_suite -- --ignored`; manual `add-version` + `cargo build` + `versions` subcommand run against a real generated project.

---

## Part PYTHON — mirror the pattern, runtime schemas

**Done:** `core/config.py.tera` (`api_version` field on the pydantic `Config`, marker sits inside the class body), `data/store.py.tera` (`resolve_store_path`), `validation/validator.py.tera` (per-version schemas cache), `cli/setup_wizard.py.tera`, new `cli/versions.py.tera` registered in `cli/__init__.py.tera`.

**Bug found and fixed:** the marker-patching primitive (`patch_marked_region`) originally assumed markers sit at column 0, matching every other target — but Python's `config.py` marker is indented inside `class Config(BaseSettings):`. A flush-left replacement body silently produced an `IndentationError` in the generated file. Fixed generically in `marker_region.rs` (not just for Python): the patcher now captures the *whitespace-only* prefix of the begin-marker's line and re-applies it to every line of the replacement body, so target languages with indentation-sensitive marker placement are handled automatically.

**Verified:** `cargo test --test e2e_generation generates_a_python_project_and_passes_its_own_test_suite -- --ignored` (real `uv sync`/`pytest`/`ruff`); manual `add-version` run confirming `config.py`'s indentation stayed correct and `uv run python -m ...versions` reported the right labels.

---

## Part CSHARP — mirror the pattern, embedded-resource schemas

**Done:** `Core/Config.cs.tera` (`ApiVersion` property; the tier-7 built-in-defaults dictionary deliberately does *not* also carry a default for this field, unlike every other optional field — keeping the default in exactly one marker-wrapped place avoids two copies drifting out of sync across an `add-version --set-default`), `Data/SqliteVecStore.cs.tera` (`ResolveStorePath`, DI registration in `Core/DataStore.cs.tera` switched from `AddSingleton<SqliteVecStore>()` to a factory resolving `McpifyOptions.ApiVersion`), `Validation/Validator.cs.tera` (per-version embedded-resource cache), `Tools/McpTools.cs.tera`'s `Call` gained an `IOptions<McpifyOptions>` parameter, `Cli/SetupWizard.cs.tera`, new `Cli/VersionsCommand.cs.tera`. `Project.csproj.tera`'s `<EmbeddedResource>` switched from one explicit `Include` to a glob (`Validation\GeneratedSchemas*.json`) so a later `add-version`'s new schemas file is picked up on the next build without the `.csproj` itself needing to be in the marker-patched set.

**Verified:** `cargo test --test e2e_generation generates_a_csharp_project_and_passes_its_own_test_suite -- --ignored`; manual `add-version` + `dotnet build` + `versions` subcommand run.

---

## Part GO — mirror the pattern, one `go:embed` per version

**Done:** `internal/core/config.go.tera` (`ApiVersion` field, `defaultAPIVersion()` marker-wrapped), `internal/data/store.go.tera` (`ResolveStorePath`), `internal/validation/validator.go.tera` (one `//go:embed` directive + var per known version — a directive binds to exactly one var, so multiple versions need multiple embed+var pairs rather than a shared one; each var's name is sanitized via the new `var_suffix` field), `internal/cli/setup.go.tera`, new `internal/cli/versions.go.tera` registered in `cmd/main.go.tera`. `internal/tools/call.go.tera`/`register.go.tera` gained an `apiVersion` parameter threaded from `internal/cli/roles.go.tera`.

**Bug found and fixed:** the initial marker-region bodies (both the `.tera` templates and the Rust `steps::versions` body-renderers) weren't `gofmt`-clean — `gofmt -l` flagged every version-aware file. Two distinct causes: (1) `gofmt` wants a blank line immediately before a marker/doc comment that follows a declaration, which the raw string bodies didn't include; (2) `gofmt` treats a `//go:embed` compiler directive as needing to be visually separated from any prose comment directly above it (here, the `// mcpify:versions:begin` marker itself), otherwise it rewrites the file to force that separation — fixed by starting the validator's marker region with a blank line. Caught by writing a throwaway integration test that ran the real Go steps end-to-end (bypassing the `golangci-lint`-dependent `run_generated_tests`, unavailable in this sandbox) and checking `gofmt -l`/`go build`/`go vet` directly; not caught by golden snapshots or unit tests alone, since neither runs an actual formatter.

**Verified:** `gofmt -l`, `go build ./...`, `go vet ./...` all clean on a real two-version generated project (both right after `generate` alone and after `add-version` patched the marker regions); manual `go build` + `versions` subcommand run confirming runtime output. The real `cargo test --test e2e_generation generates_a_go_project_and_passes_its_own_test_suite -- --ignored` and `tests/go_build_smoke.rs` both require `golangci-lint`, not installed in this environment — pre-existing limitation, unrelated to this feature (confirmed by the same test already failing the same way before this work).

---

## Cross-cutting notes

- **Compile-time vs. runtime schemas.** TypeScript and Python read their schemas asset from disk at runtime — `add-version` alone is enough, no rebuild needed. Rust, C#, and Go bake it in at compile time (`include_str!`/embedded resource/`go:embed`), so a rebuild is required after `add-version` before a newly added version actually works — a real, documented asymmetry between targets, not an oversight.
- **Golden/snapshot tests** were updated for every target's new/changed template output (the single-version case, since that's what `generate`'s own golden tests exercise) via `INSTA_UPDATE=always`, reviewed by diff.
- **`steps::versions` unit tests** (one per target) directly assert on the Rust-rendered marker-region content for a 2-version ledger, serving the same "does re-rendering actually produce correct code" role a dedicated multi-version golden-snapshot suite would, without a second parallel snapshot mechanism.
- Every target's `add-version` re-renders exactly the same 5-6 files: a config/schema file, the data-layer store-path resolver, the validator's schemas-file resolver, the setup wizard's version prompt, and a new `versions` CLI subcommand — never auth strategies, enterprise scaffolding, transports, or the test suite, which are all version-independent.

## Verification

- `cargo build`, `cargo fmt --check`, `cargo clippy --all-targets` on mcpify's own codebase — clean.
- Full `cargo test` (317 unit tests + all golden/snapshot tests) — green.
- `cargo test --test e2e_generation -- --ignored` for TypeScript, Rust, Python, and C# — all green (Go's requires `golangci-lint`, unavailable in this sandbox; verified instead via direct `gofmt`/`go build`/`go vet`).
- `cargo test --test e2e_multi_version -- --ignored` — the new, permanent, real acceptance test: generates a TypeScript project, adds two more versions (one plain, one `--set-default` promoting a fresh label), confirms the demoted version's original data survives intact via a direct SQLite query, rebuilds with `npm run build`, and confirms the generated project's own `versions` command reports the right default/active labels.
- Manual `add-version`/`add-version --set-default` runs via the real `mcpify` binary against all 5 targets, each followed by that target's own build tool (`npm run build`, `cargo build`, `uv run`, `dotnet build`, `go build`) and its generated `versions` subcommand, confirming end-to-end correctness beyond what the automated tests check.
