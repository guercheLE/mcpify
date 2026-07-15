# Project manifest

`mcpify sync --manifest mcpify.yaml` makes generation reproducible across all
five targets: Rust, TypeScript, Python, C#, and Go. Paths in the manifest are
resolved relative to the manifest file.

```yaml
language: rust
output: ./widget-mcp
publish_registry: true

publication:
  license: MIT
  repository: https://github.com/example/widget-mcp
  readme: README.md
  authors: [Example Org]
  keywords: [mcp, widget]
  categories: [command-line-utilities]
  exclude: [tests/fixtures/**]

default_headers:
  Accept: application/vnd.widget+json
  # User-Agent is generated automatically when omitted.

auth:
  - name: personalAccessToken
    kind: pat
  - name: tenantKey
    kind: api-key
    location: header
    parameter_name: X-Tenant-Key

versions:
  - version: 1.2.0
    source: specs/1.2.0.yaml
  - version: 1.2.1
    source: specs/1.2.1.yaml
    default: true
    preprocess:
      - command: ./scripts/normalize-spec
        args: [--input, "{input}"]

version_policy:
  mode: latest-per-minor

package_size_limit_mb: 12
```

Exactly one version must have `default: true`. Supported policies are `all`,
`latest-per-minor`, and `allowlist` (with a `versions` list). A policy may not
exclude the default version. `latest-per-minor` requires semantic-version
labels.

Preprocessors are executed directly, without a shell. `{input}` in an argument
is replaced with a temporary local source path; when it is omitted, the path is
appended as the final argument. The command must write the transformed spec to
stdout or update the input file. This hook is intended for controlled,
project-owned transformations; Swagger 2 conversion and missing parameter
schema repair are built in and require no hook.

The generated project records canonical settings in `.mcpify/settings.yaml`,
the version ledger in `.mcpify/versions.json`, and readable source provenance in
`docs/SCHEMA_VERSIONS.md`.

Registry publication requires `publication.license`, `publication.repository`,
and at least one `publication.authors` entry. mcpify generates ecosystem metadata and a `LICENSE`
file, runs the ecosystem's package/dry-run validation, and enforces the package
size limit with the largest source contributors in any failure report. Rust
projects additionally receive cargo-dist configuration with the supported ONNX
Runtime target matrix and a separate crates.io publishing workflow.
