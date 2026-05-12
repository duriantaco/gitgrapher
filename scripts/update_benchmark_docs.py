#!/usr/bin/env python3
"""Render benchmark tables from the committed benchmark JSON snapshot."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_SNAPSHOT = ROOT / "benchmarks" / "gitgrapher-50k-macos-aarch64.json"
README = ROOT / "README.md"
BENCHMARK_DOC = ROOT / "docs" / "benchmark.md"
SCRIPT_REL = "scripts/update_benchmark_docs.py"


def fmt_int(value: int) -> str:
    return f"{value:,}"


def fmt_duration(ms: float) -> str:
    return f"{ms / 1000:.2f}s"


def fmt_bytes(value: int | None) -> str:
    if value is None:
        return "n/a"
    return f"{value / 1_000_000:.1f} MB"


def run_by_name(report: dict[str, Any], name: str) -> dict[str, Any]:
    for run in report["runs"]:
        if run["name"] == name:
            return run
    raise SystemExit(f"benchmark snapshot is missing run: {name}")


def generated_at(report: dict[str, Any]) -> str:
    timestamp = report.get("generated_at_unix")
    if not isinstance(timestamp, int):
        return "unknown"
    return dt.datetime.fromtimestamp(timestamp, dt.timezone.utc).strftime(
        "%Y-%m-%d %H:%M:%SZ"
    )


def snapshot_stats(report: dict[str, Any]) -> dict[str, Any]:
    cold = run_by_name(report, "cold")
    no_change = run_by_name(report, "no_change")
    incremental = run_by_name(report, "one_file_incremental")
    functions = cold.get("node_counts_by_label", {}).get("Function")
    if not isinstance(functions, int):
        functions = cold["total_nodes"]
    files = cold["files_scanned"]
    peaks = [
        run.get("process_peak_rss_bytes")
        for run in report["runs"]
        if isinstance(run.get("process_peak_rss_bytes"), int)
    ]
    peak = max(peaks) if peaks else None
    per_file = functions // files if files and functions % files == 0 else None
    return {
        "cold": cold,
        "no_change": no_change,
        "incremental": incremental,
        "functions": functions,
        "files": files,
        "peak": peak,
        "functions_per_file": per_file,
    }


def replace_block(content: str, marker: str, body: str) -> str:
    start = f"<!-- {marker}:start -->"
    end = f"<!-- {marker}:end -->"
    pattern = re.compile(f"{re.escape(start)}.*?{re.escape(end)}", re.DOTALL)
    replacement = f"{start}\n{body.strip()}\n{end}"
    updated, count = pattern.subn(replacement, content)
    if count != 1:
        raise SystemExit(f"expected exactly one {marker} block")
    return updated


def render_readme_summary(report: dict[str, Any], snapshot_rel: str) -> str:
    stats = snapshot_stats(report)
    return f"""
| Benchmark fixture | Cold index | No changes | One-file incremental | Peak RSS |
|-------------------|-----------:|-----------:|---------------------:|---------:|
| {fmt_int(stats["functions"])} TypeScript functions across {fmt_int(stats["files"])} files | **{fmt_duration(stats["cold"]["duration_ms"])}** | **{fmt_duration(stats["no_change"]["duration_ms"])}** | **{fmt_duration(stats["incremental"]["duration_ms"])}** | {fmt_bytes(stats["peak"])} |

Generated from [{snapshot_rel}]({snapshot_rel}) with `{SCRIPT_REL}`.
"""


def render_benchmark_detail(report: dict[str, Any], snapshot_rel: str) -> str:
    stats = snapshot_stats(report)
    machine = report["machine"]
    repository = report["repository"]
    fixture_path = repository["path"]
    functions_per_file = stats["functions_per_file"] or "unknown"
    raw_json = json.dumps(report, indent=2)
    return f"""
## Large Synthetic Fixture

Generated from [{snapshot_rel}](../{snapshot_rel}) with `{SCRIPT_REL}`.

Fixture:

- Path: `{fixture_path}`
- Files: {fmt_int(stats["files"])} TypeScript files
- Functions: {fmt_int(stats["functions"])} exported functions
- Generator: `scripts/make_large_fixture.sh {fixture_path} {stats["files"]} {functions_per_file}`

Commands:

```bash
scripts/benchmark_50k.sh {snapshot_rel}
python3 {SCRIPT_REL}
```

The helper builds the current release binary and runs `target/release/gitgrapher benchmark --format json {fixture_path}`.

Environment:

| Field | Value |
|-------|-------|
| Recorded at | {generated_at(report)} |
| OS/arch | {machine["os"]} {machine["arch"]} |
| Logical CPUs | {machine.get("logical_cpus", "unknown")} |
| Worker threads | {machine["worker_threads"]} |
| Rust | {machine.get("rustc_version") or "unknown"} |
| GitGrapher | {report["gitgrapher_version"]} |
| Repo commit | {repository.get("git_commit") or "none"} |

Result:

| Run | Time | Files scanned | Nodes | Edges | Graph size | Peak RSS |
|-----|-----:|--------------:|------:|------:|-----------:|---------:|
| Cold | {fmt_duration(stats["cold"]["duration_ms"])} | {fmt_int(stats["cold"]["files_scanned"])} | {fmt_int(stats["cold"]["total_nodes"])} | {fmt_int(stats["cold"]["total_edges"])} | {fmt_bytes(stats["cold"].get("graph_size_bytes"))} | {fmt_bytes(stats["cold"].get("process_peak_rss_bytes"))} |
| No-change incremental | {fmt_duration(stats["no_change"]["duration_ms"])} | {fmt_int(stats["no_change"]["files_scanned"])} | {fmt_int(stats["no_change"]["total_nodes"])} | {fmt_int(stats["no_change"]["total_edges"])} | {fmt_bytes(stats["no_change"].get("graph_size_bytes"))} | {fmt_bytes(stats["no_change"].get("process_peak_rss_bytes"))} |
| One-file incremental | {fmt_duration(stats["incremental"]["duration_ms"])} | {fmt_int(stats["incremental"]["files_scanned"])} | {fmt_int(stats["incremental"]["total_nodes"])} | {fmt_int(stats["incremental"]["total_edges"])} | {fmt_bytes(stats["incremental"].get("graph_size_bytes"))} | {fmt_bytes(stats["incremental"].get("process_peak_rss_bytes"))} |

Raw JSON command output:

```json
{raw_json}
```
"""


def render(snapshot: Path, check: bool) -> int:
    report = json.loads(snapshot.read_text())
    snapshot_rel = snapshot.relative_to(ROOT).as_posix()

    replacements = {
        README: ("benchmark-summary", render_readme_summary(report, snapshot_rel)),
        BENCHMARK_DOC: ("benchmark-detail", render_benchmark_detail(report, snapshot_rel)),
    }

    stale: list[Path] = []
    for path, (marker, body) in replacements.items():
        current = path.read_text()
        updated = replace_block(current, marker, body)
        if current != updated:
            stale.append(path)
            if not check:
                path.write_text(updated)

    if check and stale:
        rels = ", ".join(path.relative_to(ROOT).as_posix() for path in stale)
        print(f"benchmark docs are stale: {rels}", file=sys.stderr)
        print(f"run: python3 {SCRIPT_REL}", file=sys.stderr)
        return 1

    if stale:
        rels = ", ".join(path.relative_to(ROOT).as_posix() for path in stale)
        print(f"updated benchmark docs: {rels}")
    else:
        print("benchmark docs are current")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--snapshot",
        type=Path,
        default=DEFAULT_SNAPSHOT,
        help="benchmark JSON snapshot to render from",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail if README/docs do not match the snapshot",
    )
    args = parser.parse_args()
    snapshot = args.snapshot
    if not snapshot.is_absolute():
        snapshot = ROOT / snapshot
    return render(snapshot, args.check)


if __name__ == "__main__":
    sys.exit(main())
