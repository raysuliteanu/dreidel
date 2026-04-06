# Building and Releasing dreidel

## Prerequisites

```bash
cargo install cargo-release cargo-deny --locked
brew install git-cliff        # or: cargo install git-cliff --locked
```

## Development Build

```bash
cargo build                   # debug build
cargo build --release         # optimised build
cargo run -- --help           # run with CLI help
```

## Testing

```bash
cargo test                    # all tests
cargo test components::cpu    # filter by module/name
INSTA_UPDATE=always cargo test  # accept updated insta snapshots
```

## Code Quality

```bash
cargo fmt --check             # formatting (also runs as pre-push hook)
cargo clippy --all-targets -- -D warnings   # lints (also runs after every .rs file save)
cargo deny check              # license compliance + security advisories
cargo audit                   # standalone CVE scan (subset of cargo deny)
```

## Changelog

The changelog is generated from [conventional commits](https://www.conventionalcommits.org/)
using [git-cliff](https://github.com/orhun/git-cliff). Config: `cliff.toml`.

```bash
# Preview what the changelog would look like for a hypothetical next tag:
git-cliff --tag v0.2.0

# Regenerate CHANGELOG.md in place (cargo release does this automatically):
git-cliff --output CHANGELOG.md
```

## Release Process

Releases are managed with [cargo-release](https://github.com/crate-ci/cargo-release).
Config: `release.toml`. The repo uses **jj** in co-located mode; `cargo release`
operates on the underlying git layer.

### Dry run (default — no changes made)

```bash
cargo release patch    # or: minor | major
```

### Execute a release

```bash
cargo release patch --execute
```

This will:

1. Bump the version in `Cargo.toml`
2. Commit the version bump
3. Create a `vX.Y.Z` git tag
4. Publish to crates.io

Note: the `scripts/release` wrapper regenerates `CHANGELOG.md` via `git-cliff`
*before* running `cargo release`, so the changelog commit precedes the version
bump commit.

After `cargo release` completes, push the new commit and tag via jj:

```bash
jj git fetch
jj bookmark set main -r @-   # ensure main points at the release commit
jj git push --bookmark main
git push origin vX.Y.Z        # push the tag (jj doesn't manage git tags directly)
```

Or simply run the release script which does all of the above:

```bash
scripts/release [patch|minor|major]   # default: patch
```

### Version scheme

This project follows [Semantic Versioning](https://semver.org/):

| Change                             | Command               |
| ---------------------------------- | --------------------- |
| Bug fixes, minor improvements      | `cargo release patch` |
| New features, backwards-compatible | `cargo release minor` |
| Breaking changes                   | `cargo release major` |

While the version is `0.x`, minor-version bumps may include breaking changes
per semver convention.

## CI

GitHub Actions runs on every push and pull request to `main`:

| Job           | What it checks                                                                   |
| ------------- | -------------------------------------------------------------------------------- |
| `fmt`         | `cargo fmt --check`                                                              |
| `clippy`      | `cargo clippy --locked --all-targets -- -D warnings`                             |
| `test`        | `cargo test --locked` on stable                                                  |
| `msrv`        | `cargo test --locked` on Rust 1.88 (declared MSRV)                               |
| `coverage`    | `cargo llvm-cov --locked --fail-under-lines 80`                                  |
| `cross-check` | `cargo check --locked --target aarch64-unknown-linux-gnu`                        |
| `deny`        | `cargo deny check` via `EmbarkStudios/cargo-deny-action` — licenses + advisories |

Config: `.github/workflows/ci.yml`.
