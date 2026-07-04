#!/usr/bin/env bash
# Coverage for mcpify's own Rust codebase (not the generated servers').
# Produces both machine-readable (lcov, json) and human-readable (html)
# reports under target/coverage/. See docs/profiling.md.
set -euo pipefail

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
  echo "error: cargo-llvm-cov not found. Install it with:" >&2
  echo "  cargo install cargo-llvm-cov" >&2
  echo "  rustup component add llvm-tools-preview" >&2
  exit 1
fi

cd "$(git rev-parse --show-toplevel)"
mkdir -p target/coverage

cargo llvm-cov --workspace --lcov --output-path target/coverage/lcov.info
cargo llvm-cov report --json --output-path target/coverage/coverage.json
cargo llvm-cov report --html --output-dir target/coverage

echo "Coverage written to:"
echo "  target/coverage/lcov.info   (machine-readable, LLM/tool-ingestable)"
echo "  target/coverage/coverage.json"
echo "  target/coverage/html/index.html (human-readable)"
