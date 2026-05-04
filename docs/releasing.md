# Releasing

GitGrapher releases are tag-driven. A `vX.Y.Z` tag builds platform archives and attaches them to the GitHub release.

## Release Checklist

1. Update versions in `Cargo.toml`, `Cargo.lock`, and `packages/gitgrapher/package.json` if the package metadata is being shipped.
2. Run local verification:

   ```bash
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo build --release --bin gitgrapher
   ```

3. Create and push an annotated tag:

   ```bash
   git tag -a v0.1.0 -m "GitGrapher v0.1.0"
   git push origin v0.1.0
   ```

4. Confirm the release workflow uploaded archives for macOS, Linux, and Windows.
5. Copy the generated release notes into the GitHub release and add any manual migration notes.

## Install Paths

Current install path:

```bash
cargo install --git https://github.com/duriantaco/gitgrapher gitgrapher
```

After the first release, the README can also point users to GitHub release archives:

```bash
gh release download v0.1.0 --pattern 'gitgrapher-*'
```

The npm package should stay unpublished until the wrapper invokes the Rust binary or native bindings end to end.
