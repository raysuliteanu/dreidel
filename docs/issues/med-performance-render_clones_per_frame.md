# [MED] `visible`, `slot_overrides`, and `palette` cloned/reconstructed on every render — DONE

## Location
`src/app.rs:409–416`

## Description
`render_to()` is called at 60 Hz. Each call clones several values to move them into the
`terminal.draw` closure (required because the closure also borrows `self` mutably for the
component iterators):

```rust
let focus = self.focus.clone();
let visible = self.visible.clone();
let show_help = self.show_help;
let loading = self.loading;
let palette = self.config.general.theme.palette();  // reconstructs ColorPalette
let status_pos = self.status_pos;
let slot_overrides = self.slot_overrides.clone();
```

- `self.visible.clone()` — `Vec<ComponentId>` where `ComponentId: Copy`. The clone is
  only needed because the closure has a different lifetime scope from `&self`.
- `self.slot_overrides.clone()` — `SlotOverrides` carries heap data.
- `self.config.general.theme.palette()` — reconstructs a `ColorPalette` struct on every
  call. `ColorPalette` is a plain struct of 8 `ratatui::style::Color` values; the cost is
  low but non-zero and entirely avoidable.

## Impact
- `Vec` allocation for `visible` at 60 Hz.
- `ColorPalette` reconstruction at 60 Hz.
- Cognitive overhead from "why is this cloned here?".

## Recommended Fix
Store the resolved `ColorPalette` in `App` rather than reconstructing it from `Theme` on
every render. The theme is fixed at startup (or changes require a restart), so this is
safe:

```rust
pub struct App {
    ...
    palette: ColorPalette,  // resolved once in App::new()
}
```

For `visible`, pass a slice reference into a helper that doesn't capture `self`:

```rust
fn render_inner(
    frame: &mut Frame,
    visible: &[ComponentId],
    ...
) { ... }
```

This avoids the clone by letting the closure capture a local reference rather than an
owned `Vec`.
