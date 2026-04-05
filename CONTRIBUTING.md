# Contributing to GitGrapher

PRs welcome. This project was vibe-coded into existence and needs people who actually know what they're doing.

## What we need most

### New language providers
Each language is a ~200 line Rust file with tree-sitter queries. See `crates/gg-parse/src/python.rs` for a good example.

To add a language:
1. Add `tree-sitter-<lang>` to `crates/gg-parse/Cargo.toml`
2. Create `crates/gg-parse/src/<lang>.rs` implementing the `LanguageProvider` trait
3. Register it in `crates/gg-parse/src/lib.rs`
4. Add tests

**Wanted:** Java, Go, Rust, C#, C/C++, PHP, Ruby, Swift, Kotlin, Dart

### Better resolution
The cross-file resolution engine (`crates/gg-resolve/`) handles imports, calls, and heritage. It's functional but not as sophisticated as it could be. Areas for improvement:
- Re-export chain walking (barrel exports)
- Receiver type inference for method calls
- Generic type propagation
- Module alias resolution for Python

### Better visualization
The 3D graph viewer works but is basic. It uses `3d-force-graph` (Three.js). Could use:
- Community-colored clusters
- Collapsible file/folder grouping
- Minimap
- Edge type filtering in the UI

### MCP server
We have the graph engine but no MCP server yet. Needs a TypeScript wrapper in `packages/gitgrapher/` using `@modelcontextprotocol/sdk` that calls the Rust core via napi-rs.

## Development setup

```bash
# Clone
git clone https://github.com/duriantaco/gitgrapher
cd gitgrapher

# Build
cargo build

# Run tests
cargo test

# Run against a repo
cargo run --bin gitgrapher -- analyze /path/to/repo
cargo run --bin gitgrapher -- query "something" -p /path/to/repo
```

## Project structure

```
crates/
  gg-core/       # Shared types (NodeLabel, RelationType, Language, Config)
  gg-parse/      # Tree-sitter parsing + language providers
  gg-resolve/    # Import/call/type resolution engine
  gg-graph/      # Graph store with persistence + BFS
  gg-search/     # Tantivy BM25 search
  gg-pipeline/   # Pipeline orchestrator + community/process detection
  gg-napi/       # napi-rs bindings (WIP)
  gg-cli/        # CLI commands
```

## Adding a language provider

The `LanguageProvider` trait requires:

```rust
pub trait LanguageProvider: Send + Sync {
    fn language(&self) -> Language;
    fn extensions(&self) -> &[&str];
    fn parse(&self, path: &Path, source: &[u8], config: &Config) -> GgResult<ParseResult>;
}
```

`ParseResult` contains:
- `nodes: Vec<GraphNode>` — extracted symbols (functions, classes, etc.)
- `imports: Vec<ExtractedImport>` — import statements
- `calls: Vec<ExtractedCall>` — function/method calls
- `heritage: Vec<ExtractedHeritage>` — extends/implements

Write tree-sitter queries for your language's AST node types, extract them with `QueryCursor`, and return the result. See the existing providers for patterns.

## Tests

Every feature should have tests. Run with:

```bash
cargo test                    # All tests
cargo test -p gg-parse        # Just parsing tests
cargo test -p gg-resolve      # Just resolution tests
cargo test -p gg-pipeline     # Just pipeline tests
cargo test -p gg-search       # Just search tests
```

## Code style

- `cargo fmt` before committing
- `cargo clippy` should be clean (some warnings are acceptable for now)
- Use `Result<T, E>` for fallible operations, not panics
- No `unwrap()` in library code — use `?` or handle errors

## License

By contributing, you agree that your contributions will be licensed under MIT.
