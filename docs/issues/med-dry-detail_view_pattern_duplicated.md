# [MED] Detail view Esc/q + fullscreen-toggle pattern duplicated in `net.rs` and `disk.rs` — DONE

## Location
- `src/components/net.rs:140–155`
- `src/components/disk.rs:142–157`

## Description
The `Detail { .. }` arm of `handle_key_event` is identical in both components:

```rust
// net.rs (identical structure in disk.rs)
NetView::Detail { .. } => {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            self.view = NetView::List;
            let action = if self.is_fullscreen {
                Action::ToggleFullScreen
            } else {
                Action::Render
            };
            return Ok(Some(action));
        }
        // Swallow all other keys
        _ => return Ok(Some(Action::Render)),
    }
}
```

The pattern — exit detail view, optionally close fullscreen, swallow all other keys — is
the same in both. The only difference is the variant name.

## Impact
- Changes to detail-view exit behavior (e.g. adding a second confirmation step, supporting
  a back-navigation key like `[`) must be applied in two places.
- The "swallow all keys" comment and contract are duplicated; divergence risk.

## Recommended Fix
Once `NetView`/`DiskView` are unified into a shared `ListView` (see
`med-dry-netview_diskview_identical.md`), this arm can be implemented once in a shared
`handle_detail_key(key, is_fullscreen, view) -> Option<Action>` helper.
