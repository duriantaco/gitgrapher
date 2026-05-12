# GitGrapher

[![CI](https://github.com/duriantaco/gitgrapher/actions/workflows/ci.yml/badge.svg)](https://github.com/duriantaco/gitgrapher/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Fast, local code intelligence for developers and AI coding agents.

GitGrapher indexes a repository into a persistent knowledge graph so you can search symbols, inspect callers and callees, trace blast radius, visualize architecture, and compare graph-aware diffs between revisions.

## Why GitGrapher

- **Native speed**: Rust, Tree-sitter, Rayon, Tantivy, and xxhash for fast indexing and search.
- **Incremental by default**: unchanged files are skipped instead of re-indexing the whole repo.
- **Local and portable**: graph data lives in `.gitgrapher/` inside the repository.
- **Permissive license**: MIT, including commercial use.
- **Useful today**: CLI search, context, impact, symbol listing, MCP tools, 3D graph export, and diff graphs.

## Install

Download the archive for your platform from the [GitHub releases](https://github.com/duriantaco/gitgrapher/releases), then put the `gitgrapher` binary on your `PATH`.

On macOS or Linux:

```bash
tar -xzf gitgrapher-vX.Y.Z-<platform>.tar.gz
install -m 755 gitgrapher-vX.Y.Z-<platform>/gitgrapher /usr/local/bin/gitgrapher
gitgrapher --version
```

Or install from source:

```bash
cargo install --git https://github.com/duriantaco/gitgrapher gitgrapher
```

Or build from source:

```bash
git clone https://github.com/duriantaco/gitgrapher
cd gitgrapher
cargo build --release
./target/release/gitgrapher --help
```

## Quick Start

```bash
# Index a repository
gitgrapher analyze /path/to/repo

# Search and inspect code structure
gitgrapher query "UserService" -p /path/to/repo
gitgrapher context handleLogin -p /path/to/repo
gitgrapher impact UserService -d up -p /path/to/repo
gitgrapher symbols -l class -p /path/to/repo

# Benchmark cold, no-change, and one-file incremental indexing
gitgrapher benchmark --format json /path/to/repo

# Expose the graph to AI coding agents over MCP
gitgrapher mcp

# Visualize in 3D
gitgrapher serve -p /path/to/repo

# Compare HEAD against the current worktree
gitgrapher diff --format html -p /path/to/repo
```

## Performance

<!-- benchmark-summary:start -->
| Benchmark fixture | Cold index | No changes | One-file incremental | Peak RSS |
|-------------------|-----------:|-----------:|---------------------:|---------:|
| 50,000 TypeScript functions across 500 files | **4.54s** | **0.23s** | **0.48s** | 395.3 MB |

Generated from [benchmarks/gitgrapher-50k-macos-aarch64.json](benchmarks/gitgrapher-50k-macos-aarch64.json) with `scripts/update_benchmark_docs.py`.
<!-- benchmark-summary:end -->

See [docs/benchmark.md](docs/benchmark.md) for the recorded JSON, fixture details, and reproduction commands.

## What It Indexes

GitGrapher tracks:

- **Symbols**: functions, classes, methods, interfaces, enums, properties
- **Relationships**: calls, imports, extends, implements, contains
- **Communities**: related code clusters detected with Louvain community detection
- **Processes**: execution flows traced from entry points through call chains
- **Cross-file links**: import paths and call targets resolved across files

## Supported Languages

- **TypeScript**: `.ts`, `.tsx`, `.mts`, `.cts`
- **JavaScript**: `.js`, `.jsx`, `.mjs`, `.cjs`
- **Python**: `.py`, `.pyi`

More languages are tracked in [ROADMAP.md](ROADMAP.md).

## CLI Commands

| Command | Description |
|---------|-------------|
| `analyze [path]` | Index a repository, incrementally when possible |
| `query <term>` | BM25 full-text search across symbols |
| `context <symbol>` | Show a symbol's callers, callees, and nearby relationships |
| `impact <symbol>` | Traverse upstream or downstream blast radius |
| `symbols [-l type]` | List symbols, optionally filtered by type |
| `status [path]` | Show index stats |
| `benchmark [path]` | Measure cold, no-change, and one-file incremental indexing |
| `mcp` | Start a stdio MCP server for AI coding agents |
| `list` | List indexed repositories |
| `serve [-P port]` | Start a local 3D graph viewer |
| `export [-f format]` | Export graph as HTML, JSON, or DOT |
| `diff [--base rev] [--head rev]` | Compare two git revisions or `HEAD` to `WORKTREE` |
| `clean [path]` | Remove the index for a repository |

## Architecture

```
crates/
|-- gg-core/       # Types, config, error handling
|-- gg-parse/      # Tree-sitter parsing
|-- gg-resolve/    # Cross-file import/call/type resolution
|-- gg-graph/      # Graph storage with persistence
|-- gg-search/     # Tantivy BM25 full-text search
|-- gg-pipeline/   # Scan -> parse -> resolve -> community -> process
|-- gg-napi/       # Node.js native bindings, experimental
`-- gg-cli/        # CLI interface
```

Core design choices:

- **Rust core** for CPU-bound parsing, graph operations, and search
- **Tree-sitter native bindings** instead of browser/WASM parsing for the CLI
- **Rayon** for parallel file parsing
- **Tantivy** for BM25 search
- **xxhash** for incremental file change detection

## AI Agent Integrations

GitGrapher ships a stdio MCP server:

```bash
gitgrapher mcp
```

The server exposes `query`, `context`, `impact`, and `list_repos` tools, plus JSON resources for indexed repositories and graph data. Point Claude Code, Cursor, or another MCP client at the `gitgrapher mcp` command after indexing a repository with `gitgrapher analyze`.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Release and packaging notes are in [docs/releasing.md](docs/releasing.md).
Scaling notes and large-fixture benchmark instructions are in [docs/scaling.md](docs/scaling.md).

## Contributing

PRs are welcome, especially for language providers, graph resolution, and CLI usability. See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT
