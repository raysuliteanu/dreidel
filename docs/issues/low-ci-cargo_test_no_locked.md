# [LOW] CI `cargo test` runs without `--locked`

## Location
`.github/workflows/ci.yml` — `test` job (line 42), `msrv` job (line 53)

## Description
Both test jobs run `cargo test` without the `--locked` flag:

```yaml
- run: cargo test
```

Without `--locked`, Cargo is free to use any compatible version of each dependency — it
will not fail if `Cargo.lock` is out of date or if a newer patch version was automatically
resolved. This means CI may test against a different dependency graph than the lock file
documents, which can mask version-specific bugs or allow silently upgraded transitive
dependencies.

This is particularly relevant because `cargo audit` runs in a separate job and audits
the lock file — if `cargo test` uses different versions, the audit results may not match
the code under test.

## Impact
- Tests may pass against a dependency set that differs from what is shipped.
- Potential divergence between the audited lock file and the tested binary.

## Recommended Fix
Add `--locked` to all test invocations:

```yaml
- run: cargo test --locked
```

Also apply to the `clippy` and `fmt` jobs for consistency, though those are less
security-sensitive.
