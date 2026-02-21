#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"

THOUGHT_BENCH_BIN="${THOUGHT_BENCH_BIN:-$REPO_ROOT/target/release/thought}"
BENCH_ARTICLES="${BENCH_ARTICLES:-300}"
BENCH_PARENT="${THOUGHT_BENCH_WORKSPACE_PARENT:-${RUNNER_TEMP:-$REPO_ROOT/target}}"
BENCH_NAME="${THOUGHT_BENCH_WORKSPACE_NAME:-thought-bench}"
THOUGHT_BENCH_WORKSPACE="$BENCH_PARENT/$BENCH_NAME"

if [[ ! -x "$THOUGHT_BENCH_BIN" ]]; then
  echo "Thought benchmark binary is missing or not executable: $THOUGHT_BENCH_BIN" >&2
  exit 1
fi

if [[ "$BENCH_ARTICLES" -lt 1 ]]; then
  echo "BENCH_ARTICLES must be >= 1, got: $BENCH_ARTICLES" >&2
  exit 1
fi

rm -rf "$THOUGHT_BENCH_WORKSPACE"
mkdir -p "$BENCH_PARENT"

(
  cd "$BENCH_PARENT"
  "$THOUGHT_BENCH_BIN" create "$BENCH_NAME"
)

for idx in $(seq 1 "$BENCH_ARTICLES"); do
  (
    cd "$THOUGHT_BENCH_WORKSPACE"
    "$THOUGHT_BENCH_BIN" article create "Benchmark Article $idx"
  )
done

if [[ -n "${GITHUB_ENV:-}" ]]; then
  echo "THOUGHT_BENCH_WORKSPACE=$THOUGHT_BENCH_WORKSPACE" >> "$GITHUB_ENV"
fi

printf '%s\n' "$THOUGHT_BENCH_WORKSPACE"
