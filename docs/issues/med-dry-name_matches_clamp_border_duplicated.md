# [MED] `name_matches`, `clamp_selection`, and `border_block` duplicated in `net.rs` and `disk.rs`

## Location
- `name_matches`: `net.rs:69–71`, `disk.rs:66–68`
- `clamp_selection`: `net.rs:73–92`, `disk.rs:70–89`
- `border_block`: `net.rs:297–307`, `disk.rs:293–303`

## Description

### `name_matches`
Both implementations are logically identical:
```rust
fn name_matches(&self, name: &str) -> bool {
    self.filter.is_empty() || name.to_lowercase().contains(&self.filter.to_lowercase())
}
```

### `clamp_selection`
Both implementations compute the filtered list length and clamp `list_state` to a valid index.
They differ only in the field name (`snap.interfaces` vs `snap.devices`) and the filter
predicate (applied to `i.name` vs `d.name`), but the shape is identical.

### `border_block`
These two methods are **byte-for-byte identical**:
```rust
fn border_block(&self, rest: &str) -> Block<'static> {
    let border_color = if self.focused { self.palette.accent } else { self.palette.border };
    Block::default()
        .title(keyed_title(self.focus_key, rest, &self.palette))
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_color))
}
```

## Impact
- Three sets of logic to maintain across two files.
- `clamp_selection` in particular has subtle edge cases (empty list, off-by-one) — a bug fix
  in one copy might not be applied to the other.

## Recommended Fix
Extract a `ListPanel` helper struct or free functions in `src/components/mod.rs` (or a new
`src/components/list_panel.rs`) that implements these behaviors generically. Alternatively,
`border_block` and `name_matches` can be free functions taking the relevant fields directly,
eliminating the need for method dispatch.
