# toppers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a bpytop-inspired TUI system monitor in Rust with CPU, memory, network, disk, and process components using a ratatui component-actor architecture.

**Architecture:** A central `App` event loop holds all component actors in-process and fans out `Action` variants to each via synchronous `update()` calls. A separate Tokio task (stats collector) owns `sysinfo::System`, refreshes metrics on each tick, and sends typed snapshot actions into a single bounded `mpsc` channel. Components own their UI state; no shared mutable state exists between actors.

**Tech Stack:** Rust 2024, ratatui, crossterm, tokio, sysinfo, clap, serde+toml, humantime, tracing, thiserror, anyhow, chrono, strum, insta (dev)

---

## File Structure

```
src/
├── main.rs                   # CLI parse, config load, tokio runtime, launch App
├── action.rs                 # Action enum (all message types)
├── app.rs                    # App struct, FocusState, event loop
├── tui.rs                    # Tui struct — terminal setup/teardown, crossterm event stream
├── config.rs                 # Config, KeyBindings — serde+toml deserialization
├── cli.rs                    # clap CLI definition (Args struct)
├── errors.rs                 # AppError via thiserror
├── theme.rs                  # Theme enum, ColorPalette
├── layout.rs                 # LayoutPreset, SlotMap, Rect allocation per preset
├── components/
│   ├── mod.rs                # Component trait, ComponentId enum
│   ├── status_bar.rs         # StatusBarComponent
│   ├── cpu.rs                # CpuComponent (sparkline ring buffers)
│   ├── mem.rs                # MemComponent
│   ├── net.rs                # NetComponent (scrollable interface list)
│   ├── disk.rs               # DiskComponent (scrollable device list)
│   ├── debug.rs              # DebugComponent (state inspector sidebar)
│   └── process/
│       ├── mod.rs            # ProcessComponent, ProcessState machine
│       ├── filter.rs         # ProcessFilter — PID/name/state matching
│       └── sort.rs           # SortColumn enum, sort comparators
└── stats/
    ├── mod.rs                # spawn_collector() — starts the tokio task
    └── snapshots.rs          # CpuSnapshot, MemSnapshot, NetSnapshot, DiskSnapshot,
                              #   SysSnapshot, ProcSnapshot, ProcessEntry
```

**Source control:** This project uses `jj` (jujutsu) co-located with git. Use `jj commit -m "..."` for all commits, never `git commit`.

**Lint/format:** `cargo fmt` and `cargo clippy -- -D warnings` must pass before each commit. A pre-push hook runs `cargo fmt --check` automatically.

---

## Phase 1: Foundation

### Task 1: Cargo.toml dependencies and project scaffold

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/main.rs`
- Create: `src/errors.rs`

- [ ] **Step 1: Add all dependencies**

```bash
cargo add ratatui crossterm tokio --features tokio/full
cargo add sysinfo clap --features clap/derive
cargo add serde --features serde/derive
cargo add toml humantime tracing tracing-subscriber anyhow thiserror chrono
cargo add strum --features strum/derive
cargo add futures tokio-util
cargo add --dev insta
```

- [ ] **Step 2: Write a compile test — verify `cargo build` succeeds**

```bash
cargo build
```

Expected: compiles with no errors.

- [ ] **Step 3: Create `src/errors.rs`**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("config error: {0}")]
    Config(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("channel send error")]
    ChannelSend,
}
```

- [ ] **Step 4: Stub `src/main.rs`**

```rust
mod action;
mod app;
mod cli;
mod components;
mod config;
mod errors;
mod layout;
mod stats;
mod theme;
mod tui;

fn main() -> anyhow::Result<()> {
    println!("toppers");
    Ok(())
}
```

- [ ] **Step 5: Create stub modules so it compiles**

Create empty files: `src/action.rs`, `src/app.rs`, `src/cli.rs`, `src/config.rs`, `src/layout.rs`, `src/theme.rs`, `src/tui.rs`, `src/components/mod.rs`, `src/stats/mod.rs`

- [ ] **Step 6: Verify build**

```bash
cargo build
```

- [ ] **Step 7: Commit**

```bash
jj commit -m "chore: scaffold project structure and add dependencies"
```

---

### Task 2: Snapshot types

**Files:**
- Create: `src/stats/snapshots.rs`
- Modify: `src/stats/mod.rs`
- Modify: `src/action.rs`

- [ ] **Step 1: Write tests for snapshot construction**

In `src/stats/snapshots.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_snapshot_stub_has_expected_shape() {
        let s = CpuSnapshot::stub();
        assert!(!s.per_core.is_empty());
        assert!(s.aggregate >= 0.0 && s.aggregate <= 100.0);
    }

    #[test]
    fn mem_snapshot_used_never_exceeds_total() {
        let s = MemSnapshot::stub();
        assert!(s.ram_used <= s.ram_total);
        assert!(s.swap_used <= s.swap_total);
    }
}
```

- [ ] **Step 2: Run tests — expect compile failure**

```bash
cargo test -p toppers stats::snapshots
```

- [ ] **Step 3: Implement `src/stats/snapshots.rs`**

```rust
#[derive(Debug, Clone)]
pub struct CpuSnapshot {
    pub per_core:  Vec<f32>,   // 0.0–100.0 per logical core
    pub aggregate: f32,
    pub frequency: Vec<u64>,   // MHz per core
    #[cfg(target_os = "linux")]
    pub temperature: Option<f32>, // degrees C
}

impl CpuSnapshot {
    pub fn stub() -> Self {
        Self {
            per_core:  vec![42.0, 18.0, 75.0, 5.0],
            aggregate: 35.0,
            frequency: vec![3400, 3400, 3400, 3400],
            #[cfg(target_os = "linux")]
            temperature: Some(62.0),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemSnapshot {
    pub ram_used:  u64,
    pub ram_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    #[cfg(target_os = "linux")]
    pub swap_in_bytes:  u64,
    #[cfg(target_os = "linux")]
    pub swap_out_bytes: u64,
}

impl MemSnapshot {
    pub fn stub() -> Self {
        Self {
            ram_used:   6_442_450_944,
            ram_total: 17_179_869_184,
            swap_used:  0,
            swap_total: 4_294_967_296,
            #[cfg(target_os = "linux")]
            swap_in_bytes:  0,
            #[cfg(target_os = "linux")]
            swap_out_bytes: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InterfaceSnapshot {
    pub name:     String,
    pub rx_bytes: u64,  // bytes/s since last tick
    pub tx_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct NetSnapshot {
    pub interfaces: Vec<InterfaceSnapshot>,
}

impl NetSnapshot {
    pub fn stub() -> Self {
        Self {
            interfaces: vec![
                InterfaceSnapshot { name: "eth0".into(), rx_bytes: 4_800_000, tx_bytes: 1_200_000 },
            ],
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiskDeviceSnapshot {
    pub name:        String,
    pub read_bytes:  u64,   // bytes/s
    pub write_bytes: u64,
    pub usage_pct:   f32,   // 0.0–100.0
}

#[derive(Debug, Clone)]
pub struct DiskSnapshot {
    pub devices: Vec<DiskDeviceSnapshot>,
}

impl DiskSnapshot {
    pub fn stub() -> Self {
        Self {
            devices: vec![
                DiskDeviceSnapshot { name: "sda".into(), read_bytes: 0, write_bytes: 102_400, usage_pct: 45.0 },
            ],
        }
    }
}

#[derive(Debug, Clone)]
pub struct SysSnapshot {
    pub hostname:  String,
    pub uptime:    u64,         // seconds
    pub load_avg:  [f64; 3],    // 1m, 5m, 15m
    pub timestamp: chrono::DateTime<chrono::Local>,
}

impl SysSnapshot {
    pub fn stub() -> Self {
        Self {
            hostname:  "dev-box".into(),
            uptime:    273_600,
            load_avg:  [1.24, 0.98, 0.87],
            timestamp: chrono::Local::now(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProcessEntry {
    pub pid:        u32,
    pub name:       String,
    pub cmd:        Vec<String>,
    pub user:       String,
    pub cpu_pct:    f32,
    pub mem_bytes:  u64,
    pub mem_pct:    f32,
    pub virt_bytes: u64,
    pub status:     String,
    pub start_time: u64,   // unix timestamp
    pub run_time:   u64,   // seconds
    pub nice:       i32,
    pub threads:    u32,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub parent_pid: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ProcSnapshot {
    pub processes: Vec<ProcessEntry>,
}

impl ProcSnapshot {
    pub fn stub() -> Self {
        Self {
            processes: vec![
                ProcessEntry {
                    pid: 12345, name: "firefox".into(),
                    cmd: vec!["firefox".into()], user: "ray".into(),
                    cpu_pct: 18.4, mem_bytes: 536_870_912, mem_pct: 3.2,
                    virt_bytes: 2_147_483_648, status: "running".into(),
                    start_time: 0, run_time: 3600, nice: 0, threads: 42,
                    read_bytes: 0, write_bytes: 0, parent_pid: Some(1),
                },
            ],
        }
    }
}
```

- [ ] **Step 4: Export from `src/stats/mod.rs`**

```rust
pub mod snapshots;
pub use snapshots::*;
```

- [ ] **Step 5: Write `src/action.rs`**

```rust
use crate::stats::snapshots::*;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Debug, Clone, Display, Serialize, Deserialize)]
pub enum Action {
    // Infrastructure
    Tick,
    Render,
    Quit,
    Suspend,
    Resume,
    ClearScreen,
    Resize(u16, u16),
    Error(String),
    // Focus
    FocusComponent(crate::components::ComponentId),
    ToggleFullScreen,
    ToggleDebug,
    // Metric updates (from stats collector)
    #[serde(skip)]
    SysUpdate(SysSnapshot),
    #[serde(skip)]
    CpuUpdate(CpuSnapshot),
    #[serde(skip)]
    MemUpdate(MemSnapshot),
    #[serde(skip)]
    NetUpdate(NetSnapshot),
    #[serde(skip)]
    DiskUpdate(DiskSnapshot),
    #[serde(skip)]
    ProcUpdate(ProcSnapshot),
}

impl PartialEq for Action {
    fn eq(&self, other: &Self) -> bool {
        // Used for filtering Tick/Render from debug logs
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}
```

- [ ] **Step 6: Run tests**

```bash
cargo test
```

- [ ] **Step 7: Commit**

```bash
jj commit -m "feat: add snapshot types and Action enum"
```

---

### Task 3: Theme and Config

**Files:**
- Create: `src/theme.rs`
- Create: `src/config.rs`

- [ ] **Step 1: Write tests**

```rust
// In src/config.rs #[cfg(test)]
#[test]
fn default_config_is_valid() {
    let c = Config::default();
    assert_eq!(c.general.refresh_rate_ms, 1000);
}

#[test]
fn config_parses_from_toml() {
    let toml = r#"
        [general]
        refresh_rate = "2s"
        theme = "dark"
    "#;
    let c: Config = toml::from_str(toml).unwrap();
    assert_eq!(c.general.theme, Theme::Dark);
}
```

- [ ] **Step 2: Implement `src/theme.rs`**

```rust
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Display, EnumString, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Theme {
    #[default]
    Auto,
    Light,
    Dark,
}

#[derive(Debug, Clone)]
pub struct ColorPalette {
    pub bg:          ratatui::style::Color,
    pub fg:          ratatui::style::Color,
    pub border:      ratatui::style::Color,
    pub accent:      ratatui::style::Color,
    pub warn:        ratatui::style::Color,
    pub critical:    ratatui::style::Color,
    pub dim:         ratatui::style::Color,
    pub highlight:   ratatui::style::Color,
}

impl ColorPalette {
    pub fn dark() -> Self {
        use ratatui::style::Color::*;
        Self {
            bg: Rgb(26, 27, 38), fg: Rgb(192, 202, 245),
            border: Rgb(65, 72, 104), accent: Rgb(122, 162, 247),
            warn: Rgb(224, 175, 104), critical: Rgb(247, 118, 142),
            dim: Rgb(86, 95, 137), highlight: Rgb(187, 154, 247),
        }
    }

    pub fn light() -> Self {
        use ratatui::style::Color::*;
        Self {
            bg: White, fg: Black,
            border: Rgb(180, 180, 180), accent: Rgb(0, 100, 200),
            warn: Rgb(180, 100, 0), critical: Rgb(180, 0, 0),
            dim: Rgb(120, 120, 120), highlight: Rgb(80, 0, 180),
        }
    }
}

impl Theme {
    pub fn palette(self) -> ColorPalette {
        match self {
            Theme::Dark | Theme::Auto => ColorPalette::dark(),
            Theme::Light => ColorPalette::light(),
        }
    }
}
```

- [ ] **Step 3: Implement `src/config.rs`**

```rust
use crate::theme::Theme;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct GeneralConfig {
    #[serde(deserialize_with = "deserialize_duration", default = "default_refresh")]
    pub refresh_rate_ms: u64,
    pub theme:            Theme,
    pub channel_capacity: usize,
}

fn deserialize_duration<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let s = String::deserialize(d)?;
    humantime::parse_duration(&s)
        .map(|d| d.as_millis() as u64)
        .map_err(serde::de::Error::custom)
}

fn default_refresh() -> u64 { 1000 }

impl Default for GeneralConfig {
    fn default() -> Self {
        Self { refresh_rate_ms: 1000, theme: Theme::Auto, channel_capacity: 128 }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct LayoutConfig {
    pub preset:     String,
    pub status_bar: String,
    pub show:       Vec<String>,
    // Slot overrides — optional
    pub left_top:         Option<String>,
    pub left_mid:         Option<String>,
    pub left_bot:         Option<String>,
    pub right:            Option<String>,
    pub top_left:         Option<String>,
    pub top_right_top:    Option<String>,
    pub top_right_bot:    Option<String>,
    pub bottom:           Option<String>,
    pub top:              Option<String>,
    pub mid_left:         Option<String>,
    pub mid_right:        Option<String>,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            preset: "sidebar".into(),
            status_bar: "top".into(),
            show: vec!["cpu".into(), "mem".into(), "net".into(), "disk".into(), "proc".into()],
            left_top: None, left_mid: None, left_bot: None, right: None,
            top_left: None, top_right_top: None, top_right_bot: None, bottom: None,
            top: None, mid_left: None, mid_right: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ProcessConfig {
    pub default_sort:     String,
    pub default_sort_dir: String,
    pub show_tree:        bool,
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self { default_sort: "cpu".into(), default_sort_dir: "desc".into(), show_tree: false }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct KeyBindings {
    pub focus_proc: char,
    pub focus_cpu:  char,
    pub focus_mem:  char,
    pub focus_net:  char,
    pub focus_disk: char,
    pub fullscreen: char,
    pub help:       char,
    pub debug:      char,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            focus_proc: 'p', focus_cpu: 'c', focus_mem: 'm',
            focus_net: 'n', focus_disk: 'd',
            fullscreen: 'f', help: '?', debug: '`',
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub general:     GeneralConfig,
    pub layout:      LayoutConfig,
    pub process:     ProcessConfig,
    pub keybindings: KeyBindings,
}

impl Config {
    /// Load from XDG config path: ~/.config/toppers/config.toml
    pub fn load(path: Option<&std::path::Path>) -> anyhow::Result<Self> {
        use anyhow::Context;
        let default_path = dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("toppers/config.toml");
        let path = path.unwrap_or(&default_path);
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        toml::from_str(&contents)
            .with_context(|| format!("parsing config {}", path.display()))
    }
}
```

- [ ] **Step 4: Add `dirs` crate for XDG path**

```bash
cargo add dirs
```

- [ ] **Step 5: Run tests**

```bash
cargo test
```

- [ ] **Step 6: Commit**

```bash
jj commit -m "feat: add theme and config types"
```

---

### Task 4: CLI with clap

**Files:**
- Create: `src/cli.rs`

- [ ] **Step 1: Write test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_theme_flag() {
        let args = Args::try_parse_from(["toppers", "--theme", "dark"]).unwrap();
        assert_eq!(args.theme, Some("dark".to_string()));
    }

    #[test]
    fn init_config_flag_parses() {
        let args = Args::try_parse_from(["toppers", "--init-config"]).unwrap();
        assert!(args.init_config);
    }
}
```

- [ ] **Step 2: Implement `src/cli.rs`**

```rust
use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "toppers", about = "A modern TUI system monitor", version)]
pub struct Args {
    #[arg(long, help = "Color theme: auto | light | dark")]
    pub theme: Option<String>,

    #[arg(long, value_name = "RATE", help = "Refresh rate e.g. 500ms, 1s, 2s")]
    pub refresh_rate: Option<String>,

    #[arg(long, value_name = "LAYOUT", help = "Layout preset: sidebar | classic | dashboard")]
    pub preset: Option<String>,

    #[arg(long, value_delimiter = ',', value_name = "COMPONENTS",
          help = "Components to show: cpu,mem,net,disk,proc")]
    pub show: Option<Vec<String>>,

    #[arg(long, value_delimiter = ',', value_name = "COMPONENTS",
          help = "Components to hide (hide wins over --show)")]
    pub hide: Option<Vec<String>>,

    #[arg(long, value_name = "POS", help = "Status bar position: top | bottom | hidden")]
    pub status_bar: Option<String>,

    #[arg(long, value_name = "PATH", help = "Alternate config file path")]
    pub config: Option<std::path::PathBuf>,

    #[arg(long, help = "Print default config to stdout and exit")]
    pub init_config: bool,

    #[arg(long, help = "Show debug sidebar on startup")]
    pub debug: bool,

    #[arg(short, long, action = clap::ArgAction::Count, help = "Increase verbosity (-v, -vv)")]
    pub verbose: u8,
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test cli
```

- [ ] **Step 4: Commit**

```bash
jj commit -m "feat: add clap CLI definition"
```

---

## Phase 2: TUI Infrastructure

### Task 5: Tui struct and Component trait

**Files:**
- Create: `src/tui.rs` (port from sprintrs pattern)
- Create: `src/components/mod.rs`

- [ ] **Step 1: Implement `src/tui.rs`**

Port directly from sprintrs with these changes:
- Use bounded channel for events: `tokio::sync::mpsc::channel(128)` instead of unbounded
- Keep all the same `Event` variants, `enter()`, `exit()`, `next_event()`, `suspend()`, `resume()`

```rust
use std::{io::{stdout, Stdout}, ops::{Deref, DerefMut}, time::Duration};
use anyhow::Context;
use crossterm::{
    cursor,
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste,
            EnableMouseCapture, Event as CrosstermEvent, EventStream,
            KeyEvent, KeyEventKind, MouseEvent},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::{FutureExt, StreamExt};
use ratatui::backend::CrosstermBackend as Backend;
use serde::{Deserialize, Serialize};
use tokio::{sync::mpsc, task::JoinHandle, time::interval};
use tokio_util::sync::CancellationToken;
use tracing::error;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Event {
    Init, Quit, Error, Closed, Tick, Render,
    FocusGained, FocusLost, Paste(String),
    Key(KeyEvent), Mouse(MouseEvent), Resize(u16, u16),
}

pub struct Tui {
    pub terminal:           ratatui::Terminal<Backend<Stdout>>,
    pub task:               JoinHandle<()>,
    pub cancellation_token: CancellationToken,
    pub event_rx:           mpsc::Receiver<Event>,
    pub event_tx:           mpsc::Sender<Event>,
    pub frame_rate:         f64,
    pub tick_rate:          f64,
    pub mouse:              bool,
    pub paste:              bool,
}

impl Tui {
    pub fn new() -> anyhow::Result<Self> {
        let (event_tx, event_rx) = mpsc::channel(256);
        Ok(Self {
            terminal: ratatui::Terminal::new(Backend::new(stdout()))
                .context("creating terminal")?,
            task: tokio::spawn(async {}),
            cancellation_token: CancellationToken::new(),
            event_rx, event_tx,
            frame_rate: 60.0, tick_rate: 4.0,
            mouse: false, paste: false,
        })
    }

    pub fn tick_rate(mut self, r: f64) -> Self   { self.tick_rate = r; self }
    pub fn frame_rate(mut self, r: f64) -> Self  { self.frame_rate = r; self }
    pub fn mouse(mut self, m: bool) -> Self       { self.mouse = m; self }

    pub fn start(&mut self) {
        self.cancel();
        self.cancellation_token = CancellationToken::new();
        let event_loop = Self::event_loop(
            self.event_tx.clone(), self.cancellation_token.clone(),
            self.tick_rate, self.frame_rate,
        );
        self.task = tokio::spawn(event_loop);
    }

    async fn event_loop(
        tx: mpsc::Sender<Event>, token: CancellationToken,
        tick_rate: f64, frame_rate: f64,
    ) {
        let mut stream = EventStream::new();
        let mut tick   = interval(Duration::from_secs_f64(1.0 / tick_rate));
        let mut render = interval(Duration::from_secs_f64(1.0 / frame_rate));

        let _ = tx.send(Event::Init).await;
        loop {
            let event = tokio::select! {
                _ = token.cancelled() => break,
                _ = tick.tick()   => Event::Tick,
                _ = render.tick() => Event::Render,
                ev = stream.next().fuse() => match ev {
                    Some(Ok(CrosstermEvent::Key(k))) if k.kind == KeyEventKind::Press => Event::Key(k),
                    Some(Ok(CrosstermEvent::Mouse(m)))    => Event::Mouse(m),
                    Some(Ok(CrosstermEvent::Resize(x, y))) => Event::Resize(x, y),
                    Some(Ok(CrosstermEvent::FocusLost))   => Event::FocusLost,
                    Some(Ok(CrosstermEvent::FocusGained)) => Event::FocusGained,
                    Some(Ok(CrosstermEvent::Paste(s)))    => Event::Paste(s),
                    Some(Err(_)) => Event::Error,
                    None => break,
                    _ => continue,
                },
            };
            if tx.send(event).await.is_err() { break; }
        }
        token.cancel();
    }

    pub fn stop(&self) -> anyhow::Result<()> {
        self.cancel();
        let mut counter = 0u32;
        while !self.task.is_finished() {
            std::thread::sleep(Duration::from_millis(1));
            counter += 1;
            if counter > 50 { self.task.abort(); }
            if counter > 100 {
                error!("Tui task did not stop within 100ms");
                break;
            }
        }
        Ok(())
    }

    pub fn enter(&mut self) -> anyhow::Result<()> {
        crossterm::terminal::enable_raw_mode().context("enabling raw mode")?;
        crossterm::execute!(stdout(), EnterAlternateScreen, cursor::Hide)
            .context("entering alternate screen")?;
        if self.mouse {
            crossterm::execute!(stdout(), EnableMouseCapture)
                .context("enabling mouse capture")?;
        }
        self.start();
        Ok(())
    }

    pub fn exit(&mut self) -> anyhow::Result<()> {
        self.stop()?;
        if crossterm::terminal::is_raw_mode_enabled().unwrap_or(false) {
            self.flush().context("flushing terminal")?;
            if self.mouse {
                crossterm::execute!(stdout(), DisableMouseCapture)?;
            }
            crossterm::execute!(stdout(), LeaveAlternateScreen, cursor::Show)?;
            crossterm::terminal::disable_raw_mode().context("disabling raw mode")?;
        }
        Ok(())
    }

    pub fn cancel(&self) { self.cancellation_token.cancel(); }
    pub fn size(&self) -> anyhow::Result<ratatui::layout::Size> {
        Ok(self.terminal.size().context("getting terminal size")?)
    }
    pub async fn next_event(&mut self) -> Option<Event> { self.event_rx.recv().await }

    pub fn resize(&mut self, rect: ratatui::layout::Rect) -> anyhow::Result<()> {
        self.terminal.resize(rect).context("resizing terminal")
    }
}

impl Deref for Tui {
    type Target = ratatui::Terminal<Backend<Stdout>>;
    fn deref(&self) -> &Self::Target { &self.terminal }
}

impl DerefMut for Tui {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.terminal }
}

impl Drop for Tui {
    fn drop(&mut self) { let _ = self.exit(); }
}
```

- [ ] **Step 2: Implement `src/components/mod.rs`**

```rust
use anyhow::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::{layout::{Rect, Size}, Frame};
use tokio::sync::mpsc::Sender;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter};

use crate::{action::Action, config::Config, tui::Event};

pub mod cpu;
pub mod debug;
pub mod disk;
pub mod mem;
pub mod net;
pub mod process;
pub mod status_bar;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumIter, Serialize, Deserialize)]
#[strum(serialize_all = "lowercase")]
pub enum ComponentId {
    Cpu,
    Mem,
    Net,
    Disk,
    Proc,
}

pub trait Component {
    fn register_action_handler(&mut self, tx: Sender<Action>) -> Result<()> {
        let _ = tx;
        Ok(())
    }
    fn register_config_handler(&mut self, config: Config) -> Result<()> {
        let _ = config;
        Ok(())
    }
    fn init(&mut self, area: Size) -> Result<()> {
        let _ = area;
        Ok(())
    }
    fn handle_events(&mut self, event: Option<Event>) -> Result<Option<Action>> {
        match event {
            Some(Event::Key(k)) => self.handle_key_event(k),
            Some(Event::Mouse(m)) => self.handle_mouse_event(m),
            _ => Ok(None),
        }
    }
    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        let _ = key;
        Ok(None)
    }
    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        let _ = mouse;
        Ok(None)
    }
    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        let _ = action;
        Ok(None)
    }
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()>;
}
```

- [ ] **Step 3: Create stub implementations for each component module**

Create files with a minimal struct and `impl Component` for: `src/components/status_bar.rs`, `src/components/cpu.rs`, `src/components/mem.rs`, `src/components/net.rs`, `src/components/disk.rs`, `src/components/debug.rs`, `src/components/process/mod.rs`, `src/components/process/filter.rs`, `src/components/process/sort.rs`

Each stub looks like:
```rust
use anyhow::Result;
use ratatui::{layout::Rect, Frame};
use crate::components::Component;

#[derive(Debug, Default)]
pub struct CpuComponent;  // (rename per file)

impl Component for CpuComponent {
    fn draw(&mut self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }
}
```

- [ ] **Step 4: Verify build**

```bash
cargo build
```

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat: add Tui struct and Component trait with stub implementations"
```

---

### Task 6: Layout system

**Files:**
- Create: `src/layout.rs`

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    #[test]
    fn sidebar_preset_allocates_right_column_to_proc() {
        let area = Rect::new(0, 0, 200, 50);
        let map = LayoutPreset::Sidebar.compute(area, &SlotOverrides::default());
        assert!(map.get(&SlotId::Right).is_some());
    }

    #[test]
    fn status_bar_reduces_available_area() {
        let area = Rect::new(0, 0, 200, 50);
        let (bar, rest) = split_status_bar(area, StatusBarPosition::Top);
        assert_eq!(bar.height, 1);
        assert_eq!(rest.height, 49);
    }
}
```

- [ ] **Step 2: Implement `src/layout.rs`**

```rust
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use strum::{Display, EnumString};

use crate::components::ComponentId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlotId {
    // sidebar
    LeftTop, LeftMid, LeftBot, Right,
    // classic
    TopLeft, TopRightTop, TopRightBot, Bottom,
    // dashboard
    Top, MidLeft, MidRight,
}

#[derive(Debug, Clone, Default)]
pub struct SlotOverrides(pub HashMap<SlotId, ComponentId>);

pub type SlotMap = HashMap<SlotId, (ComponentId, Rect)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Display, EnumString, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum LayoutPreset {
    #[default]
    Sidebar,
    Classic,
    Dashboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusBarPosition { Top, Bottom, Hidden }

pub fn split_status_bar(area: Rect, pos: StatusBarPosition) -> (Rect, Rect) {
    match pos {
        StatusBarPosition::Hidden => (Rect::default(), area),
        StatusBarPosition::Top => {
            let chunks = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)])
                .split(area);
            (chunks[0], chunks[1])
        }
        StatusBarPosition::Bottom => {
            let chunks = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)])
                .split(area);
            (chunks[1], chunks[0])
        }
    }
}

impl LayoutPreset {
    pub fn compute(&self, area: Rect, overrides: &SlotOverrides) -> SlotMap {
        let defaults = self.default_slots();
        let mut map = SlotMap::new();
        let rects = self.split_area(area);
        for (slot_id, rect) in rects {
            let component = overrides.0.get(&slot_id)
                .copied()
                .unwrap_or_else(|| *defaults.get(&slot_id).unwrap());
            map.insert(slot_id, (component, rect));
        }
        map
    }

    fn default_slots(&self) -> HashMap<SlotId, ComponentId> {
        use SlotId::*; use ComponentId::*;
        match self {
            Self::Sidebar => HashMap::from([
                (LeftTop, Cpu), (LeftMid, Mem), (LeftBot, Net), (Right, Proc),
            ]),
            Self::Classic => HashMap::from([
                (TopLeft, Cpu), (TopRightTop, Mem), (TopRightBot, Net), (Bottom, Proc),
            ]),
            Self::Dashboard => HashMap::from([
                (Top, Cpu), (MidLeft, Mem), (MidRight, Net), (Bottom, Proc),
            ]),
        }
    }

    fn split_area(&self, area: Rect) -> Vec<(SlotId, Rect)> {
        use SlotId::*;
        match self {
            Self::Sidebar => {
                let cols = Layout::horizontal([Constraint::Percentage(35), Constraint::Fill(1)])
                    .split(area);
                let left = Layout::vertical([
                    Constraint::Percentage(40), Constraint::Percentage(30), Constraint::Fill(1),
                ]).split(cols[0]);
                vec![
                    (LeftTop, left[0]), (LeftMid, left[1]), (LeftBot, left[2]),
                    (Right, cols[1]),
                ]
            }
            Self::Classic => {
                let rows = Layout::vertical([Constraint::Percentage(45), Constraint::Fill(1)])
                    .split(area);
                let top = Layout::horizontal([Constraint::Percentage(60), Constraint::Fill(1)])
                    .split(rows[0]);
                let top_right = Layout::vertical([Constraint::Percentage(50), Constraint::Fill(1)])
                    .split(top[1]);
                vec![
                    (TopLeft, top[0]), (TopRightTop, top_right[0]),
                    (TopRightBot, top_right[1]), (Bottom, rows[1]),
                ]
            }
            Self::Dashboard => {
                let rows = Layout::vertical([
                    Constraint::Length(5), Constraint::Length(8), Constraint::Fill(1),
                ]).split(area);
                let mid = Layout::horizontal([Constraint::Percentage(50), Constraint::Fill(1)])
                    .split(rows[1]);
                vec![(Top, rows[0]), (MidLeft, mid[0]), (MidRight, mid[1]), (Bottom, rows[2])]
            }
        }
    }
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test layout
```

- [ ] **Step 4: Commit**

```bash
jj commit -m "feat: add layout preset system with slot overrides"
```

---

## Phase 3: Stats Collector

### Task 7: Stats collector tokio task

**Files:**
- Create: `src/stats/mod.rs` (replace stub)

- [ ] **Step 1: Write integration test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn collector_sends_cpu_update() {
        let (tx, mut rx) = mpsc::channel(32);
        let token = tokio_util::sync::CancellationToken::new();
        let child = token.child_token();
        tokio::spawn(run_collector(tx, child, 100));

        let mut got_cpu = false;
        for _ in 0..20 {
            if let Ok(action) = tokio::time::timeout(
                std::time::Duration::from_millis(500), rx.recv()
            ).await {
                if matches!(action, Some(crate::action::Action::CpuUpdate(_))) {
                    got_cpu = true;
                    break;
                }
            }
        }
        token.cancel();
        assert!(got_cpu, "expected CpuUpdate from collector");
    }
}
```

- [ ] **Step 2: Implement `src/stats/mod.rs`**

```rust
pub mod snapshots;
pub use snapshots::*;

use crate::action::Action;
use sysinfo::{Networks, System, Disks};
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

pub fn spawn_collector(tx: Sender<Action>, token: CancellationToken, refresh_ms: u64) {
    tokio::spawn(run_collector(tx, token, refresh_ms));
}

pub async fn run_collector(tx: Sender<Action>, token: CancellationToken, refresh_ms: u64) {
    let mut sys     = System::new_all();
    let mut nets    = Networks::new_with_refreshed_list();
    let mut disks   = Disks::new_with_refreshed_list();
    let mut interval = tokio::time::interval(
        std::time::Duration::from_millis(refresh_ms)
    );

    loop {
        tokio::select! {
            _ = token.cancelled() => break,
            _ = interval.tick() => {}
        }

        sys.refresh_all();
        nets.refresh();
        disks.refresh();

        let actions = [
            Action::SysUpdate(build_sys(&sys)),
            Action::CpuUpdate(build_cpu(&sys)),
            Action::MemUpdate(build_mem(&sys)),
            Action::NetUpdate(build_net(&nets)),
            Action::DiskUpdate(build_disk(&disks)),
            Action::ProcUpdate(build_proc(&sys)),
        ];

        for action in actions {
            match tx.try_send(action) {
                Ok(_) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                    debug!("stats collector: channel full, dropping update");
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    warn!("stats collector: channel closed, stopping");
                    return;
                }
            }
        }
    }
}

fn build_sys(sys: &System) -> SysSnapshot {
    SysSnapshot {
        hostname:  System::host_name().unwrap_or_default(),
        uptime:    System::uptime(),
        load_avg:  {
            let la = System::load_average();
            [la.one, la.five, la.fifteen]
        },
        timestamp: chrono::Local::now(),
    }
}

fn build_cpu(sys: &System) -> CpuSnapshot {
    let cpus = sys.cpus();
    CpuSnapshot {
        per_core:  cpus.iter().map(|c| c.cpu_usage()).collect(),
        aggregate: sys.global_cpu_usage(),
        frequency: cpus.iter().map(|c| c.frequency()).collect(),
        #[cfg(target_os = "linux")]
        temperature: sys.components().iter()
            .find(|c| c.label().to_lowercase().contains("cpu"))
            .map(|c| c.temperature()),
    }
}

fn build_mem(sys: &System) -> MemSnapshot {
    MemSnapshot {
        ram_used:  sys.used_memory(),
        ram_total: sys.total_memory(),
        swap_used: sys.used_swap(),
        swap_total: sys.total_swap(),
        #[cfg(target_os = "linux")]
        swap_in_bytes:  read_vmstat_field("pswpin").unwrap_or(0) * 4096,
        #[cfg(target_os = "linux")]
        swap_out_bytes: read_vmstat_field("pswpout").unwrap_or(0) * 4096,
    }
}

/// Read a single numeric field from /proc/vmstat (Linux only).
/// pswpin/pswpout are cumulative page counts; multiply by PAGE_SIZE (4096) for bytes.
/// The collector snapshots the delta between ticks to get bytes/s.
#[cfg(target_os = "linux")]
fn read_vmstat_field(field: &str) -> Option<u64> {
    let content = std::fs::read_to_string("/proc/vmstat").ok()?;
    content.lines()
        .find(|l| l.starts_with(field))?
        .split_whitespace()
        .nth(1)?
        .parse().ok()
}

fn build_net(nets: &Networks) -> NetSnapshot {
    use crate::stats::snapshots::InterfaceSnapshot;
    NetSnapshot {
        interfaces: nets.iter().map(|(name, data)| InterfaceSnapshot {
            name:     name.clone(),
            rx_bytes: data.received(),
            tx_bytes: data.transmitted(),
        }).collect(),
    }
}

fn build_disk(disks: &Disks) -> DiskSnapshot {
    use crate::stats::snapshots::DiskDeviceSnapshot;
    DiskSnapshot {
        devices: disks.iter().map(|d| DiskDeviceSnapshot {
            name:        d.name().to_string_lossy().into_owned(),
            read_bytes:  d.read_bytes(),
            write_bytes: d.written_bytes(),
            usage_pct:   if d.total_space() > 0 {
                100.0 * (d.total_space() - d.available_space()) as f32
                    / d.total_space() as f32
            } else { 0.0 },
        }).collect(),
    }
}

fn build_proc(sys: &System) -> ProcSnapshot {
    use crate::stats::snapshots::ProcessEntry;
    ProcSnapshot {
        processes: sys.processes().values().map(|p| ProcessEntry {
            pid:         p.pid().as_u32(),
            name:        p.name().to_string_lossy().into_owned(),
            cmd:         p.cmd().iter().map(|s| s.to_string_lossy().into_owned()).collect(),
            user:        p.user_id().map(|u| u.to_string()).unwrap_or_default(),
            cpu_pct:     p.cpu_usage(),
            mem_bytes:   p.memory(),
            mem_pct:     if sys.total_memory() > 0 {
                100.0 * p.memory() as f32 / sys.total_memory() as f32
            } else { 0.0 },
            virt_bytes:  p.virtual_memory(),
            status:      format!("{:?}", p.status()),
            start_time:  p.start_time(),
            run_time:    p.run_time(),
            nice:        0, // sysinfo doesn't expose nice; read via /proc/<pid>/stat on Linux (v2)
            threads:     0, // sysinfo doesn't expose thread count directly (v2)
            read_bytes:  p.disk_usage().read_bytes,
            write_bytes: p.disk_usage().written_bytes,
            parent_pid:  p.parent().map(|pid| pid.as_u32()),
        }).collect(),
    }
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test stats
```

- [ ] **Step 4: Commit**

```bash
jj commit -m "feat: add stats collector tokio task with sysinfo"
```

---

## Phase 4: Simple Components

### Task 8: StatusBarComponent

**Files:**
- Create: `src/components/status_bar.rs` (replace stub)

- [ ] **Step 1: Write snapshot test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{action::Action, stats::snapshots::SysSnapshot, theme::Theme};
    use insta::assert_snapshot;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn renders_hostname_and_uptime() {
        let mut comp = StatusBarComponent::new(ColorPalette::dark());
        comp.update(Action::SysUpdate(SysSnapshot::stub())).unwrap();

        let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!(terminal.backend());
    }
}
```

- [ ] **Step 2: Run — expect failure**

```bash
cargo test status_bar
```

- [ ] **Step 3: Implement `src/components/status_bar.rs`**

```rust
use anyhow::Result;
use ratatui::{layout::Rect, style::{Style, Stylize}, text::{Line, Span}, Frame};
use crate::{action::Action, components::Component, stats::snapshots::SysSnapshot, theme::ColorPalette};

#[derive(Debug)]
pub struct StatusBarComponent {
    palette: ColorPalette,
    sys:     Option<SysSnapshot>,
}

impl StatusBarComponent {
    pub fn new(palette: ColorPalette) -> Self {
        Self { palette, sys: None }
    }
}

impl Component for StatusBarComponent {
    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        if let Action::SysUpdate(snap) = action {
            self.sys = Some(snap);
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let Some(sys) = &self.sys else { return Ok(()); };

        let uptime = format_uptime(sys.uptime);
        let load   = format!("{:.2} {:.2} {:.2}", sys.load_avg[0], sys.load_avg[1], sys.load_avg[2]);
        let time   = sys.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();

        let line = Line::from(vec![
            Span::styled(format!(" {} ", sys.hostname), Style::new().fg(self.palette.accent).bold()),
            Span::styled("| ", Style::new().fg(self.palette.border)),
            Span::styled(format!("up {} ", uptime), Style::new().fg(self.palette.fg)),
            Span::styled("| load: ", Style::new().fg(self.palette.dim)),
            Span::styled(format!("{} ", load), Style::new().fg(self.palette.fg)),
            Span::styled("| ", Style::new().fg(self.palette.border)),
            Span::styled(time, Style::new().fg(self.palette.dim)),
        ]);
        frame.render_widget(line, area);
        Ok(())
    }
}

fn format_uptime(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    if d > 0 { format!("{}d {}h {}m", d, h, m) }
    else if h > 0 { format!("{}h {}m", h, m) }
    else { format!("{}m", m) }
}
```

- [ ] **Step 4: Run tests and approve snapshot**

```bash
cargo test status_bar
cargo insta review     # approve the generated snapshot
cargo test status_bar  # verify it now passes
```

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat: implement StatusBarComponent with snapshot test"
```

---

### Task 9: CpuComponent

**Files:**
- Create: `src/components/cpu.rs` (replace stub)

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{action::Action, stats::snapshots::CpuSnapshot};
    use insta::assert_snapshot;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn renders_without_data() {
        let mut comp = CpuComponent::default();
        let mut terminal = Terminal::new(TestBackend::new(60, 10)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("cpu_no_data", terminal.backend());
    }

    #[test]
    fn renders_with_cpu_data() {
        let mut comp = CpuComponent::default();
        comp.update(Action::CpuUpdate(CpuSnapshot::stub())).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(60, 10)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("cpu_with_data", terminal.backend());
    }

    #[test]
    fn history_ring_buffer_bounded() {
        let mut comp = CpuComponent::default();
        for _ in 0..200 {
            comp.update(Action::CpuUpdate(CpuSnapshot::stub())).unwrap();
        }
        assert!(comp.history.len() <= HISTORY_LEN);
    }
}
```

- [ ] **Step 2: Implement `src/components/cpu.rs`**

```rust
use std::collections::VecDeque;
use anyhow::Result;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    symbols,
    text::{Line, Span},
    widgets::{Block, Borders, Sparkline, Gauge, Paragraph},
    Frame,
};
use crate::{action::Action, components::Component, stats::snapshots::CpuSnapshot,
            theme::ColorPalette};

pub const HISTORY_LEN: usize = 100;

#[derive(Debug)]
pub struct CpuComponent {
    palette:  ColorPalette,
    latest:   Option<CpuSnapshot>,
    pub history: VecDeque<u64>, // aggregate history for sparkline, 0–100
}

impl Default for CpuComponent {
    fn default() -> Self {
        Self { palette: ColorPalette::dark(), latest: None, history: VecDeque::new() }
    }
}

impl CpuComponent {
    pub fn new(palette: ColorPalette) -> Self {
        Self { palette, ..Default::default() }
    }
}

impl Component for CpuComponent {
    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        if let Action::CpuUpdate(snap) = action {
            if self.history.len() >= HISTORY_LEN { self.history.pop_front(); }
            self.history.push_back(snap.aggregate as u64);
            self.latest = Some(snap);
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let block = Block::default()
            .title(" CPU ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(self.palette.border));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let Some(snap) = &self.latest else { return Ok(()); };

        // Split: top half = sparkline, bottom = per-core bars
        let rows = Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]).split(inner);

        // Aggregate sparkline
        let data: Vec<u64> = self.history.iter().copied().collect();
        let sparkline = Sparkline::default()
            .data(&data)
            .style(Style::new().fg(self.palette.accent))
            .max(100);
        frame.render_widget(sparkline, rows[0]);

        // Per-core gauges
        let n = snap.per_core.len().min(8); // max 8 cores rendered
        if n == 0 { return Ok(()); }
        let constraints: Vec<Constraint> = (0..n).map(|_| Constraint::Length(1)).collect();
        let core_rows = Layout::vertical(constraints).split(rows[1]);
        for (i, (pct, rect)) in snap.per_core.iter().zip(core_rows.iter()).enumerate() {
            let gauge = Gauge::default()
                .ratio((*pct as f64 / 100.0).clamp(0.0, 1.0))
                .label(format!("c{:<2} {:>5.1}%", i, pct))
                .gauge_style(Style::new().fg(self.palette.accent));
            frame.render_widget(gauge, *rect);
        }
        Ok(())
    }
}
```

- [ ] **Step 3: Run tests and approve snapshots**

```bash
cargo test cpu
cargo insta review
cargo test cpu
```

- [ ] **Step 4: Commit**

```bash
jj commit -m "feat: implement CpuComponent with sparkline and per-core gauges"
```

---

### Task 10: MemComponent, NetComponent, DiskComponent

**Files:**
- Replace stubs: `src/components/mem.rs`, `src/components/net.rs`, `src/components/disk.rs`

Follow the same TDD pattern as Task 9 for each. Key implementation notes:

**MemComponent:**
- Two gauge bars: RAM and Swap
- On Linux, show swap-in/swap-out rates if non-zero
- Receives `Action::MemUpdate`

**NetComponent:**
- Scrollable list of interfaces using `ratatui::widgets::List`
- Each row: `iface  ▲ tx_rate  ▼ rx_rate` with rate formatted as human-readable (KB/s, MB/s)
- Scroll state: `ratatui::widgets::ListState`
- Receives `Action::NetUpdate`; `↑`/`↓` keys scroll

**DiskComponent:**
- Similar scrollable list pattern to NetComponent
- Each row: `device  read_rate  write_rate  usage%`
- Receives `Action::DiskUpdate`

For each:
- [ ] Write snapshot test
- [ ] Run — verify fail
- [ ] Implement
- [ ] Run — verify pass + approve snapshot
- [ ] Commit with `feat: implement <Name>Component`

---

## Phase 5: Process Component

### Task 11: Process sort and filter logic

**Files:**
- Create: `src/components/process/sort.rs`
- Create: `src/components/process/filter.rs`

- [ ] **Step 1: Write sort tests**

```rust
// src/components/process/sort.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::snapshots::ProcSnapshot;

    #[test]
    fn sort_by_cpu_desc_puts_highest_first() {
        let mut procs = ProcSnapshot::stub().processes;
        procs.push(crate::stats::snapshots::ProcessEntry {
            pid: 1, name: "low".into(), cpu_pct: 1.0,
            // fill remaining fields with defaults
            ..procs[0].clone()
        });
        sort_processes(&mut procs, SortColumn::Cpu, SortDir::Desc);
        assert!(procs[0].cpu_pct >= procs[1].cpu_pct);
    }

    #[test]
    fn sort_by_name_asc_is_alphabetical() {
        let mut procs = ProcSnapshot::stub().processes;
        procs.push(crate::stats::snapshots::ProcessEntry {
            name: "aardvark".into(), ..procs[0].clone()
        });
        sort_processes(&mut procs, SortColumn::Name, SortDir::Asc);
        assert!(procs[0].name <= procs[1].name);
    }
}
```

- [ ] **Step 2: Implement `src/components/process/sort.rs`**

```rust
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString};
use crate::stats::snapshots::ProcessEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Display, EnumIter, EnumString,
         Serialize, Deserialize)]
#[strum(serialize_all = "lowercase")]
pub enum SortColumn { #[default] Cpu, Mem, Pid, Name }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortDir { #[default] Desc, Asc }

pub fn sort_processes(procs: &mut [ProcessEntry], col: SortColumn, dir: SortDir) {
    procs.sort_by(|a, b| {
        let ord = match col {
            SortColumn::Cpu  => a.cpu_pct.partial_cmp(&b.cpu_pct).unwrap_or(std::cmp::Ordering::Equal),
            SortColumn::Mem  => a.mem_bytes.cmp(&b.mem_bytes),
            SortColumn::Pid  => a.pid.cmp(&b.pid),
            SortColumn::Name => a.name.cmp(&b.name),
        };
        if dir == SortDir::Desc { ord.reverse() } else { ord }
    });
}
```

- [ ] **Step 3: Write filter tests**

```rust
// src/components/process/filter.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::snapshots::ProcSnapshot;

    #[test]
    fn filter_by_name_substring() {
        let procs = ProcSnapshot::stub().processes;
        let f = ProcessFilter::Name("fire".into());
        let result: Vec<_> = procs.iter().filter(|p| f.matches(p)).collect();
        assert!(result.iter().all(|p| p.name.to_lowercase().contains("fire")));
    }

    #[test]
    fn filter_by_pid_exact() {
        let procs = ProcSnapshot::stub().processes;
        let f = ProcessFilter::Pid(12345);
        let result: Vec<_> = procs.iter().filter(|p| f.matches(p)).collect();
        assert!(result.iter().all(|p| p.pid == 12345));
    }

    #[test]
    fn filter_by_state() {
        let procs = ProcSnapshot::stub().processes;
        let f = ProcessFilter::State("running".into());
        let result: Vec<_> = procs.iter().filter(|p| f.matches(p)).collect();
        assert!(result.iter().all(|p| p.status.to_lowercase().contains("running")));
    }
}
```

- [ ] **Step 4: Implement `src/components/process/filter.rs`**

```rust
use crate::stats::snapshots::ProcessEntry;

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessFilter {
    None,
    Name(String),
    Pid(u32),
    State(String),
}

impl ProcessFilter {
    pub fn matches(&self, p: &ProcessEntry) -> bool {
        match self {
            Self::None         => true,
            Self::Name(s)      => p.name.to_lowercase().contains(&s.to_lowercase()),
            Self::Pid(pid)     => p.pid == *pid,
            Self::State(s)     => p.status.to_lowercase().contains(&s.to_lowercase()),
        }
    }

    /// Parse a raw filter string. "/" followed by a number → Pid, "/s:..." → State, else Name.
    pub fn parse(input: &str) -> Self {
        let s = input.trim();
        if s.is_empty() { return Self::None; }
        if let Ok(pid) = s.parse::<u32>() { return Self::Pid(pid); }
        if let Some(rest) = s.strip_prefix("s:") { return Self::State(rest.into()); }
        Self::Name(s.into())
    }
}
```

- [ ] **Step 5: Run all process tests**

```bash
cargo test process
```

- [ ] **Step 6: Commit**

```bash
jj commit -m "feat: add process sort and filter logic"
```

---

### Task 12: ProcessComponent — list view

**Files:**
- Create: `src/components/process/mod.rs` (replace stub)

- [ ] **Step 1: Write snapshot test**

```rust
#[test]
fn renders_process_list() {
    let mut comp = ProcessComponent::default();
    comp.update(Action::ProcUpdate(ProcSnapshot::stub())).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
    assert_snapshot!(terminal.backend());
}
```

- [ ] **Step 2: Implement the list view in `src/components/process/mod.rs`**

Key implementation points:
- Hold `Vec<ProcessEntry>` (filtered + sorted), `ListState`, `ProcessFilter`, `SortColumn`, `SortDir`
- On `Action::ProcUpdate`, apply filter then sort, update display list
- `draw()` renders a `Table` widget (ratatui) with columns: PID | Name | CPU% | MEM% | Status
- Highlight selected row with `palette.highlight` style
- Header row shows sort column with ▲/▼ indicator
- `↑`/`↓` key events update `ListState::select`

- [ ] **Step 3: Run tests and approve snapshot**

```bash
cargo test process
cargo insta review
```

- [ ] **Step 4: Commit**

```bash
jj commit -m "feat: implement ProcessComponent list view"
```

---

### Task 13: ProcessComponent — filter mode, detail view, kill

**Files:**
- Modify: `src/components/process/mod.rs`

- [ ] **Step 1: Write tests for each state transition**

```rust
#[test]
fn slash_key_enters_filter_mode() {
    let mut comp = ProcessComponent::default();
    comp.handle_key_event(key('/'));
    assert_eq!(comp.state, ProcessState::FilterMode { input: String::new() });
}

#[test]
fn esc_in_filter_mode_returns_to_list() {
    let mut comp = ProcessComponent::default();
    comp.handle_key_event(key('/'));
    comp.handle_key_event(key_code(KeyCode::Esc));
    assert_eq!(comp.state, ProcessState::NormalList);
}

#[test]
fn enter_opens_detail_view() {
    let mut comp = ProcessComponent::default();
    comp.update(Action::ProcUpdate(ProcSnapshot::stub())).unwrap();
    comp.handle_key_event(key_code(KeyCode::Enter));
    assert!(matches!(comp.state, ProcessState::DetailView { .. }));
}
```

Helper in test module:
```rust
fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}
fn key_code(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}
```

- [ ] **Step 2: Add `ProcessState` enum and state machine transitions**

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessState {
    NormalList,
    FilterMode { input: String },
    DetailView { pid: u32 },
    KillConfirm { pid: u32, name: String },
}
```

- [ ] **Step 3: Implement `handle_key_event` with state-aware dispatch**

Key dispatch rules (per spec):
- `ProcessState::FilterMode`: `Esc` → clear filter, return `NormalList`; printable chars → append to input, re-filter; `Backspace` → pop char
- `ProcessState::NormalList`: `↑`/`↓` scroll; `Enter` → `DetailView`; `/` → `FilterMode`; `k` → `KillConfirm`; `s` → cycle sort; `S` → reverse sort; `t` → toggle tree
- `ProcessState::DetailView`: `Esc`/`q` → `NormalList`
- `ProcessState::KillConfirm`: `y`/`Enter` → send SIGTERM + `NormalList`; `n`/`Esc` → `NormalList`

- [ ] **Step 4: Implement detail view rendering**

Detail view overlays the full component area with: command line, user, nice, uptime, thread count, open files, memory breakdown (virt/res), I/O bytes.

- [ ] **Step 5: Implement SIGTERM kill**

```rust
fn kill_process(pid: u32) -> Result<()> {
    use std::process::Command;
    Command::new("kill").arg("-TERM").arg(pid.to_string()).status()
        .context("sending SIGTERM")?;
    Ok(())
}
```

- [ ] **Step 6: Run all process tests**

```bash
cargo test process
cargo insta review
```

- [ ] **Step 7: Commit**

```bash
jj commit -m "feat: add ProcessComponent filter mode, detail view, and kill"
```

---

## Phase 6: App Event Loop and Wiring

### Task 14: App struct and event loop

**Files:**
- Create: `src/app.rs` (replace stub)

- [ ] **Step 1: Implement `src/app.rs`**

```rust
use anyhow::{Context, Result};
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::{
    action::Action,
    components::{Component, ComponentId},
    config::Config,
    layout::{split_status_bar, LayoutPreset, SlotMap, SlotOverrides, StatusBarPosition},
    stats::spawn_collector,
    tui::{Event, Tui},
};
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq)]
pub enum FocusState {
    Normal { focused: ComponentId },
    FullScreen(ComponentId),
}

pub struct App {
    config:         Config,
    action_tx:      mpsc::Sender<Action>,
    action_rx:      mpsc::Receiver<Action>,
    components:     Vec<(ComponentId, Box<dyn Component>)>,
    status_bar:     Box<dyn Component>,
    debug_comp:     Box<dyn Component>,
    focus:          FocusState,
    show_debug:     bool,
    should_quit:    bool,
    should_suspend: bool,
    preset:         LayoutPreset,
    slot_overrides: SlotOverrides,
    status_pos:     StatusBarPosition,
    visible:        std::collections::HashSet<ComponentId>,
}

impl App {
    pub fn new(config: Config, show_debug: bool) -> Result<Self> {
        let (action_tx, action_rx) = mpsc::channel(config.general.channel_capacity);

        let palette = config.general.theme.palette();
        // Build component list
        let components: Vec<(ComponentId, Box<dyn Component>)> = vec![
            (ComponentId::Cpu,  Box::new(crate::components::cpu::CpuComponent::new(palette.clone()))),
            (ComponentId::Mem,  Box::new(crate::components::mem::MemComponent::new(palette.clone()))),
            (ComponentId::Net,  Box::new(crate::components::net::NetComponent::new(palette.clone()))),
            (ComponentId::Disk, Box::new(crate::components::disk::DiskComponent::new(palette.clone()))),
            (ComponentId::Proc, Box::new(crate::components::process::ProcessComponent::new(palette.clone(), &config.process))),
        ];

        let visible = config.layout.show.iter()
            .filter_map(|s| ComponentId::from_str(s).ok())
            .collect();

        let preset = LayoutPreset::from_str(&config.layout.preset).unwrap_or_default();
        let status_pos = match config.layout.status_bar.as_str() {
            "bottom" => StatusBarPosition::Bottom,
            "hidden" => StatusBarPosition::Hidden,
            _ => StatusBarPosition::Top,
        };

        Ok(Self {
            action_tx,
            action_rx,
            status_bar: Box::new(crate::components::status_bar::StatusBarComponent::new(palette.clone())),
            debug_comp: Box::new(crate::components::debug::DebugComponent::new(palette.clone())),
            focus: FocusState::Normal { focused: ComponentId::Proc },
            show_debug,
            should_quit: false,
            should_suspend: false,
            preset,
            slot_overrides: SlotOverrides::default(),
            status_pos,
            visible,
            config,
            components,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut tui = Tui::new()?.mouse(true);
        tui.enter().context("entering TUI")?;

        // Register action handler with all components
        for (_, comp) in &mut self.components {
            comp.register_action_handler(self.action_tx.clone())
                .context("registering action handler")?;
            comp.register_config_handler(self.config.clone())
                .context("registering config handler")?;
        }

        // Init all components
        let size = tui.size().context("getting terminal size")?;
        for (_, comp) in &mut self.components {
            comp.init(size).context("initializing component")?;
        }

        // Start stats collector
        let collector_token = tokio_util::sync::CancellationToken::new();
        spawn_collector(
            self.action_tx.clone(),
            collector_token.child_token(),
            self.config.general.refresh_rate_ms,
        );

        loop {
            self.handle_events(&mut tui).await.context("handling events")?;
            self.handle_actions(&mut tui).context("handling actions")?;

            if self.should_suspend {
                tui.suspend_and_resume(&self.action_tx).await?;
            } else if self.should_quit {
                collector_token.cancel();
                tui.exit().context("exiting TUI")?;
                break;
            }
        }
        Ok(())
    }

    async fn handle_events(&mut self, tui: &mut Tui) -> Result<()> {
        let Some(event) = tui.next_event().await else { return Ok(()); };
        let tx = &self.action_tx;
        match &event {
            Event::Quit         => { let _ = tx.try_send(Action::Quit); }
            Event::Tick         => { let _ = tx.try_send(Action::Tick); }
            Event::Render       => { let _ = tx.try_send(Action::Render); }
            Event::Resize(x, y) => { let _ = tx.try_send(Action::Resize(*x, *y)); }
            Event::Key(key) => {
                // Give the focused component first right of refusal on key events.
                // If it returns Some(action), it consumed the key — do not run global handler.
                // This is critical for Esc/q which mean different things inside vs outside
                // component sub-states (e.g., q in DetailView ≠ quit).
                let focused_id = match &self.focus {
                    FocusState::Normal { focused } | FocusState::FullScreen(focused) => *focused,
                };
                let consumed = self.components.iter_mut()
                    .find(|(id, _)| *id == focused_id)
                    .and_then(|(_, comp)| comp.handle_key_event(*key).ok().flatten());

                if let Some(action) = consumed {
                    let _ = self.action_tx.try_send(action);
                } else {
                    // Component did not consume — run global handler
                    self.handle_key_event(*key)?;
                }
            }
            _ => {}
        }
        // Fan out non-key events to all components
        if !matches!(event, Event::Key(_)) {
            for (_, comp) in &mut self.components {
                if let Some(action) = comp.handle_events(Some(event.clone()))? {
                    let _ = self.action_tx.try_send(action);
                }
            }
        }
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<()> {
        use crossterm::event::KeyCode;
        let kb = &self.config.keybindings;
        let ch = match key.code {
            KeyCode::Char(c) => Some(c.to_ascii_lowercase()),
            _ => None,
        };
        if let Some(c) = ch {
            if      c == kb.focus_proc { let _ = self.action_tx.try_send(Action::FocusComponent(ComponentId::Proc)); }
            else if c == kb.focus_cpu  { let _ = self.action_tx.try_send(Action::FocusComponent(ComponentId::Cpu)); }
            else if c == kb.focus_mem  { let _ = self.action_tx.try_send(Action::FocusComponent(ComponentId::Mem)); }
            else if c == kb.focus_net  { let _ = self.action_tx.try_send(Action::FocusComponent(ComponentId::Net)); }
            else if c == kb.focus_disk { let _ = self.action_tx.try_send(Action::FocusComponent(ComponentId::Disk)); }
            else if c == kb.fullscreen { let _ = self.action_tx.try_send(Action::ToggleFullScreen); }
            else if c == kb.debug      { let _ = self.action_tx.try_send(Action::ToggleDebug); }
            else if c == 'q'           { let _ = self.action_tx.try_send(Action::Quit); }
        }
        if key.code == KeyCode::Esc {
            match &self.focus {
                FocusState::FullScreen(_) => {
                    let _ = self.action_tx.try_send(Action::ToggleFullScreen);
                }
                FocusState::Normal { .. } => {
                    let _ = self.action_tx.try_send(Action::Quit);
                }
            }
        }
        Ok(())
    }

    fn handle_actions(&mut self, tui: &mut Tui) -> Result<()> {
        while let Ok(action) = self.action_rx.try_recv() {
            if !matches!(action, Action::Tick | Action::Render) {
                debug!("action: {action}");
            }
            match &action {
                Action::Tick         => {}
                Action::Quit         => self.should_quit = true,
                Action::Suspend      => self.should_suspend = true,
                Action::Resume       => self.should_suspend = false,
                Action::ClearScreen  => { tui.terminal.clear().context("clearing screen")?; }
                Action::ToggleDebug  => self.show_debug = !self.show_debug,
                Action::ToggleFullScreen => {
                    self.focus = match &self.focus {
                        FocusState::Normal { focused } => FocusState::FullScreen(*focused),
                        FocusState::FullScreen(id)    => FocusState::Normal { focused: *id },
                    };
                }
                Action::FocusComponent(id) => {
                    self.focus = FocusState::Normal { focused: *id };
                }
                Action::Resize(w, h) => {
                    tui.resize(Rect::new(0, 0, *w, *h)).context("resizing")?;
                    self.render(tui).context("re-rendering after resize")?;
                }
                Action::Render => self.render(tui).context("rendering")?,
                _ => {}
            }
            // Fan out to all components
            for (_, comp) in &mut self.components {
                if let Some(new_action) = comp.update(action.clone())? {
                    let _ = self.action_tx.try_send(new_action);
                }
            }
            self.status_bar.update(action.clone())?;
        }
        Ok(())
    }

    fn render(&mut self, tui: &mut Tui) -> Result<()> {
        let focus   = self.focus.clone();
        let preset  = self.preset;
        let visible = self.visible.clone();
        let show_debug = self.show_debug;
        let status_pos = self.status_pos;
        let slot_overrides = self.slot_overrides.clone();

        tui.draw(|frame| {
            let total_area = frame.area();

            // Status bar strip
            let (status_rect, content_area) = split_status_bar(total_area, status_pos);
            if status_pos != StatusBarPosition::Hidden {
                let _ = self.status_bar.draw(frame, status_rect);
            }

            // Debug sidebar
            let (main_area, debug_area) = if show_debug {
                let cols = ratatui::layout::Layout::horizontal([
                    ratatui::layout::Constraint::Fill(1),
                    ratatui::layout::Constraint::Length(40),
                ]).split(content_area);
                (cols[0], Some(cols[1]))
            } else {
                (content_area, None)
            };

            if let Some(da) = debug_area {
                let _ = self.debug_comp.draw(frame, da);
            }

            // Full-screen or normal layout
            match &focus {
                FocusState::FullScreen(id) => {
                    if let Some((_, comp)) = self.components.iter_mut().find(|(cid, _)| cid == id) {
                        let _ = comp.draw(frame, main_area);
                    }
                }
                FocusState::Normal { .. } => {
                    let slot_map = preset.compute(main_area, &slot_overrides);
                    for (_, (component_id, rect)) in &slot_map {
                        if !visible.contains(component_id) { continue; }
                        if let Some((_, comp)) = self.components.iter_mut()
                            .find(|(cid, _)| cid == component_id)
                        {
                            let _ = comp.draw(frame, *rect);
                        }
                    }
                }
            }
        })?;
        Ok(())
    }
}
```

- [ ] **Step 2: Verify build**

```bash
cargo build
```

- [ ] **Step 3: Commit**

```bash
jj commit -m "feat: implement App event loop with full-screen focus and layout"
```

---

### Task 15: DebugComponent

**Files:**
- Create: `src/components/debug.rs` (replace stub)

- [ ] **Step 1: Implement**

Per the spec, `DebugComponent` renders the `{:#?}` formatted state of each component actor — not just an action log. `App` passes snapshot strings into it via `Action::DebugSnapshot`. Add this action variant to `action.rs`:

```rust
// In action.rs, add to Action enum:
DebugSnapshot(String),   // formatted {:#?} state from all components
```

Then in `App::handle_actions`, after fanning out each action to components, collect their debug state:

```rust
// After fanning out in handle_actions:
if self.show_debug {
    let snapshot = self.components.iter()
        .map(|(id, comp)| format!("[{id}]\n{comp:#?}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    let _ = self.action_tx.try_send(Action::DebugSnapshot(snapshot));
}
```

This requires each component to derive or implement `Debug` (already required by `#[derive(Debug)]` on their structs).

```rust
use anyhow::Result;
use ratatui::{
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use crate::{action::Action, components::Component, theme::ColorPalette};

#[derive(Debug, Default)]
pub struct DebugComponent {
    palette:  ColorPalette,
    snapshot: String,
}

impl DebugComponent {
    pub fn new(palette: ColorPalette) -> Self {
        Self { palette, snapshot: String::new() }
    }
}

impl Component for DebugComponent {
    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        if let Action::DebugSnapshot(s) = action {
            self.snapshot = s;
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let block = Block::default()
            .title(" DEBUG ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(self.palette.warn));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let para = Paragraph::new(self.snapshot.as_str())
            .style(Style::new().fg(self.palette.dim))
            .wrap(Wrap { trim: true });
        frame.render_widget(para, inner);
        Ok(())
    }
}
```

- [ ] **Step 2: Commit**

```bash
jj commit -m "feat: implement DebugComponent action log sidebar"
```

---

## Phase 7: Main Entry and Polish

### Task 16: Wire main.rs, tracing, and --init-config

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Implement full `src/main.rs`**

```rust
use anyhow::Context;
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod action;
mod app;
mod cli;
mod components;
mod config;
mod errors;
mod layout;
mod stats;
mod theme;
mod tui;

fn main() -> anyhow::Result<()> {
    let args = cli::Args::parse();

    // Set up tracing to file
    let log_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("toppers");
    std::fs::create_dir_all(&log_dir).context("creating log dir")?;
    let log_file = std::fs::File::create(log_dir.join("toppers.log"))
        .context("creating log file")?;
    let level = match args.verbose {
        0 => tracing::Level::WARN,
        1 => tracing::Level::INFO,
        _ => tracing::Level::DEBUG,
    };
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(log_file))
        .with(tracing_subscriber::filter::LevelFilter::from_level(level))
        .init();

    if args.init_config {
        print!("{}", DEFAULT_CONFIG_TEMPLATE);
        return Ok(());
    }

    // Load and merge config
    let mut cfg = config::Config::load(args.config.as_deref())
        .context("loading config")?;
    apply_cli_overrides(&mut cfg, &args);

    // Run async
    let rt = tokio::runtime::Runtime::new().context("creating tokio runtime")?;
    rt.block_on(async {
        let mut app = app::App::new(cfg, args.debug).context("creating App")?;
        app.run().await.context("running App")
    })
}

fn apply_cli_overrides(cfg: &mut config::Config, args: &cli::Args) {
    if let Some(t) = &args.theme {
        if let Ok(theme) = t.parse() { cfg.general.theme = theme; }
    }
    if let Some(r) = &args.refresh_rate {
        if let Ok(d) = humantime::parse_duration(r) {
            cfg.general.refresh_rate_ms = d.as_millis() as u64;
        }
    }
    if let Some(p) = &args.preset {
        cfg.layout.preset = p.clone();
    }
    if let Some(pos) = &args.status_bar {
        cfg.layout.status_bar = pos.clone();
    }
    // --hide wins over --show
    if let Some(show) = &args.show { cfg.layout.show = show.clone(); }
    if let Some(hide) = &args.hide {
        cfg.layout.show.retain(|c| !hide.contains(c));
    }
}

const DEFAULT_CONFIG_TEMPLATE: &str = r#"# toppers default configuration
# Generated by: toppers --init-config
# All options are commented out — uncomment and edit as needed.

[general]
# Refresh interval for all components. Examples: "500ms", "1s", "2s"
# refresh_rate = "1s"

# Color theme. "auto" detects light/dark from terminal background.
# theme = "auto"   # "auto" | "light" | "dark"

# Bounded action channel capacity.
# channel_capacity = 128

[layout]
# Base layout preset.
# preset = "sidebar"   # "sidebar" | "classic" | "dashboard"

# Status bar position.
# status_bar = "top"   # "top" | "bottom" | "hidden"

# Which components to show (omit to hide).
# show = ["cpu", "mem", "net", "disk", "proc"]

# Slot overrides — replace default component in a named slot.
# Sidebar slots: left_top, left_mid, left_bot, right
# Classic slots: top_left, top_right_top, top_right_bot, bottom
# Dashboard slots: top, mid_left, mid_right, bottom
# left_top = "cpu"

[process]
# Default sort column: "cpu" | "mem" | "pid" | "name"
# default_sort = "cpu"
# default_sort_dir = "desc"   # "asc" | "desc"
# show_tree = false

[keybindings]
# focus_proc = "p"
# focus_cpu  = "c"
# focus_mem  = "m"
# focus_net  = "n"
# focus_disk = "d"
# fullscreen = "f"
# help       = "?"
# debug      = "`"
"#;
```

- [ ] **Step 2: Build and run**

```bash
cargo build
./target/debug/toppers --init-config  # should print template
./target/debug/toppers --version      # should print version
```

- [ ] **Step 3: Commit**

```bash
jj commit -m "feat: wire main.rs with tracing, CLI overrides, and --init-config"
```

---

### Task 17: End-to-end smoke test and snapshot coverage

**Files:**
- Create: `tests/snapshots.rs`

- [ ] **Step 1: Write integration smoke test**

```rust
// tests/snapshots.rs
use toppers::{
    action::Action,
    components::{cpu::CpuComponent, mem::MemComponent, Component},
    stats::snapshots::{CpuSnapshot, MemSnapshot},
    theme::ColorPalette,
};
use insta::assert_snapshot;
use ratatui::{backend::TestBackend, Terminal};

#[test]
fn cpu_component_snapshot() {
    let mut comp = CpuComponent::new(ColorPalette::dark());
    comp.update(Action::CpuUpdate(CpuSnapshot::stub())).unwrap();
    let mut t = Terminal::new(TestBackend::new(80, 20)).unwrap();
    t.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
    assert_snapshot!(t.backend());
}

#[test]
fn mem_component_snapshot() {
    let mut comp = MemComponent::new(ColorPalette::dark());
    comp.update(Action::MemUpdate(MemSnapshot::stub())).unwrap();
    let mut t = Terminal::new(TestBackend::new(60, 10)).unwrap();
    t.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
    assert_snapshot!(t.backend());
}
```

This requires `lib.rs` to re-export the necessary modules. Add `src/lib.rs`:

```rust
pub mod action;
pub mod components;
pub mod config;
pub mod layout;
pub mod stats;
pub mod theme;
pub mod tui;
// (omit app, cli, errors — keep those main.rs-only)
```

- [ ] **Step 2: Run all tests**

```bash
cargo test
cargo insta review   # approve all new snapshots
cargo test           # verify all pass
```

- [ ] **Step 3: Check coverage**

```bash
cargo llvm-cov --summary-only
```

Target: ≥ 80% line coverage. Add tests for any under-covered paths.

- [ ] **Step 4: Final lint check**

```bash
cargo fmt --check
cargo clippy -- -D warnings
```

- [ ] **Step 5: Final commit**

```bash
jj commit -m "test: add integration snapshot tests and verify coverage"
```

---

## Checklist Before Calling Done

- [ ] `cargo build` — clean
- [ ] `cargo test` — all pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — clean
- [ ] `cargo llvm-cov --summary-only` — ≥ 80% lines
- [ ] `./target/debug/toppers --init-config` — prints template
- [ ] `./target/debug/toppers` — TUI launches, all 5 components visible in sidebar layout
- [ ] `p` key — focuses processes; `↑`/`↓` scroll; `/` opens filter; `Enter` opens detail
- [ ] `f` key — full-screen; `Esc` — returns to layout
- [ ] `` ` `` key — debug sidebar appears; `` ` `` again — hides it
- [ ] `q` — quits cleanly, terminal restored
