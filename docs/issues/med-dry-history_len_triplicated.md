# [MED] `HISTORY_LEN` constant triplicated across three components

## Location
- `src/components/cpu.rs:23`
- `src/components/net.rs:23`
- `src/components/disk.rs:23`

## Description
All three components independently declare:

```rust
pub const HISTORY_LEN: usize = 100;
```

The constant controls the depth of the ring-buffer history used for chart rendering.
If the value ever needs to change (e.g. to make history configurable, or to tune memory
usage), it must be updated in three places.

## Impact
- Three-way change required for any history depth adjustment.
- Risk of the values drifting apart silently (they are currently all 100 but the type is
  `pub`, so callers could reference any of the three and get a stale value after a partial
  update).

## Recommended Fix
Declare a single constant in `src/components/mod.rs`:

```rust
pub(crate) const HISTORY_LEN: usize = 100;
```

Remove the local declarations in `cpu.rs`, `net.rs`, and `disk.rs`, and reference the shared
constant. If the value should eventually be user-configurable, this single constant becomes
the natural place to thread config into.
