# [MED] All `Tui` struct fields are `pub` — exposes implementation details — DONE

## Location
`src/tui.rs:42–52`

## Description
The `Tui` struct exposes all of its fields publicly:

```rust
pub struct Tui {
    pub terminal: ratatui::Terminal<Backend<Stdout>>,
    pub task: JoinHandle<()>,
    pub cancellation_token: CancellationToken,
    pub event_rx: mpsc::Receiver<Event>,
    pub event_tx: mpsc::Sender<Event>,
    pub frame_rate: f64,
    pub tick_rate: f64,
    pub mouse: bool,
    pub paste: bool,
}
```

The fields `task`, `cancellation_token`, `event_rx`, `event_tx`, `frame_rate`, `tick_rate`,
`mouse`, and `paste` are implementation details of the event loop. Exposing them:

- Allows external code to interfere with the cancellation token or join handle.
- Couples callers to the internal representation (e.g. if `event_rx` is changed from
  `mpsc::Receiver` to a different abstraction, all callers break).
- The only external use of `pub` fields in `app.rs` is `tui.terminal.clear()` (line 356),
  which could be exposed via a `clear()` method on `Tui`.

## Impact
- Any caller can cancel the token, drain the event channel, or change rate fields without
  going through `Tui`'s controlled interface.
- Makes future refactoring of `Tui` internals a breaking change.

## Recommended Fix
Make all fields `pub(crate)` or private, and expose only the operations callers need:

```rust
pub struct Tui {
    terminal: ratatui::Terminal<Backend<Stdout>>,
    task: JoinHandle<()>,
    cancellation_token: CancellationToken,
    event_rx: mpsc::Receiver<Event>,
    event_tx: mpsc::Sender<Event>,
    frame_rate: f64,
    tick_rate: f64,
    mouse: bool,
    paste: bool,
}

impl Tui {
    pub fn clear(&mut self) -> anyhow::Result<()> {
        self.terminal.clear().context("clearing terminal")
    }
}
```

The builder methods (`mouse`, `frame_rate`, `tick_rate`) are the correct public API for
configuration — they can remain `pub`.
