# Roadmap

This roadmap is intentionally focused on adoption blockers and the core engine work that makes GitGrapher defensible.

## Near Term

- [ ] Publish tagged release binaries for macOS, Linux, and Windows.
- [ ] Add Go and Rust language providers.
- [ ] Improve receiver type inference for method calls.
- [ ] Add screenshot/GIF assets for graph and diff workflows.

## Medium Term

- [ ] Publish a working npm wrapper after the Rust binary distribution path is stable.
- [ ] Add Java and C# language providers.
- [ ] Add graph export filters for relation type, language, community, and symbol label.
- [ ] Add regression fixtures for import resolution and graph diffs.

## Done

- [x] Rust CLI for analyze, query, context, impact, symbols, status, list, clean, export, serve, and diff.
- [x] TypeScript, JavaScript, and Python parsing.
- [x] Persistent local graph storage in `.gitgrapher/`.
- [x] Incremental file change detection.
- [x] MCP over stdio with tools for `query`, `context`, `impact`, `list_repos`, and graph resources.
- [x] Benchmark command with machine-readable JSON output.
- [x] Published 50,000-function synthetic benchmark in `docs/benchmark.md`.
- [x] Interactive 3D graph export and local viewer.
