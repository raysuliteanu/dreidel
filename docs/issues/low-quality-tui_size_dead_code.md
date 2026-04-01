# [LOW] `Tui::size()` is unused with `#[allow(dead_code)]` and a TODO comment — DONE

## Location
`src/tui.rs:187–190`

## Description
```rust
// TODO: do we even need this method since it's not used
#[allow(dead_code)]
pub fn size(&self) -> anyhow::Result<ratatui::layout::Size> {
    Ok(self.terminal.size()?)
}
```

The TODO comment acknowledges the method is unused. It is suppressed from dead-code lints
with `#[allow(dead_code)]`. This is unresolved dead code with a known TODO.

## Impact
- Clutters the public API of `Tui`.
- The `#[allow(dead_code)]` hides the signal that would otherwise prompt cleanup.

## Recommended Fix
Remove the method. If terminal size is needed in future, it can be re-added at that point.
The underlying `self.terminal.size()` is always accessible directly.
