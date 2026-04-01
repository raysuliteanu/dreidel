# [MED] `self.latest.clone()` in `draw()` clones full snapshot on every render frame

## Location
- `src/components/cpu.rs:361` — `let Some(snap) = self.latest.clone()`
- `src/components/net.rs:554` — `&& let Some(snap) = &self.latest.clone()`
- `src/components/disk.rs:418` — similar pattern

## Description
In the `draw()` method of `CpuComponent`, the latest snapshot is cloned in full before
rendering:

```rust
let Some(snap) = self.latest.clone() else {
    return Ok(());
};
```

`CpuSnapshot` contains `Vec<f32>` (per-core data), `Vec<u64>` (frequencies), and `String`
fields. In `NetSnapshot`, the clone includes a `Vec<InterfaceSnapshot>` where each
`InterfaceSnapshot` contains `Vec<String>` IP addresses. These clones occur at 60 Hz.

The clone is unnecessary because `draw()` only reads from the snapshot — it does not need
to own it. The borrow checker conflict that motivates the clone (the component needs
`&mut self` for `draw_chart` while also referencing `self.latest`) can be resolved by
passing the snapshot as a parameter to the rendering helpers, or by restructuring slightly.

## Impact
- Full heap copies of snapshot data at 60 Hz.
- For a system with many network interfaces (each with multiple IP addresses), this is a
  significant allocation per frame.

## Recommended Fix
Use `as_ref()` and pass a reference to inner helpers:

```rust
// cpu.rs draw()
let Some(snap) = self.latest.as_ref() else {
    return Ok(());
};
self.draw_header(frame, header_area, snap);  // snap: &CpuSnapshot
self.draw_chart(frame, chart_area, snap);
```

Rendering helpers that currently take `snap: &CpuSnapshot` by reference already work
this way — the fix is primarily at the `draw()` entry point where `.clone()` is used
instead of `.as_ref()`.
