# Implementation Plan: `cargo binstall` Support

**Date**: 2026-04-03
**Issue**: [raysuliteanu/dreidel#6](https://github.com/raysuliteanu/dreidel/issues/6)

## Overview

Add a GitHub Actions release workflow that builds pre-built Linux binaries, attaches
them to GitHub Releases, configures `[package.metadata.binstall]` in `Cargo.toml`,
and updates the README placeholder.

## Platform Scope: Linux Only

dreidel is Linux-only — `procfs` reads `/proc`, `nix` is used for process kill — so
targets are:

| Target | Notes |
|--------|-------|
| `x86_64-unknown-linux-gnu` | Native, most common |
| `aarch64-unknown-linux-gnu` | Raspberry Pi, ARM servers |
| `x86_64-unknown-linux-musl` | Static binary, any distro |
| `aarch64-unknown-linux-musl` | Static binary, ARM |

## Tooling Decision: Hand-crafted Workflow (not `cargo-dist`)

`cargo-dist` conflicts with the existing `cargo-release` + `jj` + `git-cliff` workflow.
A ~80-line matrix workflow is simpler, fully transparent, and slots directly into what
already exists.

## How the Release Script Integrates

**No changes to `scripts/release` are needed structurally.** Line 96 already does:

```bash
git push origin "$TAG"
```

This push of a `v*` tag is exactly the trigger for the new release workflow
(`on: push: tags: ["v*"]`). The existing release flow becomes:

```
scripts/release [patch|minor|major]
  → git-cliff regenerates CHANGELOG.md
  → jj commit (changelog)
  → cargo release --execute (bumps Cargo.toml, commits, creates tag)
  → jj git push --bookmark main
  → git push origin vX.Y.Z  ← triggers the new release.yml workflow
```

A note will be added to the `scripts/release` header comment documenting this trigger.

---

## Implementation Phases

### Phase 1 — `.github/workflows/release.yml` (new file)

- Trigger: `on: push: tags: ["v*"]`
- Matrix of 4 targets on `ubuntu-latest`; native `cargo` for `x86_64-unknown-linux-gnu`,
  `cross` (via Docker) for the other three
- Each matrix job:
  1. `actions/checkout`
  2. `dtolnay/rust-toolchain@stable` with target added
  3. Install `cross` via `taiki-e/install-action@cross` (cross-compiled targets only)
  4. `cargo build --release --locked` (native) or `cross build --release --locked` (cross)
  5. Strip binary
  6. Create tarball: `dreidel-v{version}-{target}.tar.gz` containing `dreidel` binary + `LICENSE`
  7. Upload as workflow artifact
- Final `release` job (depends on all build jobs):
  1. Download all artifacts
  2. Generate `sha256sums.txt`
  3. Create GitHub Release via `softprops/action-gh-release@v2` with all tarballs + checksums

**Key detail**: Binary is placed at the archive root (not in a subdirectory), so
`[package.metadata.binstall]` must set `bin-dir = "{ bin }{ binary-ext }"`.

### Phase 2 — `Cargo.toml` binstall metadata

Add after the existing `[package.metadata.docs.rs]` section:

```toml
[package.metadata.binstall]
pkg-url = "{ repo }/releases/download/v{ version }/{ name }-v{ version }-{ target }.tar.gz"
bin-dir = "{ bin }{ binary-ext }"
pkg-fmt = "tgz"
```

### Phase 3 — `README.md` update

Replace lines 60–63 (the current placeholder text) with real installation instructions
listing the 4 supported targets.

### Phase 4 — `scripts/release` comment update

Add a note to the script header explaining that `git push origin $TAG` (line 96)
triggers `.github/workflows/release.yml` to build and publish pre-built binaries.

### Phase 5 — Verification (post-merge)

After tagging and pushing:
- [ ] All 4 tarballs appear as Release assets
- [ ] `sha256sums.txt` is attached
- [ ] `cargo binstall dreidel` installs correctly
- [ ] `dreidel --version` reports the correct version

---

## Risks and Mitigations

| Risk | Severity | Mitigation |
|------|----------|------------|
| `cross` aarch64 linker issues | Medium | `cross` Docker containers handle sysroot/linker automatically |
| musl build fails (C deps) | Medium | `procfs`/`nix` are minimal C; test locally with `cross build --target x86_64-unknown-linux-musl` before merging |
| `vergen-gix` fails in cross Docker (no `.git`) | Medium | `cross` mounts the full workspace including `.git`; verify on first run |
| binstall URL mismatch | Low | Explicit `[package.metadata.binstall]` config prevents guessing; verify with `cargo binstall --dry-run dreidel` |

---

## Files to Change

| File | Change |
|------|--------|
| `.github/workflows/release.yml` | **New** — matrix build + release workflow |
| `Cargo.toml` | Add `[package.metadata.binstall]` section after `[package.metadata.docs.rs]` |
| `README.md` | Replace placeholder in `cargo binstall` section (lines 60–63) |
| `scripts/release` | Add header comment documenting workflow trigger |
