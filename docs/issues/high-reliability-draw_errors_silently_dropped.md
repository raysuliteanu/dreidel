# [HIGH] Draw errors silently dropped inside `terminal.draw` closure

## Location
`src/app.rs:448, 464, 478, 489, 494`

## Description
Ratatui's `terminal.draw(|frame| { ... })` closure cannot return `Result`. As a result, every
component `draw()` call uses `let _ = comp.draw(frame, rect)`, silently discarding any error.
If a component fails to render (e.g. an underlying I/O error, a layout panic, or a logic bug),
the user sees a blank or partial panel with no indication that anything went wrong.

Affected call sites:
- `let _ = self.status_bar.draw(frame, status_rect);` (line 448)
- `let _ = comp.draw(frame, rect);` (adaptive layout, line 464)
- `let _ = comp.draw(frame, *rect);` (preset layout, line 478)
- `let _ = comp.draw(frame, modal);` (fullscreen overlay, line 489)
- `let _ = self.help_comp.draw(frame, total_area);` (help overlay, line 494)

## Impact
- Rendering failures are invisible to the user and to developers reading logs.
- Bugs in `draw()` implementations may go undetected in production.

## Recommended Fix
Log errors at `tracing::warn!` (or `error!`) at each discard site. Since the draw closure
cannot propagate errors, the errors must be captured and logged inline:

```rust
if let Err(e) = comp.draw(frame, rect) {
    tracing::warn!(component = ?id, error = %e, "draw failed");
}
```

A more thorough fix would collect draw errors in a `Vec` inside the closure and propagate
them after `terminal.draw` returns, converting them into `Action::Error` events.
