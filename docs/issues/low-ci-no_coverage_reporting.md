# [LOW] No test coverage reporting in CI

## Location
`.github/workflows/ci.yml`

## Description
The CI pipeline runs tests but does not measure or enforce coverage. The project guidelines
specify an 80% coverage minimum, but there is no automated check verifying this target is
met. Coverage can silently regress as new code is added without tests.

## Impact
- Coverage targets from project guidelines are not enforced.
- New code paths can be added without tests and CI will remain green.
- No visibility into which modules are under-tested.

## Recommended Fix
Add a coverage job using `cargo-llvm-cov`:

```yaml
coverage:
  name: Coverage
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
      with:
        components: llvm-tools-preview
    - uses: Swatinem/rust-cache@v2
    - name: Install cargo-llvm-cov
      uses: taiki-e/install-action@cargo-llvm-cov
    - name: Generate coverage report
      run: cargo llvm-cov --locked --lcov --output-path lcov.info
    - name: Upload to Codecov (optional)
      uses: codecov/codecov-action@v4
      with:
        files: lcov.info
```

To enforce a minimum threshold:
```bash
cargo llvm-cov --locked --fail-under-lines 80
```

Note: some coverage tools undercount TUI rendering code (draw paths) because they depend
on display state. Consider excluding `draw()` method bodies from the line threshold or
accepting a lower threshold for the rendering layer.
