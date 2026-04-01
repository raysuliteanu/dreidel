# [MED] Double `to_lowercase()` allocation on every filter comparison in `name_matches`

## Location
- `src/components/net.rs:70`
- `src/components/disk.rs:66`

## Description
Both components implement `name_matches` like this:

```rust
fn name_matches(&self, name: &str) -> bool {
    self.filter.is_empty() || name.to_lowercase().contains(&self.filter.to_lowercase())
}
```

On every call, this allocates **two** `String` values: one for `name.to_lowercase()` and one
for `self.filter.to_lowercase()`. This method is called once per interface/device per render
frame (in both the draw path and the key handler path). On a system with many interfaces
or disks, this creates many short-lived allocations per 60 Hz tick.

The filter string is constant between renders — its lowercase form only needs to be computed
once when the filter changes, not on every comparison.

The same issue exists in the draw loop's inline filter:
- `net.rs:404`: `let filter = self.filter.to_lowercase()` (called once per draw — this is
  correct, but the `name.to_lowercase()` per item is still wasteful)
- `disk.rs:347`: same pattern

## Impact
- Two String allocations per network interface/disk device per render at 60 Hz.
- Scales with the number of monitored interfaces/devices.

## Recommended Fix
Store the filter already lowercased. Update `filter` at every write site to store
`new_value.to_lowercase()`:

```rust
// When updating the filter:
self.filter = input.to_lowercase();
```

Then `name_matches` becomes a single allocation:
```rust
fn name_matches(&self, name: &str) -> bool {
    self.filter.is_empty() || name.to_lowercase().contains(&self.filter)
}
```

The draw loop can use `&self.filter` directly without `to_lowercase()`.
