# Roadmap

This roadmap is intentionally focused on adoption blockers and the core engine work that makes GitGrapher defensible.

## Near Term

- [ ] Publish tagged release binaries for macOS, Linux, and Windows.
- [ ] Add a reproducible benchmark fixture and publish current numbers in `docs/benchmark.md`.
- [ ] Implement MCP over stdio with tools for `query`, `context`, `impact`, `list_repos`, and graph resources.
- [ ] Add Go and Rust language providers.
- [ ] Improve receiver type inference for method calls.
- [ ] Add screenshot/GIF assets for graph and diff workflows.

## Medium Term

- [ ] Publish a working npm wrapper after the Rust binary distribution path is stable.
- [ ] Add Java and C# language providers.
- [ ] Add graph export filters for relation type, language, community, and symbol label.
- [ ] Add a benchmark command that emits machine-readable JSON.
- [ ] Add regression fixtures for import resolution and graph diffs.

## Done

- [x] Rust CLI for analyze, query, context, impact, symbols, status, list, clean, export, serve, and diff.
- [x] TypeScript, JavaScript, and Python parsing.
- [x] Persistent local graph storage in `.gitgrapher/`.
- [x] Incremental file change detection.
- [x] Interactive 3D graph export and local viewer.
