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

echo "Full index"
rm -rf "$repo/.gitgrapher"
/usr/bin/time -p "$bin" analyze "$repo"

echo
echo "No-change incremental"
/usr/bin/time -p "$bin" analyze "$repo"
