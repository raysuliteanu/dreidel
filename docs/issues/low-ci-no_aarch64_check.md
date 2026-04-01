# [LOW] CI does not verify `aarch64-unknown-linux-gnu` target despite `deny.toml` listing it — DONE target despite `deny.toml` listing it

## Location
`.github/workflows/ci.yml`
`deny.toml` — `targets` list

## Description
`deny.toml` declares two platform targets for license and advisory checking:

```toml
targets = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
]
```

This implies the project intends to support `aarch64`. However, CI only runs on
`ubuntu-latest` (x86_64). There is no compile check for the `aarch64` target, so
platform-specific code (`#[cfg(target_os = "linux")]` blocks that call into `/proc`
or use Linux-specific sysinfo features) is never verified to compile on aarch64.

## Impact
- A bug that causes a compile failure on `aarch64` would not be caught by CI.
- The `deny.toml` target list may give a false impression of tested multi-arch support.

## Recommended Fix
Add a cross-compilation check job. No hardware is needed — `cargo check` with a
cross-compilation target catches compile errors without running tests:

```yaml
cross-check:
  name: Cross-compile check (aarch64)
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
      with:
        targets: aarch64-unknown-linux-gnu
    - uses: Swatinem/rust-cache@v2
    - run: cargo check --locked --target aarch64-unknown-linux-gnu
```

If the project does not currently intend `aarch64` support, remove it from `deny.toml`
to eliminate the false signal.
