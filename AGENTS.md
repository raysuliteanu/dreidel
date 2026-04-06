# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Source Control

This repo uses **jj (jujutsu)** in co-located mode — always use `jj` commands, never `git commit`. Commits use conventional-commits format (`feat:`, `fix:`, `refactor:`, etc.).

```bash
jj status
jj diff
jj commit -m 'fix: description'
```

## Build & Test

```bash
cargo build
cargo test                          # all tests
cargo test components::cpu          # filter by module/name
INSTA_UPDATE=always cargo test      # accept updated insta snapshots
cargo run -- --help                 # CLI flags
cargo run -- --init-config          # print default config template
```

## Hooks (auto-run, no action needed)

- **PostToolUse on `.rs` edit**: `cargo clippy -D warnings` — blocks on any warning. Fix before moving to the next file.
- **Pre-push**: `cargo fmt --check` — ensure code is formatted before pushing.

Because clippy fires after every single file save, multi-file changes that create temporary broken states will block. Strategy: complete the full implementation chain within one file before saving, or temporarily add `#[allow(dead_code)]` / `#[allow(unused_imports)]` and remove it once the chain is wired up.

## Architecture

The codebase is a dual lib+bin crate (`src/lib.rs` exports modules; `src/main.rs` is the binary entry point).

### Data flow

```
sysinfo → stats/mod.rs (spawn_collector) → Action::*Update → App::handle_actions
                                                              → Component::update()
                                                              → render() → draw()
```

`spawn_collector` owns a `sysinfo::System` in a background Tokio task and sends typed `Action::*Update(Snapshot)` variants on a bounded `mpsc::Sender<Action>`. It runs two intervals: a **fast interval** (`refresh_rate_ms`, default 1s) that refreshes CPU/mem/net/disk/process data, and a **slow interval** (`thread_refresh_ms`, default 5s) that enumerates per-process threads via `/proc/<pid>/task/`. Thread entries are cached between slow ticks and merged into every `ProcUpdate`. `App` is the sole receiver.

### Action bus

`action.rs` defines the `Action` enum. All app logic communicates via this channel. Key variants:
- `*Update(Snapshot)` — metric payloads from the stats collector
- `FocusComponent(ComponentId)`, `ToggleFullScreen`, `ToggleHelp` — UI state
- `Render`, `Quit` — infrastructure

### Component trait

Every panel implements `src/components/mod.rs::Component`:

```rust
fn set_focused(&mut self, focused: bool) {}          // called before every render
fn preferred_height(&self) -> Option<u16> { None }   // compact layout hint
fn handle_key_event(&mut self, key) -> Result<Option<Action>>
fn update(&mut self, action: Action) -> Result<Option<Action>>
fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()>
```

`App::render()` calls `set_focused` on all components before drawing and reads `preferred_height` to compute `LayoutHints` for the sidebar left-column split. The `draw` closure captures pre-computed values — components are not accessible inside `tui.draw(|frame| { ... })`.

### Layout system

`layout.rs` maps `SlotId → (ComponentId, Rect)` via `LayoutPreset::compute(area, overrides, hints)`. The sidebar preset has 4 slots: `LeftTop`=CPU, `LeftBot`=Net, `LeftExtra`=Disk, `Right`=Process. `LayoutHints` carries a preferred height from CPU so that panel is tight to its content; Net/Disk split remaining height equally.

### Key dispatch

In `App::handle_events`, `Event::Key` goes to the *focused component* first via `handle_key_event`. If the component returns `Ok(None)`, the global handler runs (focus-switch keys `p/c/n/d`, `Tab`/`Shift-Tab`, `f` fullscreen, `?` help, `q` quit). Tab cycling uses `App::rendered_ids` — only components that have a layout slot AND are in `visible` — updated each render.

### ComponentId ↔ config string mapping

`ComponentId` uses `#[strum(serialize_all = "lowercase")]`. The strings used in `config.layout.show` and `--show`/`--hide` CLI flags must match exactly: `"cpu"`, `"net"`, `"disk"`, `"process"`. (`"mem"` and `"proc"` will silently fail to match.)

### Snapshot testing

Component `draw()` methods are tested with `ratatui::backend::TestBackend` + `insta::assert_snapshot!`. Snapshots live in `src/components/snapshots/`. When rendering changes intentionally, run `INSTA_UPDATE=always cargo test` to accept new snapshots, then verify the diff looks correct.

### Snapshot stubs

Each `*Snapshot` struct in `stats/snapshots.rs` has a `.stub()` constructor for use in tests, so tests don't depend on the stats collector.

## Key Conventions

- Use `.context("present tense phrase")` on every `?` propagation (`anyhow`).
- Use `expect("why this can't fail")` over `unwrap()`.
- `#[cfg(target_os = "linux")]` guards per-core and package temperature (CpuSnapshot), swap activity (MemSnapshot, `/proc/vmstat`), and thread enumeration.
- Logs go to `~/.local/share/dreidel/dreidel.log` (never stderr — would corrupt the TUI).
- Config file: `~/.config/dreidel/config.toml` (TOML, all fields optional with serde defaults).
