# [MED] `NetView` and `DiskView` are structurally identical enums — DONE

## Location
- `src/components/net.rs:27–34`
- `src/components/disk.rs:27–34`

## Description
Both enums have the same three variants with identical associated data:

```rust
// net.rs
enum NetView {
    List,
    Filter { input: String },
    Detail { name: String },
}

// disk.rs
enum DiskView {
    List,
    Filter { input: String },
    Detail { name: String },
}
```

The semantics are identical — the only difference is the type name. All logic that operates
on these enums (filter state transitions, detail navigation, title formatting) is consequently
duplicated across both components.

## Impact
- Any new view variant (e.g. a `TreeView`) must be added to both enums.
- The state machine logic that drives transitions is duplicated (see also
  `med-dry-filter_state_machine_duplicated.md` and `med-dry-detail_view_pattern_duplicated.md`).

## Recommended Fix
Introduce a shared `ListView` enum in `src/components/mod.rs` (or a new
`src/components/list_panel.rs`) used by both components:

```rust
#[derive(Debug, Clone)]
pub(crate) enum ListView {
    List,
    Filter { input: String },
    Detail { name: String },
}
```

Both `NetComponent` and `DiskComponent` replace their local enum with `ListView`. This is
a prerequisite for extracting the shared filter state machine and navigation logic.
