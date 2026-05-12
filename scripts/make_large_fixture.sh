#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 || $# -gt 3 ]]; then
  echo "usage: scripts/make_large_fixture.sh /path/to/output [files=500] [functions_per_file=100]" >&2
  exit 64
fi

out="$1"
files="${2:-500}"
functions_per_file="${3:-100}"

if [[ -e "$out" && -n "$(find "$out" -mindepth 1 -maxdepth 1 2>/dev/null | head -n 1)" ]]; then
  echo "refusing to write into non-empty directory: $out" >&2
  exit 65
fi

mkdir -p "$out/src"

cat > "$out/package.json" <<'JSON'
{
  "name": "gitgrapher-large-fixture",
  "private": true,
  "type": "module"
}
JSON

for i in $(seq 1 "$files"); do
  file="$out/src/module_$(printf '%04d' "$i").ts"
  {
    printf 'export const moduleId_%04d = %d;\n' "$i" "$i"
    for j in $(seq 1 "$functions_per_file"); do
      printf 'export function fn_%04d_%04d(input: number): number { return input + %d + %d; }\n' "$i" "$j" "$i" "$j"
    done
  } > "$file"
done

if command -v git >/dev/null 2>&1; then
  git -C "$out" init -q
  git -C "$out" add package.json src
  GIT_AUTHOR_DATE="2026-01-01T00:00:00Z" \
    GIT_COMMITTER_DATE="2026-01-01T00:00:00Z" \
    git -C "$out" \
      -c user.name="GitGrapher Benchmark" \
      -c user.email="benchmark@gitgrapher.local" \
      commit -qm "Generate large TypeScript benchmark fixture"
fi

total=$((files * functions_per_file))
echo "created $out with $files files and $total exported functions"
