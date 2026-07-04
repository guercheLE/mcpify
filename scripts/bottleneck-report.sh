#!/usr/bin/env bash
# Runs scripts/coverage.sh + scripts/profile.sh and assembles the results
# into one small markdown file meant to be pasted into an LLM (or handed to
# another tool) to answer "what should I optimize or test next in mcpify
# itself." See docs/profiling.md.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

./scripts/coverage.sh
./scripts/profile.sh

REPORT=target/bottleneck-report.md
{
  echo "# mcpify generator bottleneck report"
  echo
  echo "Generated $(date -u +%Y-%m-%dT%H:%M:%SZ)."
  echo
  echo "## Coverage gaps"
  echo
  echo '```'
  # lcov.info SF: lines mark each source file; DA: lines are per-line hit
  # counts. Summarize files with the lowest hit ratio so the list stays
  # short and readable, not a full per-line dump.
  awk '
    /^SF:/ { file=substr($0,4); hit=0; total=0 }
    /^DA:/ { total++; split($0,a,","); if (a[2]+0 > 0) hit++ }
    /^end_of_record/ {
      if (total > 0) {
        pct = 100*hit/total
        printf "%6.1f%% covered  %s  (%d/%d lines)\n", pct, file, hit, total
      }
    }
  ' target/coverage/lcov.info | sort -n | head -20
  echo '```'
  echo
  echo "Full report: target/coverage/lcov.info, target/coverage/html/index.html"
  echo
  echo "## Hottest functions (CPU, self-time sample count)"
  echo
  echo '```'
  tail -n +2 target/profile/top-functions.txt | head -20
  echo '```'
  echo
  echo "Full profile: target/profile/profile.json.gz (open with \`samply load\`), target/profile/folded-stacks.txt"
} > "$REPORT"

echo "Bottleneck report written to $REPORT"
