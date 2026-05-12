# Benchmark Method

The README performance table should stay reproducible. Use this file to record the repository, commit, machine, command, and environment used for each published benchmark.

<!-- benchmark-detail:start -->
## Large Synthetic Fixture

Generated from [benchmarks/gitgrapher-50k-macos-aarch64.json](../benchmarks/gitgrapher-50k-macos-aarch64.json) with `scripts/update_benchmark_docs.py`.

Fixture:

- Path: `/private/tmp/gitgrapher-50k`
- Files: 500 TypeScript files
- Functions: 50,000 exported functions
- Generator: `scripts/make_large_fixture.sh /private/tmp/gitgrapher-50k 500 100`

Commands:

```bash
scripts/benchmark_50k.sh benchmarks/gitgrapher-50k-macos-aarch64.json
python3 scripts/update_benchmark_docs.py
```

The helper builds the current release binary and runs `target/release/gitgrapher benchmark --format json /private/tmp/gitgrapher-50k`.

Environment:

| Field | Value |
|-------|-------|
| Recorded at | 2026-05-12 14:11:48Z |
| OS/arch | macos aarch64 |
| Logical CPUs | 8 |
| Worker threads | 7 |
| Rust | rustc 1.86.0 (05f9846f8 2025-03-31) |
| GitGrapher | 0.1.0 |
| Repo commit | bea1c30d45a028b4c3e000b9591b513084b4dbde |

Result:

| Run | Time | Files scanned | Nodes | Edges | Graph size | Peak RSS |
|-----|-----:|--------------:|------:|------:|-----------:|---------:|
| Cold | 4.54s | 500 | 50,502 | 50,001 | 40.3 MB | 395.3 MB |
| No-change incremental | 0.23s | 0 | 50,502 | 50,001 | 40.3 MB | 395.3 MB |
| One-file incremental | 0.48s | 1 | 50,502 | 50,001 | 40.3 MB | 395.3 MB |

Raw JSON command output:

```json
{
  "schema_version": 1,
  "generated_at_unix": 1778595108,
  "gitgrapher_version": "0.1.0",
  "machine": {
    "os": "macos",
    "arch": "aarch64",
    "logical_cpus": 8,
    "worker_threads": 7,
    "rustc_version": "rustc 1.86.0 (05f9846f8 2025-03-31)",
    "peak_rss_source": "getrusage(RUSAGE_SELF).ru_maxrss"
  },
  "repository": {
    "path": "/private/tmp/gitgrapher-50k",
    "git_commit": "bea1c30d45a028b4c3e000b9591b513084b4dbde"
  },
  "sample_file": "src/module_0001.ts",
  "runs": [
    {
      "name": "cold",
      "duration_ms": 4541.403875,
      "files_scanned": 500,
      "total_nodes": 50502,
      "total_edges": 50001,
      "graph_size_bytes": 40296231,
      "process_peak_rss_bytes": 395280384,
      "node_counts_by_label": {
        "File": 500,
        "Folder": 1,
        "Function": 50000,
        "Project": 1
      },
      "edge_counts_by_type": {
        "CONTAINS": 50001
      }
    },
    {
      "name": "no_change",
      "duration_ms": 227.39483299999998,
      "files_scanned": 0,
      "total_nodes": 50502,
      "total_edges": 50001,
      "graph_size_bytes": 40296231,
      "process_peak_rss_bytes": 395280384,
      "node_counts_by_label": {
        "File": 500,
        "Folder": 1,
        "Function": 50000,
        "Project": 1
      },
      "edge_counts_by_type": {
        "CONTAINS": 50001
      }
    },
    {
      "name": "one_file_incremental",
      "duration_ms": 484.604791,
      "files_scanned": 1,
      "total_nodes": 50502,
      "total_edges": 50001,
      "graph_size_bytes": 40296231,
      "process_peak_rss_bytes": 395280384,
      "node_counts_by_label": {
        "File": 500,
        "Folder": 1,
        "Function": 50000,
        "Project": 1
      },
      "edge_counts_by_type": {
        "CONTAINS": 50001
      }
    }
  ]
}
```
<!-- benchmark-detail:end -->

## GitGrapher Commands

Build the release binary:

```bash
cargo build --release --bin gitgrapher
```

Or run the helper:

```bash
scripts/benchmark.sh /path/to/repo
GITGRAPHER_BENCHMARK_FORMAT=json scripts/benchmark.sh /path/to/repo
scripts/benchmark_50k.sh benchmarks/gitgrapher-50k-macos-aarch64.json
```

Or use the built-in benchmark command, which temporarily backs up any existing `.gitgrapher` index, runs cold/no-change/one-file incremental indexing, restores the touched source file, and restores the previous index:

```bash
target/release/gitgrapher benchmark --format json /path/to/repo
```

For a synthetic large TypeScript fixture:

```bash
scripts/make_large_fixture.sh /tmp/gitgrapher-50k 500 100
target/release/gitgrapher benchmark --format json /tmp/gitgrapher-50k
```

The command reports elapsed time, files scanned, nodes, edges, graph size, and process peak RSS where supported by the OS.

Manual cold index timing:

```bash
rm -rf /path/to/repo/.gitgrapher
/usr/bin/time -lp target/release/gitgrapher analyze /path/to/repo
```

Run the no-change incremental path:

```bash
/usr/bin/time -lp target/release/gitgrapher analyze /path/to/repo
```

Run the one-file incremental path:

```bash
printf '\n// benchmark mutation\n' >> /path/to/repo/src/some-file.ts
/usr/bin/time -lp target/release/gitgrapher analyze /path/to/repo
```

On Linux, use `-v` instead of `-lp`:

```bash
/usr/bin/time -v target/release/gitgrapher analyze /path/to/repo
```

## Reporting Checklist

Record:

- GitGrapher commit
- target repository URL or private fixture description
- target repository commit
- OS, CPU, RAM, and storage type
- `rustc --version`
- `gitgrapher --version`
- command output and `/usr/bin/time` output

Do not update README numbers without updating this file.
