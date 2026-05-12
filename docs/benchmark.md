# Benchmark Method

The README performance table should stay reproducible. Use this file to record the repository, commit, machine, command, and environment used for each published benchmark.

## Current Published Result

Benchmark target: 1,555-file TypeScript codebase.

| Metric | GitGrapher | GitNexus (TypeScript) |
|--------|------------|----------------------|
| Full index | ~6s | ~45s |
| Incremental, 1 file changed | ~1.8s | ~45s |
| No changes | ~0.7s | ~45s |
| Memory | ~200MB | 2-4GB |
| Max file size | 32MB | 512KB |

## Large Synthetic Fixture

Generated with:

```bash
scripts/make_large_fixture.sh /private/tmp/gitgrapher-50k 500 100
cargo build --release --bin gitgrapher
target/release/gitgrapher benchmark --format json /private/tmp/gitgrapher-50k
```

Environment:

- Date: 2026-05-12
- OS/arch: macOS aarch64
- Worker threads: 7
- Fixture: 500 TypeScript files, 50,000 exported functions

Result:

| Run | Time | Files scanned | Nodes | Edges | Graph size | Peak RSS |
|-----|------|---------------|-------|-------|------------|----------|
| Cold | 3.04s | 500 | 50,502 | 50,001 | 40.3MB | 331.8MB |
| No-change incremental | 0.17s | 0 | 50,502 | 50,001 | 40.3MB | 331.8MB |
| One-file incremental | 0.34s | 1 | 50,502 | 50,001 | 40.3MB | 345.4MB |

Raw JSON:

```json
{
  "repo": "/private/tmp/gitgrapher-50k",
  "git_commit": null,
  "gitgrapher_version": "0.1.0",
  "os": "macos",
  "arch": "aarch64",
  "worker_threads": 7,
  "sample_file": "src/module_0001.ts",
  "runs": [
    {
      "name": "cold",
      "duration_ms": 3035.227125,
      "files_scanned": 500,
      "total_nodes": 50502,
      "total_edges": 50001,
      "graph_size_bytes": 40296231,
      "process_peak_rss_bytes": 331825152
    },
    {
      "name": "no_change",
      "duration_ms": 170.791125,
      "files_scanned": 0,
      "total_nodes": 50502,
      "total_edges": 50001,
      "graph_size_bytes": 40296231,
      "process_peak_rss_bytes": 331825152
    },
    {
      "name": "one_file_incremental",
      "duration_ms": 337.827916,
      "files_scanned": 1,
      "total_nodes": 50502,
      "total_edges": 50001,
      "graph_size_bytes": 40296231,
      "process_peak_rss_bytes": 345391104
    }
  ]
}
```

## GitGrapher Commands

Build the release binary:

```bash
cargo build --release --bin gitgrapher
```

Or run the helper:

```bash
scripts/benchmark.sh /path/to/repo
GITGRAPHER_BENCHMARK_FORMAT=json scripts/benchmark.sh /path/to/repo
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
