# GitGrapher

Rust-powered code intelligence engine. Parses codebases into knowledge graphs for AI agents and developers.

## What it does

GitGrapher indexes your codebase into a knowledge graph that tracks:
- **Symbols**: functions, classes, methods, interfaces, enums, properties
- **Relationships**: calls, imports, extends, implements, contains
- **Communities**: automatically detected clusters of related code (Louvain algorithm)
- **Processes**: execution flows traced from entry points through call chains
- **Cross-file resolution**: import paths resolved, call targets linked across files

Then lets you query it from the CLI or visualize it in 3D.

## Quick start

```bash
# Build from source
cargo build --release

# Index a repository
gitgrapher analyze /path/to/repo

# Query it
gitgrapher query "UserService"
gitgrapher context handleLogin
gitgrapher impact UserService -d up
gitgrapher symbols -l class

# Check what's indexed
gitgrapher status /path/to/repo
gitgrapher list

# Visualize in 3D (opens browser)
gitgrapher serve -p /path/to/repo

# Export graph data
gitgrapher export -f json -p /path/to/repo
gitgrapher export -f dot -p /path/to/repo
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

## Supported languages

- **TypeScript** (.ts, .tsx, .mts, .cts)
- **JavaScript** (.js, .jsx, .mjs, .cjs)
- **Python** (.py, .pyi)

More coming (Java, Go, Rust, C#, etc.) — PRs welcome.

## Architecture

```
crates/
├── gg-core/       # Types, config, error handling
├── gg-parse/      # Tree-sitter parsing (TS, JS, Python)
├── gg-resolve/    # Cross-file import/call/type resolution
├── gg-graph/      # Graph storage with persistence
├── gg-search/     # Tantivy BM25 full-text search
├── gg-pipeline/   # Orchestrator (scan → parse → resolve → community → process)
├── gg-napi/       # Node.js bindings via napi-rs (WIP)
└── gg-cli/        # CLI interface
```

**Core design decisions:**
- **Rust core** for all CPU-bound work (parsing, resolution, graph ops, search)
- **Tree-sitter** for multi-language AST parsing (native Rust bindings, no WASM overhead)
- **Rayon** for parallel file parsing
- **Tantivy** for BM25 full-text search
- **Louvain algorithm** for community detection
- **Incremental indexing** via xxhash file change detection
- All limits configurable via env vars (`GG_MAX_FILE_SIZE`, `GG_WORKER_THREADS`, etc.)

## CLI commands

| Command | Description |
|---------|-------------|
| `analyze [path]` | Index a repository (incremental if already indexed) |
| `query <term>` | BM25 full-text search across symbols |
| `context <symbol>` | Show a symbol's callers, callees, and connections |
| `impact <symbol>` | Blast radius analysis (upstream/downstream) |
| `symbols [-l type]` | List symbols, optionally filtered by type |
| `status [path]` | Show index stats |
| `list` | List all indexed repositories |
| `serve [-P port]` | Start local server with 3D graph visualization |
| `export [-f format]` | Export graph as HTML (3D), JSON, or DOT |
| `setup` | Configure MCP for Claude Code / Cursor |
| `clean [path]` | Remove index for a repository |

## Building from source

```bash
# Prerequisites: Rust 1.86+
git clone https://github.com/duriantaco/gitgrapher
cd gitgrapher
cargo build --release

# Run tests
cargo test

# The binary is at target/release/gitgrapher
```

## How it works

1. **Scan**: Walk the filesystem, detect languages by extension, skip ignored patterns
2. **Parse**: Tree-sitter extracts ASTs in parallel (Rayon), producing symbols, calls, imports, heritage
3. **Resolve**: Cross-file linking — import paths → target files, calls → target symbols, extends → parent classes
4. **Community detection**: Louvain clustering groups related symbols by CALLS/EXTENDS/IMPLEMENTS edges
5. **Process detection**: Entry point scoring + BFS traces execution flows through the graph
6. **Persist**: Graph saved to `.gitgrapher/graph.json` inside the repo
7. **Search**: Tantivy indexes all symbols for BM25 full-text search

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Short version: PRs welcome, especially for new language providers.

## License

MIT

## Disclaimer

This project was ~70% vibe-coded in a single session with Claude.
