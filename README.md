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
- **Useful today**: CLI search, context, impact, symbol listing, 3D graph export, and diff graphs.

## Install

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

# Visualize in 3D
gitgrapher serve -p /path/to/repo

# Compare HEAD against the current worktree
gitgrapher diff --format html -p /path/to/repo
```

## Performance

Benchmarked against a 1,555-file TypeScript codebase:

| Metric | GitGrapher | GitNexus (TypeScript) |
|--------|------------|----------------------|
| Full index | **~6s** | ~45s |
| Incremental (1 file changed) | **~1.8s** | ~45s (no incremental) |
| No changes | **~0.7s** | ~45s (re-indexes anyway) |
| Memory | ~200MB | 2-4GB (V8 heap) |
| Max file size | 32MB | 512KB |

See [docs/benchmark.md](docs/benchmark.md) for the benchmark method and commands.

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

MCP support is planned but not shipped in the current CLI. The current release focuses on the local graph engine, CLI workflows, and graph/diff visualization. See [ROADMAP.md](ROADMAP.md) for the MCP plan.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Release and packaging notes are in [docs/releasing.md](docs/releasing.md).

## Contributing

PRs are welcome, especially for language providers, graph resolution, and CLI usability. See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT
