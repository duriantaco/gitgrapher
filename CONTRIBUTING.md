# Contributing to GitGrapher

GitGrapher is a Rust-first code intelligence engine. Contributions are welcome, especially when they improve language coverage, graph resolution quality, performance, and CLI ergonomics.

## What We Need Most

### New Language Providers

Each language provider is a focused Rust module backed by Tree-sitter queries. See `crates/gg-parse/src/python.rs` for a compact example.

To add a language:

1. Add `tree-sitter-<lang>` to `crates/gg-parse/Cargo.toml`.
2. Create `crates/gg-parse/src/<lang>.rs` implementing the `LanguageProvider` trait.
3. Register it in `crates/gg-parse/src/lib.rs`.
4. Add parser and pipeline tests.

Wanted next: Java, Go, Rust, C#, C/C++, PHP, Ruby, Swift, Kotlin, and Dart.

### Better Resolution

The cross-file resolution engine in `crates/gg-resolve/` handles imports, calls, and heritage. High-value improvements include:

- Re-export chain walking for barrel exports
- Receiver type inference for method calls
- Generic type propagation
- Module alias resolution for Python

### Better Visualization

The 3D graph viewer uses `3d-force-graph` and Three.js. Useful improvements include:

- Community-colored clusters
- Collapsible file and folder grouping
- Minimap
- Edge type filtering in the UI

### AI Agent Integrations

MCP support is planned but not shipped yet. The roadmap is to expose `query`, `context`, `impact`, repository listing, and graph resources through a standard MCP server without weakening the Rust CLI path.

## Development Setup

```bash
git clone https://github.com/duriantaco/gitgrapher
cd gitgrapher

cargo build
cargo test

cargo run --bin gitgrapher -- analyze /path/to/repo
cargo run --bin gitgrapher -- query "something" -p /path/to/repo
```

## Project Structure

```
crates/
  gg-core/       # Shared types, config, errors
  gg-parse/      # Tree-sitter parsing and language providers
  gg-resolve/    # Import, call, and type resolution
  gg-graph/      # Graph store with persistence and BFS
  gg-search/     # Tantivy BM25 search
  gg-pipeline/   # Pipeline orchestration and graph enrichment
  gg-napi/       # Experimental Node.js native bindings
  gg-cli/        # CLI commands
```

## Adding a Language Provider

The `LanguageProvider` trait requires:

```rust
pub trait LanguageProvider: Send + Sync {
    fn language(&self) -> Language;
    fn extensions(&self) -> &[&str];
    fn parse(&self, path: &Path, source: &[u8], config: &Config) -> GgResult<ParseResult>;
}
```

`ParseResult` contains:

- `nodes: Vec<GraphNode>`: extracted symbols
- `imports: Vec<ExtractedImport>`: import statements
- `calls: Vec<ExtractedCall>`: function and method calls
- `heritage: Vec<ExtractedHeritage>`: extends and implements relationships

Use Tree-sitter queries for the language's AST node types, extract them with `QueryCursor`, and return the structured result. Match existing provider patterns before introducing new abstractions.

## Tests

Run the full suite before opening a PR:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Targeted test commands:

```bash
cargo test -p gg-parse
cargo test -p gg-resolve
cargo test -p gg-pipeline
cargo test -p gg-search
```

## Code Style

- Use the existing crate boundaries and local helper APIs.
- Keep parser fixtures small and explicit.
- Use `Result<T, E>` for fallible operations.
- Avoid `unwrap()` in library code unless a test fixture is intentionally asserting setup.
- Keep public behavior documented in README, ROADMAP, or crate docs.

## License

By contributing, you agree that your contributions will be licensed under MIT.
