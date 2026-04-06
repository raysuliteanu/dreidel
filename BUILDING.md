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

## Documentation Screenshots

The `USER_GUIDE.md` includes text-based screenshots rendered from real component
output via `ratatui::backend::TestBackend`. To regenerate them after a rendering
change:

```bash
cargo test --test doc_screenshots
```

This writes updated `.txt` files to `docs/screenshots/`. The screenshots use
stub data (`.stub()` constructors) so they are deterministic and don't require
a live system. Replaced sections in `USER_GUIDE.md` are marked with
`<!-- Auto-generated: cargo test --test doc_screenshots -->` comments.

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

# Regenerate CHANGELOG.md in place:
git-cliff --output CHANGELOG.md
```

## Release Process

Use `scripts/release` to cut a release. It handles changelog generation,
version bumping, tagging, and pushing in one step:

```bash
scripts/release                   # patch release (default)
scripts/release minor             # minor release
scripts/release major             # major release
scripts/release -n                # dry run — preview without making changes
scripts/release -n minor          # dry-run a minor release
```

The script performs these steps:

1. Regenerate `CHANGELOG.md` via `git-cliff` and commit it
2. Run `cargo release` which bumps the version in `Cargo.toml`, commits,
   creates a `vX.Y.Z` git tag, and publishes to crates.io
3. Sync jj, advance the `main` bookmark, and push the bookmark and tag
   to origin

In dry-run mode (`-n` / `--dry-run`), changelog and push steps are echoed
but not executed, and `cargo release` runs in its own dry-run mode.

### Prerequisites

The script requires `cargo-release`, `git-cliff`, and `jj`:

```bash
cargo install cargo-release --locked
brew install git-cliff        # or: cargo install git-cliff --locked
```

### Underlying tools

- [cargo-release](https://github.com/crate-ci/cargo-release) — config: `release.toml`
- [git-cliff](https://github.com/orhun/git-cliff) — config: `cliff.toml`
- The repo uses **jj** in co-located mode; `cargo release` operates on the
  underlying git layer

### Version scheme

This project follows [Semantic Versioning](https://semver.org/):

| Change                             | Command                  |
| ---------------------------------- | ------------------------ |
| Bug fixes, minor improvements      | `scripts/release`        |
| New features, backwards-compatible | `scripts/release minor`  |
| Breaking changes                   | `scripts/release major`  |

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
