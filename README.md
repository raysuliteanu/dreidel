# toppers

**A fast, keyboard-driven terminal system monitor.**

![License: GPL-3.0-only](https://img.shields.io/badge/license-GPL--3.0--only-blue)
![Language: Rust](https://img.shields.io/badge/language-Rust-orange)

---

## Screenshots

<!-- screenshot: sidebar layout -->
<!-- screenshot: dashboard layout -->
<!-- screenshot: process fullscreen -->

_Screenshots coming soon._

---

## Overview

toppers is a terminal UI system monitor that gives you a live view of your machine at a glance:

- **CPU** — aggregate usage sparkline and per-core bars
- **Memory** — RAM and swap usage with history
- **Network** — per-interface RX/TX rates with optional graph view
- **Disk** — per-device read/write rates and usage percentage
- **Process** — sortable, filterable process table with kill support

Everything is navigable by keyboard, customisable via a config file or CLI flags, and designed
to stay out of your way.

Developers and contributors interested in how toppers is built can refer to
[ARCHITECTURE.md](ARCHITECTURE.md) for a technical deep-dive.

---

## Installation

### cargo install

<!-- TODO: publish to crates.io, then enable this section -->
<!--
```sh
cargo install toppers
```
-->

_`cargo install` support coming once the crate is published to crates.io._

### cargo binstall

<!-- TODO: set up cargo-binstall metadata (package.metadata.binstall in Cargo.toml) and publish releases with pre-built binaries -->
<!--
```sh
cargo binstall toppers
```
-->

_`cargo binstall` support coming once pre-built release binaries are available._

### From source

Requires a recent stable [Rust toolchain](https://rustup.rs).

```sh
git clone https://github.com/raysuliteanu/toppers
cd toppers
cargo build --release
./target/release/toppers
```

---

## Quick Start

```sh
toppers               # launch with defaults
toppers --help        # see all options
toppers --init-config # print a commented config template to stdout
```

---

## Layouts

Select a layout with `--preset <NAME>` or set `layout.preset` in your config file.

| Preset                | Description                                                                               |
| --------------------- | ----------------------------------------------------------------------------------------- |
| `sidebar` _(default)_ | Narrow left panel (CPU, Net, Disk) beside a tall process list                             |
| `classic`             | CPU top-left, Disk/Net stacked top-right, Process fills the bottom                        |
| `dashboard`           | CPU strip across the top, Disk + Net side-by-side in the middle, Process fills the bottom |
| `grid`                | Disk + Net stacked on the left, CPU + Process stacked on the right                        |

> **Tip:** In the `dashboard` layout (and whenever a component is in fullscreen mode) the process
> list is wide enough to automatically switch to the extended column view.

---

## Components

### CPU

Displays overall CPU usage as a scrolling sparkline (last 100 samples) alongside per-core
utilisation bars. Bars are colour-coded: green → orange (>80%) → red (>95%).

Focus key: `c`

### Network

Lists all network interfaces with live receive and transmit rates. Press `Enter` on a selected
interface to open a scrolling sparkline graph for that interface. Press `Esc` or `q` to close
the graph and return to the list.

Focus key: `n`

### Disk

Lists all storage devices with per-device read/write byte rates and disk usage percentage.
Usage colour coding follows the same green → orange → red scale as CPU.

Focus key: `i`

### Process

A live process table with two display modes that switch automatically based on terminal width:

| Mode     | Columns                                                                         | When                              |
| -------- | ------------------------------------------------------------------------------- | --------------------------------- |
| Normal   | PID · Name · CPU% · MEM · Status                                                | terminal < 120 cols               |
| Extended | PID · User · PR · NI · VIRT · RES · SHR · Status · %CPU · %MEM · TIME · Command | terminal ≥ 120 cols or fullscreen |

**Filter:** Press `/` to enter filter mode. Type any substring to narrow the list in real time.
Press `Esc` to clear the filter and return to the full list.

**Sort:** Press `s` to cycle through sort columns in their left-to-right display order.
Press `S` to toggle the sort direction (ascending / descending). The active sort column and
direction are shown in the column header.

**Kill:** Navigate to a process and press `k`. toppers will ask for confirmation (`y`/`n`)
before sending `SIGTERM`.

Focus key: `p`

### Status Bar

Displays a clock, hostname, and global system stats. Can be positioned at the top (default),
bottom, or hidden entirely via `--status-bar` or `layout.status_bar` in the config file.

---

## Keyboard Reference

### Global

| Key                 | Action                                                |
| ------------------- | ----------------------------------------------------- |
| `c`                 | Focus CPU panel                                       |
| `m`                 | Focus Memory panel                                    |
| `n`                 | Focus Network panel                                   |
| `i`                 | Focus Disk panel                                      |
| `p`                 | Focus Process panel                                   |
| `Tab` / `Shift+Tab` | Cycle focus forward / backward through visible panels |
| `f`                 | Toggle fullscreen for the focused panel               |
| `?`                 | Show help overlay                                     |
| `d`                 | Toggle debug sidebar                                  |
| `q` / `Esc`         | Quit (or exit fullscreen / close overlay)             |

### Process panel

| Key             | Action                                         |
| --------------- | ---------------------------------------------- |
| `↑` / `↓`       | Move row selection                             |
| `PgUp` / `PgDn` | Scroll 10 rows at a time                       |
| `s`             | Cycle sort column (left-to-right order)        |
| `S`             | Toggle sort direction (ascending / descending) |
| `/`             | Enter filter mode                              |
| `Esc`           | Exit filter / close detail view                |
| `Enter`         | Open detailed process info                     |
| `k`             | Kill selected process (prompts `y`/`n`)        |

### Network and Disk panels

| Key             | Action                              |
| --------------- | ----------------------------------- |
| `↑` / `↓`       | Move selection                      |
| `PgUp` / `PgDn` | Scroll 10 rows at a time            |
| `Enter`         | Open interface graph (Network only) |
| `Esc` / `q`     | Close graph view                    |

---

## CLI Reference

```
toppers [OPTIONS]
```

| Flag                    | Default                         | Description                                                            |
| ----------------------- | ------------------------------- | ---------------------------------------------------------------------- |
| `--theme <THEME>`       | `auto`                          | Color theme: `auto` \| `light` \| `dark`                               |
| `--refresh-rate <RATE>` | `1s`                            | Stats refresh interval, e.g. `500ms`, `2s`                             |
| `--preset <LAYOUT>`     | `sidebar`                       | Layout preset: `sidebar` \| `classic` \| `dashboard` \| `grid`         |
| `--show <COMPONENTS>`   | _(all)_                         | Comma-separated list of components to show: `cpu,mem,net,disk,process` |
| `--hide <COMPONENTS>`   | _(none)_                        | Components to hide — takes precedence over `--show`                    |
| `--status-bar <POS>`    | `top`                           | Status bar position: `top` \| `bottom` \| `hidden`                     |
| `--config <PATH>`       | `~/.config/toppers/config.toml` | Path to an alternate config file                                       |
| `--init-config`         | —                               | Print a default config template to stdout and exit                     |
| `--debug`               | off                             | Show the debug sidebar on launch                                       |
| `-v` / `-vv`            | —                               | Increase log verbosity                                                 |

---

## Configuration File

toppers reads `~/.config/toppers/config.toml` on startup. All fields are optional — omit any
section or key to keep the built-in default.

Run `toppers --init-config` to generate a commented template you can save and edit:

```toml
# toppers default configuration
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
# show = ["cpu", "mem", "net", "disk", "process"]

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
# focus_disk = "i"
# fullscreen = "f"
# help       = "?"
# debug      = "d"
```

### Notes

- `refresh_rate` uses [humantime](https://docs.rs/humantime) format: `500ms`, `1s`, `2s`, etc.
- `layout.show` controls which panels are visible. Components not listed are hidden entirely.
- `process.default_sort` accepts any column name: `pid` `name` `cpu` `mem` `status` `user`
  `priority` `nice` `virt` `res` `shr` `time`
- Keys in `[keybindings]` are single characters. Remapping them does not affect the `Tab`/`Shift+Tab`
  or arrow-key navigation.

---

## Contributing

Contributions are welcome!

- **Bug reports and feature requests** — please [open an issue](https://github.com/raysuliteanu/toppers/issues)
- **Code contributions** — fork the repository, create a branch, and open a pull request

<https://github.com/raysuliteanu/toppers>
