# Test organization

mcpify's own test suite (REQ-2.6.1) is independent of the generated
project's test suite (REQ-2.3.6, verified by `run_generated_tests` —
Story 14). This file only concerns the former.

## Where a test belongs

- **Unit tests** live inline in the module they test, in a
  `#[cfg(test)] mod tests { ... }` block at the bottom of the file. This is
  the default for anything that only exercises one module's own logic
  (parsing, classification, schema resolution, template rendering).
- **Integration tests** (`tests/*.rs`) exercise more than one module
  together through the crate's public API — `run_shared_pipeline`, the
  full `TypeScriptTargetGenerator::execute()` lifecycle, rollback behavior
  across the directory guard and target dispatch. They can only see `pub`
  items, same as an external caller would.
- **Golden/snapshot tests** (`tests/golden_typescript.rs`, snapshots under
  `tests/snapshots/`) assert that the TypeScript target's generation steps
  produce the same output for a fixture spec as a reviewed baseline —
  guarding against unintentional template drift. Review changes with
  `cargo insta review` after `cargo test --test golden_typescript`.

## Fixtures

`tests/fixtures/openapi/` holds small, hand-written specs covering the
scheme/schema shapes the generator needs to handle correctly:

| Fixture | Purpose |
| --- | --- |
| `minimal.yaml` / `minimal.json` | Baseline: one operation, no auth scheme |
| `minimal-with-auth.yaml` | One Basic auth scheme |
| `minimal-oauth2.json` | OAuth2 scheme, JSON format |
| `minimal-multi-scheme.yaml` | All four scheme kinds in one spec |
| `minimal-no-auth-scheme.json` | No `securitySchemes` — triggers the interactive-prompt fallback path |
| `widgets-with-refs.yaml` | `allOf` and a self-referential `$ref` — the schema resolver's highest-risk case |
| `malformed.yaml` | Neither valid JSON nor YAML — error path |

Prefer extending this list over inlining new fixture YAML/JSON as string
literals inside a test, when more than one test would benefit from the
same fixture.

## Coverage

`cargo llvm-cov` (via `cargo install cargo-llvm-cov`) is the recommended
local coverage tool; run `cargo llvm-cov --html` for a browsable report.
Not currently wired into CI as a hard threshold — CI (Story 17) fails the
build on any test failure, which is the enforced bar; coverage is a
supplementary signal for contributors to check locally.
