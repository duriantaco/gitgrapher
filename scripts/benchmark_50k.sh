#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
cd "$repo_root"

out="${1:-}"
fixture="${GITGRAPHER_BENCHMARK_FIXTURE:-/private/tmp/gitgrapher-50k}"
files="${GITGRAPHER_BENCHMARK_FILES:-500}"
functions_per_file="${GITGRAPHER_BENCHMARK_FUNCTIONS_PER_FILE:-100}"
bin="${GITGRAPHER_BIN:-target/release/gitgrapher}"

ensure_fixture_commit() {
  if [[ -d "$fixture/.git" ]] && git -C "$fixture" rev-parse --verify HEAD >/dev/null 2>&1; then
    return
  fi
  if ! command -v git >/dev/null 2>&1; then
    echo "git is required to record a fixture commit" >&2
    exit 69
  fi
  git -C "$fixture" init -q
  git -C "$fixture" add package.json src
  GIT_AUTHOR_DATE="2026-01-01T00:00:00Z" \
    GIT_COMMITTER_DATE="2026-01-01T00:00:00Z" \
    git -C "$fixture" \
      -c user.name="GitGrapher Benchmark" \
      -c user.email="benchmark@gitgrapher.local" \
      commit -qm "Generate large TypeScript benchmark fixture"
}

if [[ ! -d "$fixture/src" ]]; then
  scripts/make_large_fixture.sh "$fixture" "$files" "$functions_per_file" >&2
else
  actual_files="$(find "$fixture/src" -maxdepth 1 -type f -name '*.ts' | wc -l | tr -d ' ')"
  if [[ "$actual_files" != "$files" ]]; then
    echo "fixture $fixture has $actual_files TypeScript files, expected $files" >&2
    exit 65
  fi
  ensure_fixture_commit
fi

cargo build --release --locked --bin gitgrapher >&2

if [[ -n "$out" ]]; then
  mkdir -p "$(dirname "$out")"
  "$bin" benchmark --format json "$fixture" > "$out"
else
  "$bin" benchmark --format json "$fixture"
fi
