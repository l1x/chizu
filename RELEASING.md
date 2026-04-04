# Releasing Chizu

This document defines the basic release flow for Chizu.

## Versioning

- Use semantic version tags in the form `vX.Y.Z`.
- Keep the crate versions in `chizu-core`, `chizu-index`, `chizu-query`, and
  `chizu-cli` aligned unless there is a deliberate reason to split them.
- Add or update the matching `## [X.Y.Z]` section in [CHANGELOG.md](CHANGELOG.md)
  before tagging.

## Local Verification

Run the basic release checks from the workspace root:

```bash
cargo check --workspace --locked
cargo test --workspace --locked
cargo package --package chizu-core --locked
```

Why only `chizu-core` here: before the first crates.io publish of the dependent
workspace crates, Cargo strips local `path` dependencies during packaging and
expects the referenced versions to already exist on crates.io. That means
`chizu-index`, `chizu-query`, and `chizu-cli` become fully package-verifiable in
publish order, not all at once from a fresh local workspace.

## GitHub Release

1. Update [CHANGELOG.md](CHANGELOG.md) and the crate versions.
2. Commit the release changes.
3. Create and push a version tag:

```bash
git tag v0.2.0
git push origin v0.2.0
```

4. The GitHub Actions `release` workflow will:
   - verify the tag version matches the crate manifests
   - run `cargo check --workspace --locked`
   - run `cargo test --workspace --locked`
   - run `cargo package --package chizu-core --locked`
   - create a GitHub release using the matching `CHANGELOG.md` section

## crates.io Publish Order

Publish the crates in dependency order:

```bash
cargo publish -p chizu-core
cargo publish -p chizu-index
cargo publish -p chizu-query
cargo publish -p chizu-cli
```

Wait for the crates.io index to update between publishes when required. The CLI
crate depends on the other three crates, so it should be published last.

## Installation Stories

Install from source:

```bash
cargo install --path chizu-cli
```

Install from crates.io after release:

```bash
cargo install chizu-cli --bin chizu
```

