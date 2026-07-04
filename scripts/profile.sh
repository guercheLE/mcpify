#!/usr/bin/env bash
# CPU profiling for mcpify's own Rust codebase (not the generated servers').
# Uses samply (not cargo-flamegraph/perf) so this works on both macOS and
# Linux without sudo/dtrace friction, matching the CI matrix in
# .github/workflows/ci.yml. See docs/profiling.md.
set -euo pipefail

if ! command -v samply >/dev/null 2>&1; then
  echo "error: samply not found. Install it with:" >&2
  echo "  cargo install samply" >&2
  echo "  samply setup   # macOS only: codesigns samply for process attach" >&2
  exit 1
fi

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

FIXTURE="${MCPIFY_PROFILE_FIXTURE:-tests/fixtures/openapi/minimal-multi-scheme.yaml}"
LANGUAGE="${MCPIFY_PROFILE_LANGUAGE:-typescript}"
OUT_DIR="$(mktemp -d)"
trap 'rm -rf "$OUT_DIR"' EXIT

mkdir -p target/profile
cargo build --release

echo "Profiling: mcpify -i $FIXTURE -o $OUT_DIR -l $LANGUAGE -f"
echo "(first run of a given target is slow — it downloads/builds the generated"
echo "project's own dependencies as part of the run_generated_tests gate; reruns"
echo "reuse warmed caches)"


# The profiled command's own exit status is irrelevant here — this script's
# job is "did we capture a profile," not "did generation succeed" (that's
# run_generated_tests's job, in the real pipeline). Don't let `set -e` abort
# the script just because the profiled command itself failed/exited nonzero.
set +e
samply record --save-only --unstable-presymbolicate -o target/profile/profile.json.gz -- \
  ./target/release/mcpify -i "$FIXTURE" -o "$OUT_DIR" -l "$LANGUAGE" -f
samply_status=$?
set -e
if [ "$samply_status" -ne 0 ]; then
  echo "note: the profiled 'mcpify' invocation exited non-zero ($samply_status) —" >&2
  echo "still profiled fine; this just means run_generated_tests failed for the" >&2
  echo "fixture/language used. A profile was captured regardless." >&2
fi

python3 scripts/samply_to_text.py \
  target/profile/profile.json.gz \
  target/profile/folded-stacks.txt \
  target/profile/top-functions.txt

echo
echo "Profile written to:"
echo "  target/profile/profile.json.gz      (interactive — samply load target/profile/profile.json.gz)"
echo "  target/profile/folded-stacks.txt    (LLM/tool-ingestable: func;func;func count)"
echo "  target/profile/top-functions.txt    (LLM/tool-ingestable: top functions by self-time)"
