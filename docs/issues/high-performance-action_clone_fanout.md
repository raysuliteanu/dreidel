# [HIGH] Full `Action` payloads cloned 5× per tick at 60 Hz — DONE

## Location
`src/app.rs:381, 385`

## Description
Inside `handle_actions`, every incoming `Action` is cloned once per component and once for
the status bar — 5 clones total per action per tick:

```rust
for (_, comp) in &mut self.components {
    if let Some(new_action) = comp.update(action.clone())? {
        let _ = self.action_tx.try_send(new_action);
    }
}
self.status_bar.update(action.clone())?;
```

`Action` variants such as `CpuUpdate(CpuSnapshot)`, `NetUpdate(NetSnapshot)`,
`DiskUpdate(DiskSnapshot)`, and `ProcUpdate(ProcSnapshot)` carry owned heap data:
- `CpuSnapshot`: `Vec<f32>` per core, `Vec<u64>` frequencies
- `NetSnapshot`: `Vec<InterfaceSnapshot>` each with `Vec<String>` IP addresses
- `ProcSnapshot`: `Vec<ProcessEntry>` — potentially hundreds of entries with `String` fields

At 60 Hz, with 6 actions dispatched per collector tick, this is **1,800 clone operations per
second** involving large heap allocations. This is the single most significant performance
issue given that this code sits directly in the application's hot path.

## Impact
- Excessive heap allocations at 60 Hz.
- CPU overhead from copying large `Vec` payloads to components that don't use them.
- Memory pressure from transient allocations.

## Recommended Fix

### Option A: Change `Component::update` to take `&Action` (preferred)
```rust
fn update(&mut self, action: &Action) -> Result<Option<Action>> {
    Ok(None)
}
```
Components match on `&Action` and clone only what they need to store. This is the smallest
diff and the most idiomatic fix.

### Option B: Use `Arc<Action>` for large payload variants
Wrap large payloads in `Arc` so the fan-out is a pointer copy:
```rust
Action::CpuUpdate(Arc<CpuSnapshot>)
```
Snapshots are already immutable after construction, so `Arc` is semantically correct.

### Option C: Consume the action, clone only for remaining components
Consume the action into the last component and clone for all preceding ones. Saves one clone
but still O(n) for large payloads; not recommended unless combined with Option A or B.
