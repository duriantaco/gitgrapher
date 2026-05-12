# Releasing

GitGrapher releases are tag-driven. A `vX.Y.Z` tag builds platform archives, generates checksums, and attaches them to the GitHub release.

## Release Artifacts

The release workflow publishes:

| Platform | Archive |
|----------|---------|
| Linux x86_64 | `gitgrapher-vX.Y.Z-linux-x86_64.tar.gz` |
| macOS x86_64 | `gitgrapher-vX.Y.Z-macos-x86_64.tar.gz` |
| macOS aarch64 | `gitgrapher-vX.Y.Z-macos-aarch64.tar.gz` |
| Windows x86_64 | `gitgrapher-vX.Y.Z-windows-x86_64.zip` |
| Checksums | `SHA256SUMS` |

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
5. Download and verify checksums:

   ```bash
   gh release download v0.1.0 --pattern 'gitgrapher-*' --pattern SHA256SUMS
   sha256sum -c SHA256SUMS
   ```

6. Smoke test at least one downloaded archive:

   ```bash
   tar -xzf gitgrapher-v0.1.0-macos-aarch64.tar.gz
   ./gitgrapher-v0.1.0-macos-aarch64/gitgrapher --version
   ```

7. Copy the generated release notes into the GitHub release and add any manual migration notes.

## Install Paths

From a release archive:

```bash
gh release download v0.1.0 --pattern 'gitgrapher-v0.1.0-macos-aarch64.tar.gz'
tar -xzf gitgrapher-v0.1.0-macos-aarch64.tar.gz
install -m 755 gitgrapher-v0.1.0-macos-aarch64/gitgrapher /usr/local/bin/gitgrapher
gitgrapher --version
```

From source:

```bash
cargo install --git https://github.com/duriantaco/gitgrapher gitgrapher
```

The npm package should stay unpublished until the wrapper invokes the Rust binary or native bindings end to end.
