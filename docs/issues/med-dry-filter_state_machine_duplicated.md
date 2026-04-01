# [MED] Filter state machine duplicated across `net.rs`, `disk.rs`, and `cpu.rs` — DONE

## Location
- `src/components/net.rs:157–183`
- `src/components/disk.rs:159–185`
- `src/components/cpu.rs:259–286`

## Description
The `Filter { input }` arm of `handle_key_event` is nearly identical in all three components.
All three handle the same four keys with the same semantics:

| Key | Behavior |
|-----|----------|
| `Esc` | Clear `filter`, return to `List`/`Normal` state |
| `Enter` | Commit filter, return to `List`/`Normal` state |
| `Backspace` | Pop last char from `input`, update `filter` and view state |
| `Char(c)` | Push `c` to `input`, update `filter` and view state |

The only differences are:
- The enum variant name (`NetView::Filter`, `DiskView::Filter`, `CpuState::FilterMode`)
- The secondary side-effect (`clamp_selection()` vs `scroll_offset = 0`)

## Impact
- A bug in filter input handling (e.g. incorrect Backspace behavior on multi-byte characters)
  must be fixed in three places.
- Adding a new key (e.g. Ctrl+U to clear input) requires three changes.
- The Backspace path contains an unnecessary double-clone that exists in all three copies
  (see `med-performance-view_state_clone_for_match.md`).

## Recommended Fix
Extract a `FilterInput` struct that owns the `input: String` and exposes a
`handle_key(key: KeyEvent) -> FilterInputEvent` method returning one of
`{ Updated(String), Committed(String), Cancelled }`. Each component delegates to this helper
and applies the component-specific side effects after receiving the event.

```rust
pub(crate) struct FilterInput {
    pub input: String,
}

pub(crate) enum FilterEvent {
    Updated,
    Committed,
    Cancelled,
}

impl FilterInput {
    pub fn handle_key(&mut self, key: KeyEvent) -> FilterEvent { ... }
}
```
