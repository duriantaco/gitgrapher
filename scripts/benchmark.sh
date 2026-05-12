#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/benchmark.sh /path/to/repo" >&2
  exit 64
fi

repo="$1"
bin="${GITGRAPHER_BIN:-target/release/gitgrapher}"

if [[ ! -x "$bin" ]]; then
  cargo build --release --bin gitgrapher
fi

echo "GitGrapher: $("$bin" --version)"
echo "Repository: $repo"
echo

format="${GITGRAPHER_BENCHMARK_FORMAT:-text}"
"$bin" benchmark --format "$format" "$repo"
