# [MED] `fmt_rate()` duplicated verbatim in `net.rs` and `disk.rs`

## Location
- `src/components/net.rs:118–128`
- `src/components/disk.rs:120–130`

## Description
Both files define an identical private function:

```rust
fn fmt_rate(bytes_per_sec: u64) -> String {
    const MB: u64 = 1_000_000;
    const KB: u64 = 1_000;
    if bytes_per_sec >= MB {
        format!("{:.1} MB/s", bytes_per_sec as f64 / MB as f64)
    } else if bytes_per_sec >= KB {
        format!("{:.1} KB/s", bytes_per_sec as f64 / KB as f64)
    } else {
        format!("{} B/s", bytes_per_sec)
    }
}
```

The sister function `fmt_rate_col` (which omits the "/s" suffix) already lives in
`src/components/mod.rs`. The duplicated `fmt_rate` should join it there.

## Impact
- Any change to formatting logic (e.g. adding GB/s tier) must be made in two places.
- The functions have diverged subtly in the past and will diverge again.

## Recommended Fix
Move the function to `src/components/mod.rs` as `pub(crate) fn fmt_rate(...)` and remove
both local definitions. Update call sites in `net.rs` and `disk.rs` to use the shared version.
