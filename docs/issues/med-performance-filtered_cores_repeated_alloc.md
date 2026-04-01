# [MED] `filtered_cores()` allocates `Vec<usize>` 3–4× per render frame

## Location
`src/components/cpu.rs:85–92` (definition), called at:
- Line 97 — from `clamp_scroll()`, called from `draw_chart()`
- Line 168 — directly in `draw_chart()`
- Line 251 — from `preferred_height()`, called by `App` before every render
- Line 289 — in `handle_key_event` (after state match)

## Description
`filtered_cores()` constructs a new `Vec<usize>` on every invocation:

```rust
fn filtered_cores(&self) -> Vec<usize> {
    let n = self.num_cores();
    if self.filter.is_empty() {
        return (0..n).collect();
    }
    let f = self.filter.to_lowercase();
    (0..n).filter(|&i| format!("cpu{i}").contains(&f)).collect()
}
```

Within a single render frame, `preferred_height()` and then the full draw path both call
this, resulting in at least 3 `Vec<usize>` allocations per 60 Hz tick — one of which
(`preferred_height`) is discarded immediately after `.len()` is read.

On a system with many cores and an active filter, this also allocates a temporary `String`
per core per call via `format!("cpu{i}")`.

## Impact
- ~180 unnecessary `Vec<usize>` allocations per second at 60 Hz.
- Additional `String` allocations per core when a filter is active.

## Recommended Fix

### Option A: Cache the filtered indices
Add a `filtered_core_indices: Vec<usize>` field, invalidated when `filter` changes or new
CPU data arrives. `preferred_height` and `draw_chart` both read the cached value.

### Option B: Add a `filtered_core_count()` helper
For call sites that only need the count (i.e. `preferred_height` and `clamp_scroll`), add:
```rust
fn filtered_core_count(&self) -> usize {
    let n = self.num_cores();
    if self.filter.is_empty() { return n; }
    let f = self.filter.to_lowercase();
    (0..n).filter(|&i| format!("cpu{i}").contains(&f)).count()
}
```
This avoids the `Vec` allocation for count-only callers while keeping the existing
`filtered_cores()` for call sites that need the index list.
