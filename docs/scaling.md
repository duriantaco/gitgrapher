# Scaling Notes

GitGrapher is designed to handle tens of thousands of symbols locally.

For a 50,000-function TypeScript fixture, generate and benchmark with:

```bash
scripts/make_large_fixture.sh /tmp/gitgrapher-50k 500 100
cargo build --release --bin gitgrapher
target/release/gitgrapher benchmark --format json /tmp/gitgrapher-50k
```

## Large Repository Behavior

- Repository walking is deterministic and skips dependency/build/cache folders by default.
- `.gitignore` files are respected, including nested `.gitignore` files and simple negation rules.
- Symlinked files and directories are skipped to avoid recursive loops and duplicate indexing.
- The pipeline scans only languages with registered parser providers.
- Parsing is parallelized with Rayon. Tune with `GG_WORKER_THREADS`.
- Large files are skipped by default above 32 MB. Tune with `GG_MAX_FILE_SIZE`.
- Incremental indexing hashes file contents and reparses only added or modified files.

## Practical Limits

The core graph store and CLI queries are suitable for 50,000 symbols. Search, context, impact, and MCP tool calls return bounded result sets.

Visualization is intentionally capped by default: browser-based force graph rendering is not the right interface for every node in a very large repository. Use filtered exports or the query/context/impact tools for large graphs.

The current persistent store is a single `.gitgrapher/graph.json` file. That is acceptable for tens of thousands of symbols; for hundreds of thousands to millions of symbols, the next storage step should be a sharded or embedded database backend.
