# Clipboard Copy Feature Design

**Date:** 2026-03-29
**Status:** TODO — design complete, not yet implemented

---

## Context

Users want to copy data from the dreidel TUI to the system clipboard — primarily device/interface names and row data from the net, disk, and process tables. The feature must work both in desktop terminals and over SSH (where X11 forwarding may not be available). It is opt-in at compile time via a Cargo feature flag to avoid pulling in system clipboard libraries for users who don't need them.

---

## Keybindings

| Key | Action |
|-----|--------|
| `y` | Copy selected row as tab-separated values |
| `Y` | Copy primary identifier only (interface name / device name / process name or PID) |

**Future:** `ctrl-c` as an alias for `y`. Needs investigation — verify that crossterm delivers `KeyEvent { code: Char('c'), modifiers: CONTROL }` to the app event loop rather than the OS consuming it as SIGINT first.

**Not implemented (noted for future):** Mouse text selection. Shift+drag in the terminal bypasses app mouse capture and works today without any code changes. For click-to-copy-column support, see the Future Considerations section.

---

## Architecture

### Feature gate

```toml
[features]
default = []
clipboard = ["dep:arboard"]

[dependencies]
arboard = { version = "3", optional = true }
base64 = "0.22"   # non-optional; needed for OSC 52 path
```

When built without `--features clipboard`, the `y`/`Y` keys are still handled by components and return `Action::CopyToClipboard`, but `App` no-ops (logs a debug message) instead of writing to the clipboard.

### Action flow

```
KeyEvent('y') in focused component
  → build tab-separated row string
  → return Action::CopyToClipboard(row_string)

KeyEvent('Y') in focused component
  → extract primary identifier
  → return Action::CopyToClipboard(name_string)

App::handle_actions(Action::CopyToClipboard(s))
  → #[cfg(feature = "clipboard")] clipboard::copy(s)
  → fire Action::Notify { message, level, duration_ms }
```

### New action variants (`src/action.rs`)

```rust
CopyToClipboard(String),
Notify { message: String, level: NotifyLevel, duration_ms: u64 },
```

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifyLevel { Success, Warning, Error }
```

---

## Clipboard Module (`src/clipboard.rs`)

Entirely wrapped in `#[cfg(feature = "clipboard")]`.

```rust
pub fn copy(text: &str) -> anyhow::Result<()> {
    if try_arboard(text).is_ok() {
        return Ok(());
    }
    try_osc52(text)
}

fn try_arboard(text: &str) -> anyhow::Result<()> {
    arboard::Clipboard::new()
        .context("creating clipboard")?
        .set_text(text)
        .context("setting clipboard text")
}

fn try_osc52(text: &str) -> anyhow::Result<()> {
    use std::io::Write;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let seq = format!("\x1b]52;c;{encoded}\x07");
    // Write directly to /dev/tty to avoid corrupting ratatui's output buffer
    let mut tty = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/tty")
        .context("opening /dev/tty for OSC 52")?;
    tty.write_all(seq.as_bytes()).context("writing OSC 52 sequence")
}
```

**arboard path:** Uses X11/Wayland (Linux), AppKit (macOS), or Win32. Fails fast if no display server is available.

**OSC 52 path:** Writes an escape sequence directly to `/dev/tty`. Supported by kitty, alacritty, iTerm2, WezTerm, tmux (with `set-clipboard on`), and most modern terminals. Works over SSH without X forwarding.

---

## Status Bar Notification System (`src/components/status_bar.rs`)

A general-purpose transient notification channel — not clipboard-specific. Any `Action::Notify` fired by any component or subsystem will display here.

Add to `StatusBar` struct:
```rust
notification: Option<(String, NotifyLevel, Instant)>,
```

Handle in `update()`:
```rust
Action::Notify { message, level, duration_ms } => {
    self.notification = Some((
        message,
        level,
        Instant::now() + Duration::from_millis(duration_ms),
    ));
}
```

In `draw()`: if `Instant::now() < expiry`, render notification text in place of normal status content, color-coded:

| Level | Color |
|-------|-------|
| Success | green |
| Warning | yellow |
| Error | red |

After TTL expires, normal status bar content resumes with no user action needed.

---

## Config Additions (`src/config.rs`)

All fields have serde defaults so existing configs continue to work without changes.

```toml
[notifications]
show_copy_success = true   # show "Copied!" on success; default true
show_copy_failure = true   # show "Copy failed" on failure; default true
ttl_ms = 3000              # notification display duration in ms; default 3000

[process]
copy_primary = "name"      # what Y copies: "name" (default) or "pid"
```

`[notifications]` is general-purpose for future use, not clipboard-specific.

---

## Per-Component Copy Behavior

### Net component

| Key | Copies |
|-----|--------|
| `y` | `eth0\t4.8 MB/s\t1.2 MB/s\t192.168.1.100/24` (name, tx, rx, IP) |
| `Y` | `eth0` |

In detail view: `y` copies all detail fields as `key\tvalue\n...` pairs; `Y` still copies interface name.

### Disk component

| Key | Copies |
|-----|--------|
| `y` | `sda\t102.4 KB/s\t0 B/s\t45.0%` (name, read rate, write rate, usage%) |
| `Y` | `sda` |

### Process component

| Key | Copies |
|-----|--------|
| `y` | Tab-separated visible columns (compact or wide depending on current view) |
| `Y` | Process name or PID per `config.process.copy_primary` |

In detail view: `y` copies all detail fields as `key\tvalue\n...` pairs; `Y` copies name/pid.

### CPU and Mem components

Not table-based, no row selection — `y`/`Y` keys are ignored when these are focused.

---

## Files to Create / Modify

| File | Change |
|------|--------|
| `Cargo.toml` | Add `arboard` optional dep, `base64` dep, `clipboard` feature |
| `src/action.rs` | Add `CopyToClipboard(String)`, `Notify { ... }`, `NotifyLevel` enum |
| `src/clipboard.rs` | **New file** — `#[cfg(feature = "clipboard")]` arboard + OSC 52 |
| `src/config.rs` | Add `NotificationConfig`, `ProcessConfig` with `copy_primary` |
| `src/app.rs` | Handle `Action::CopyToClipboard`, fire `Action::Notify` |
| `src/components/status_bar.rs` | Handle `Action::Notify`, render transient notification |
| `src/components/net.rs` | Handle `y`/`Y`, return `Action::CopyToClipboard` |
| `src/components/disk.rs` | Handle `y`/`Y`, return `Action::CopyToClipboard` |
| `src/components/process/mod.rs` | Handle `y`/`Y`, return `Action::CopyToClipboard` |

---

## Testing

- Unit test `clipboard::try_osc52` using a seam (write to a temp file instead of `/dev/tty`)
- Unit test status bar notification rendering with `TestBackend` + insta snapshot for each `NotifyLevel` color
- Unit test status bar TTL expiry (notification cleared after duration)
- Unit test each component's `handle_key_event` for `y`/`Y` — verify correct `Action::CopyToClipboard` payload
- `cargo build --features clipboard` — clean build
- `cargo build` (no feature) — clean build; `y`/`Y` keys work, no clipboard write
- Manual: focus net, press `y` → green "Copied: eth0\t..." in status bar for ~3s
- Manual: SSH session without X forwarding → OSC 52 path used
- Manual: `show_copy_success = false` → no notification on success
- Manual: `copy_primary = "pid"` → `Y` in process copies PID

---

## Future Considerations

### ctrl-c alias

`ctrl-c` (`KeyCode::Char('c')` with `KeyModifiers::CONTROL`) as an alias for `y`. Needs investigation: does crossterm deliver `Event::Key` for ctrl-c, or does the OS consume it as SIGINT? If it works, add as an alias in each component's `handle_key_event`.

### Mouse click-to-copy-column

For click-to-copy specific column values (e.g. just the IP address):

1. Add `handle_mouse_event(&mut self, mouse: MouseEvent)` to the `Component` trait
2. Route `Event::Mouse` in `App::handle_events` to the focused component (infrastructure already exists: mouse capture is enabled but events are currently discarded)
3. In each component, store column boundary x-positions from the last `draw()` call
4. On `MouseEventKind::Down(MouseButton::Left)`, determine which column was clicked and return `Action::CopyToClipboard(column_value)`

### Right-click context menu

Builds on mouse dispatch above. Needs a small overlay widget and `MouseEventKind::Down(MouseButton::Right)` handling.
