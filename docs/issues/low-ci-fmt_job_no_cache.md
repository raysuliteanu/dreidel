# [LOW] `fmt` CI job has no dependency cache

## Location
`.github/workflows/ci.yml` — `fmt` job (lines 14–22)

## Description
The `fmt` job does not use `Swatinem/rust-cache@v2`, unlike the `clippy` and `test` jobs:

```yaml
fmt:
  name: Format
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
      with:
        components: rustfmt
    - run: cargo fmt --check   # no cache step
```

`cargo fmt --check` itself is fast, but installing the Rust toolchain from scratch on every
run adds latency. The cache stores the compiled registry index and proc-macro dependencies,
reducing setup time across all jobs.

## Impact
- Slightly slower `fmt` job compared to its peers.
- Minor — `cargo fmt` is fast enough that this rarely matters in practice.

## Recommended Fix
Add `Swatinem/rust-cache@v2` for consistency:

```yaml
    - uses: Swatinem/rust-cache@v2
    - run: cargo fmt --check
```
