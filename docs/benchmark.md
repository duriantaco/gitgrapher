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

## GitGrapher Commands

Build the release binary:

```bash
cargo build --release --bin gitgrapher
```

Or run the helper:

```bash
scripts/benchmark.sh /path/to/repo
```

Start from a clean index:

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
touch /path/to/repo/src/some-file.ts
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
