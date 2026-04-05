# Architecture

dreidel is a terminal system monitor written in Rust. It uses an async event loop with
a message-passing action bus to decouple data collection from rendering.

## Crate Structure

The crate is a dual lib+bin package:

- `src/lib.rs` — re-exports all public modules; used by integration tests and the binary
- `src/main.rs` — thin entry point: parses CLI args, loads config, runs `App`

## Key Third-Party Crates

| Crate                            | Purpose                                                                             |
| -------------------------------- | ----------------------------------------------------------------------------------- |
| `ratatui`                        | Terminal UI rendering (widgets, layout, styling)                                    |
| `crossterm`                      | Cross-platform terminal backend (raw mode, alternate screen, mouse, key events)     |
| `tokio`                          | Async runtime; powers the event loop and stats collector                            |
| `tokio-util`                     | `CancellationToken` for clean shutdown of background tasks                          |
| `sysinfo`                        | Cross-platform system metrics: CPU, memory, processes, networks, disks              |
| `procfs`                         | Linux-only `/proc` access for per-process priority, nice, threads, SHR, CPU split, faults, context switches, tty, fd count, and richer I/O |
| `clap`                           | CLI argument parsing (derive macro)                                                 |
| `serde` / `toml`                 | Config file deserialization                                                         |
| `humantime`                      | Parses human-readable durations (`"1s"`, `"500ms"`) in config                       |
| `anyhow`                         | Application-level error context                                                     |
| `thiserror`                      | Typed error definitions (errors module)                                             |
| `strum`                          | Derive `Display`, `EnumString`, `EnumIter` on enums (`ComponentId`, `LayoutPreset`) |
| `chrono`                         | Timestamps in `SysSnapshot`                                                         |
| `dirs`                           | XDG config directory path resolution                                                |
| `tracing` / `tracing-subscriber` | Structured logging to `~/.local/share/dreidel/dreidel.log`                          |
| `futures`                        | `StreamExt`/`FutureExt` used in the `Tui` event loop                                |
| `insta`                          | Snapshot testing for component `draw()` output                                      |

## Data Flow

```
sysinfo / procfs
      │
      ▼
stats/mod.rs  run_collector()          (background Tokio task)
      │  builds *Snapshot structs
      │  sends Action::*Update variants
      ▼
tokio::mpsc channel  (bounded, capacity from config)
      │
      ▼
App::handle_actions()                  (main async loop)
      │  dispatches to every Component via comp.update(action)
      │  handles UI state (focus, fullscreen, debug, quit)
      ▼
App::render()
      │  calls LayoutPreset::compute() to map SlotId → Rect
      │  calls comp.draw(frame, rect) for each visible component
      ▼
ratatui Terminal::draw()               (double-buffered, diff-based)
      │
      ▼
crossterm / stdout
```

The stats collector runs in a dedicated Tokio task and is the sole writer to the
channel. `App` is the sole reader. If the channel is full the collector drops the
update (backpressure) rather than blocking the render loop.

## Module Reference

### `src/app.rs` — `App`

The top-level application struct. Owns:

- The `mpsc` action channel (tx + rx)
- All `Component` boxed trait objects
- Focus state (`FocusState::Normal` / `FocusState::FullScreen`)
- Layout preset and visibility set

`App::run()` is the main loop: `handle_events → handle_actions` per iteration.
`App::render()` calls `render_to()` internally, which accepts any `ratatui::Backend`
— this seam is what allows `TestBackend`-based render tests in `app.rs`.

Key dispatch: the focused component gets first right of refusal on every key event
via `handle_key_event`. If it returns `Ok(None)`, the global handler runs (focus
switching, fullscreen, help, quit). This lets sub-modes inside a component (e.g.
`FilterMode` in the process panel) intercept keys like `Esc` before the global
handler sees them.

### `src/action.rs` — `Action`

The single enum that all app logic communicates through:

- Infrastructure: `Render`, `Quit`, `Suspend`, `Resume`, `Resize`, `ClearScreen`
- UI state: `FocusComponent(ComponentId)`, `ToggleFullScreen`, `ToggleDebug`, `ToggleHelp`
- Metric payloads: `CpuUpdate(CpuSnapshot)`, `MemUpdate`, `NetUpdate`, `DiskUpdate`,
  `ProcUpdate`, `SysUpdate` — these carry the snapshot structs from the stats collector
  to every component
- Debug: `DebugSnapshot(String)`

`Action` derives `Clone` for cases where the app needs to re-queue an action (e.g.
`Render`). It is **not** cloned to fan-out to components — `App::handle_actions` passes
`&action` to each component's `update` method. Payload variants are `#[serde(skip)]`
because the snapshot structs are not serializable.

### `src/tui.rs` — `Tui`

Wraps the `ratatui::Terminal` and owns the crossterm event loop Tokio task.
Handles alternate screen, raw mode, mouse capture, and clean teardown.
`Tui::next_event()` is how `App::handle_events` reads the next input event.

### `src/layout.rs` — `LayoutPreset` / `SlotId` / `SlotMap`

The layout engine. `LayoutPreset` has four variants:

- `Sidebar` (default) — 35% left column (CPU / Net / Disk), 65% right (Process)
- `Classic` — top 45% split left/right, bottom 55% for Process
- `Dashboard` — CPU full-width top, two small panels mid, Process fills rest
- `Grid` — two equal columns, each split in two

`LayoutPreset::compute(area, overrides, hints) → SlotMap` maps each `SlotId` to a
`(ComponentId, Rect)` pair. `LayoutHints` carries preferred heights from components
(e.g. CPU reports the exact rows it needs) so panels are tight to their content
instead of using fixed percentages. `SlotOverrides` allows individual slots to be
reassigned to a different component via config.

### `src/components/mod.rs` — `Component` trait

Every panel implements:

```rust
fn set_focused(&mut self, focused: bool) {}          // called before every render
fn preferred_height(&self) -> Option<u16> { None }   // layout hint
fn handle_key_event(&mut self, key) -> Result<Option<Action>>
fn update(&mut self, action: &Action) -> Result<Option<Action>>
fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()>
```

Note that `update` receives `&Action` — the action is not cloned per component.

#### Fullscreen state isolation

The App renders every visible component in the compact sidebar layout first, then
renders the fullscreen-focused component again as a modal overlay in the same frame.
Because both passes call the same component instance, a two-part mechanism prevents
fullscreen interactions (navigation, sort, filter) from affecting the compact sidebar.

**Part 1 — save-and-restore on enter/exit:**

- On `Action::ToggleFullScreen` while entering: a `compact_snapshot` struct is frozen,
  capturing selection index, filter string, sort state, sub-mode, and (for Process)
  the displayed row list.
- On `Action::ToggleFullScreen` while exiting, or on `set_focused(false)` mid-fullscreen:
  the snapshot is restored and cleared, returning the component to its pre-fullscreen state.

**Part 2 — render-time isolation via `begin_overlay_render()`:**

`App::render()` calls `comp.begin_overlay_render()` immediately before the fullscreen
overlay draw. Components set a one-shot `rendering_as_overlay: bool` flag in that
method. Inside `draw()`, the flag is consumed:

- `is_fullscreen=true, rendering_as_overlay=false` → compact background pass:
  component temporarily swaps in snapshot state, calls `draw()` with `is_fullscreen=false`
  (which prevents re-entry and disables overlay-only rendering features), then restores
  live state. The compact sidebar always shows frozen pre-fullscreen content.
- `is_fullscreen=true, rendering_as_overlay=true` → overlay pass: component renders
  normally with live state, showing the current fullscreen view to the user.

Key events are not involved in this isolation: `App::handle_events` already dispatches
key events only to the focused component, which is the fullscreen modal when active.
The compact "view" never receives key events during fullscreen; it only needs render
isolation, which Part 2 provides.

The snapshot covers all mutable UI state the user can change during fullscreen.
This keeps the compact sidebar pixel-identical to how it was before the overlay
was opened.

`ComponentId` is a `strum`-derived enum with `#[strum(serialize_all = "lowercase")]`;
the lowercase string representations (`"cpu"`, `"mem"`, `"net"`, `"disk"`,
`"process"`) are what appear in config and CLI flags.

#### Shared view utilities

`src/components/mod.rs` also exports shared types used by the Net and Disk panels:

- **`ListView`** — the three-state view enum (`List`, `Filter { input }`, `Detail { name }`)
  shared by `NetComponent` and `DiskComponent` instead of per-component duplicates
- **`FilterEvent`** — result of a filter-mode keypress (`Clear`, `Commit`, `Update(String)`,
  `Ignored(String)`)
- **`FilterInput`** — stateless struct; `FilterInput::handle_key(input, key) → FilterEvent`
  is the shared filter key handler used by Net, Disk, and CPU panels
- **`handle_detail_key(key, is_fullscreen, view)`** — shared helper for the Detail arm:
  Esc/q/Q returns to `ListView::List` (toggling fullscreen if active), all other keys
  are swallowed
- **`list_border_block(focus_key, rest, palette, focused)`** — shared border/title builder
  for the Net and Disk list panels

### `src/components/` — Panels

| File             | Component            | Key behaviour                                                    |
| ---------------- | -------------------- | ---------------------------------------------------------------- |
| `cpu.rs`         | `CpuComponent`       | Per-core line chart + aggregate gauge; per-core temps on Linux; reports `preferred_height` |
| `net.rs`         | `NetComponent`       | Per-interface RX/TX table; uses shared `ListView` + `FilterInput` |
| `disk.rs`        | `DiskComponent`      | Per-device read/write/usage table; uses shared `ListView` + `FilterInput` |
| `process/mod.rs` | `ProcessComponent`   | Sortable process table with state machine (see below)            |
| `status_bar.rs`  | `StatusBarComponent` | Top/bottom strip: hostname, uptime, load avg, time               |
| `debug.rs`       | `DebugComponent`     | Right-side debug sidebar; receives `DebugSnapshot` action        |
| `help.rs`        | `HelpComponent`      | Full-screen overlay listing all keybindings                      |

#### `ProcessComponent` state machine

The process panel has an explicit `ProcessState` enum:

- `NormalList` — default scrollable table
- `FilterMode { input }` — incremental filter bar, `Esc` returns to `NormalList`
- `DetailView { pid }` — expanded single-process view rendered as a two-column name/value inspector
- `KillConfirm { pid, name }` — confirmation dialog before sending `SIGKILL`
- `KillError { message }` — error dialog if kill fails

Sorting (`process/sort.rs`) and filtering (`process/filter.rs`) are in separate
submodules. Sorting supports all visible columns; the set of visible columns changes
between compact (< 120 cols) and wide layouts, and `is_wide_layout` is stored in the
component so key handlers use the correct column order.

### `src/stats/`

#### `stats/mod.rs` — Collector

`spawn_collector(tx, token, refresh_ms, thread_refresh_ms)` launches a Tokio task
that runs two `tokio::time::interval` timers:

- **Fast interval** (`refresh_ms`, default 1s) — refreshes `sysinfo::System`,
  `Networks`, `Disks`, and `Components`, then builds and sends six
  `Action::*Update` variants. Process data includes the full process list but
  *not* per-process threads.
- **Slow interval** (`thread_refresh_ms`, default 5s) — enumerates threads for
  every process via `/proc/<pid>/task/` (Linux only). The resulting thread entries
  are cached and merged into every subsequent `ProcUpdate` on the fast cadence.

This dual-interval design avoids thousands of `/proc` syscalls on every tick while
still keeping thread data visible in the UI.

Linux-specific metrics (`package_temp`, `per_core_temp`, `swap_in/out_bytes`,
per-process `priority/nice/threads/shr_bytes`, CPU user/system split, page faults,
context switches, tty, swap, fd count, and `/proc/<pid>/io` counters) are guarded by
`#[cfg(target_os = "linux")]` and read from hwmon sensors (via `sysinfo`),
`/proc/vmstat`, sysfs CPU topology, and `procfs` respectively.

Process snapshots combine cross-platform `sysinfo` fields (name, cmdline, RSS,
virtual memory, start/runtime, executable path, cwd, root, user/group IDs,
session ID, and cumulative disk I/O bytes) with Linux-only `procfs` fields.
The process detail modal currently surfaces the highest-value per-process fields in
two columns: identity, CPU, memory, runtime, filesystem paths, scheduling,
fault counters, context switches, descriptor count, and detailed I/O counters.

Per-core temperatures are mapped from physical core sensors (coretemp hwmon
labels like "Core 0") to logical core indices via
`/sys/devices/system/cpu/cpuN/topology/core_id`. Hyperthreaded siblings share
their physical core's temperature reading.

#### `stats/snapshots.rs` — Snapshot structs

Plain data structs — no logic, only fields. Each has a `.stub()` constructor used
in tests to avoid depending on the live stats collector. The structs are:

`SysSnapshot`, `CpuSnapshot` (includes `per_core_temp` and `package_temp` on
Linux), `MemSnapshot`, `NetSnapshot` / `InterfaceSnapshot`,
`DiskSnapshot` / `DiskDeviceSnapshot`, `ProcSnapshot` / `ProcessEntry` / `ProcessStatus`.

`ProcessEntry` now includes both list-view fields and detail-only fields. The list
uses a compact subset for table rendering and sorting; the detail inspector reads the
same cached `ProcessEntry` and renders additional metadata such as `exe`, `cwd`,
`root`, effective IDs, session ID, tty, user/system CPU time split, minor/major
faults, context-switch counters, open file-descriptor count, swap usage, and
`/proc/<pid>/io` syscall and character-byte counters.

### `src/config.rs` — `Config`

Loaded from `~/.config/dreidel/config.toml` (XDG). All fields have serde defaults
so a missing file is fine. Sub-structs:

- `GeneralConfig` — `refresh_rate_ms` (humantime), `thread_refresh_ms` (humantime,
  default 5s), `theme`, `channel_capacity`
- `LayoutConfig` — `preset`, `status_bar` position, `show` component list, per-slot
  overrides
- `ProcessConfig` — `default_sort`, `default_sort_dir`, `show_tree`
- `KeyBindings` — all focus/action keys as `char` fields

### `src/theme.rs` — `Theme` / `ColorPalette`

`Theme` is a three-variant enum (`Auto`, `Light`, `Dark`). `Theme::palette()` returns
a `ColorPalette` with named semantic roles: `fg`, `bg`, `border`, `accent`, `warn`,
`critical`, `dim`, `highlight`. All components receive a cloned `ColorPalette` at
construction time; they do not query a global.

### `src/cli.rs` — CLI

`clap` derive-based struct. Flags:

- `--config <path>` — override config file location
- `--init-config` — print the default config template and exit
- `--refresh-rate <RATE>` — override refresh interval (e.g. `500ms`, `2s`)
- `--thread-refresh <RATE>` — override thread enumeration interval (e.g. `5s`, `10s`)
- `--show` / `--hide` — override which components are visible (comma-separated
  `ComponentId` strings)

### `src/errors.rs`

`thiserror`-based error types for the crate's public error surface.

## Testing

Tests live in two places:

1. **Inline `#[cfg(test)]` modules** — unit tests in the same file as the code.
   Snapshot structs have `.stub()` constructors so tests never touch the live
   stats collector.

2. **`tests/snapshots.rs`** — integration-style tests that call `comp.draw()` on a
   `ratatui::backend::TestBackend` and assert the rendered buffer with
   `insta::assert_snapshot!`. Snapshots live in `src/components/snapshots/`.

To regenerate snapshots after an intentional rendering change:

```bash
INSTA_UPDATE=always cargo test
```

## Adding a New Component

1. Create `src/components/mywidget.rs` and implement the `Component` trait.
2. Add a `MyWidget` variant to `ComponentId` in `src/components/mod.rs`.
3. Register the component in `App::new()` in `src/app.rs`.
4. Add a `SlotId` variant and wire it into at least one `LayoutPreset` in
   `src/layout.rs`.
5. If the component needs data, add an `Action::MyWidgetUpdate(MySnapshot)` variant
   to `src/action.rs` and build the snapshot in `src/stats/mod.rs`.
6. Add a `.stub()` constructor to the snapshot struct and write snapshot tests.

## Logging

All log output goes to `~/.local/share/dreidel/dreidel.log` via `tracing`. Nothing
is written to stderr — stderr would corrupt the alternate-screen TUI.
