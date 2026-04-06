# dreidel User Guide

A complete reference for every feature, keyboard shortcut, and
configuration option in dreidel.

---

## Table of Contents

1. [Interface Overview](#1-interface-overview)
2. [Keyboard Reference](#2-keyboard-reference)
3. [Components](#3-components)
   - [CPU](#31-cpu)
   - [Network](#32-network)
   - [Disk](#33-disk)
   - [Process](#34-process)
   - [Status Bar](#35-status-bar)
   - [Help Overlay](#36-help-overlay)
4. [Fullscreen Mode](#4-fullscreen-mode)
5. [Layouts](#5-layouts)
6. [Themes](#6-themes)
7. [CLI Reference](#7-cli-reference)
8. [Configuration File](#8-configuration-file)
9. [Troubleshooting](#9-troubleshooting)

---

## 1. Interface Overview

dreidel divides the terminal into panels. Each panel hosts one
component (CPU, Network, Disk, or Process). A status bar sits at
the top (or bottom) with uptime, load averages, and RAM/swap
gauges.

```
┌─ hostname ──────────────── uptime · load avg · time ──────────────────────┐
│  RAM [████████░░░░░░░░░░░░] 4.2G/16G  26%  │  SWAP [░░░░░░░░] 0B/4G   0%  │
├─────────────┬─────────────────────────────────────────────────────────────┤
│ [CPU]       │                                                             │
│  cpu00  12% │  PID   NAME             CPU%  MEM%   S                      │
│  cpu01   8% │  1234  firefox          42.1   3.2   R                      │
│  cpu02   3% │   891  code             12.3   2.1   S                      │
│  ...        │   ...                                                       │
├─────────────┤                                                             │
│ Net         │                                                             │
│ ▶ eth0      │                                                             │
│   lo        │                                                             │
├─────────────┤                                                             │
│ Disk        │                                                             │
│   sda  72%  │                                                             │
└─────────────┴─────────────────────────────────────────────────────────────┘
```

**Focus** is indicated by a highlighted border. Press the
component's focus key (`c`, `n`, `d`, `p`) or `Tab`/`Shift+Tab`
to move focus. Most navigation keys only act on the focused
component.

---

## 2. Keyboard Reference

### Global Keys

These work from any component at any time.

| Key             | Action                                      |
| --------------- | ------------------------------------------- |
| `c`             | Focus the CPU panel                         |
| `n`             | Focus the Network panel                     |
| `d`             | Focus the Disk panel                        |
| `p`             | Focus the Process panel                     |
| `Tab`           | Cycle focus forward through visible panels  |
| `Shift+Tab`     | Cycle focus backward through visible panels |
| `f`             | Toggle fullscreen for the focused panel     |
| `?`             | Open/close the help overlay                 |
| `q` / `Esc`     | Quit (or exit fullscreen / close overlay)   |

All focus keys (`c`, `n`, `d`, `p`, `f`, `?`) are remappable in
the [config file](#keybindings).

### CPU Keys

| Key             | Action                                      | Mode        |
| --------------- | ------------------------------------------- | ----------- |
| `↑` / `↓`       | Scroll the per-core view up/down            | Normal      |
| `PageUp` / `PageDown` | Scroll by 8 cores                     | Normal      |
| `/`             | Enter filter mode                           | Normal      |
| Any character   | Append to filter query                      | Filter mode |
| `Backspace`     | Delete last filter character                | Filter mode |
| `Enter`         | Confirm filter and return to normal         | Filter mode |
| `Esc`           | Clear filter and return to normal           | Filter mode |

### Network Keys

| Key             | Action                                                           | Mode        |
| --------------- | ---------------------------------------------------------------- | ----------- |
| `↑` / `↓`       | Move interface selection                                         | List        |
| `PageUp` / `PageDown` | Jump 10 interfaces                                         | List        |
| `Enter`         | Open per-interface graph (auto-enters fullscreen if not already) | List        |
| `/`             | Enter filter mode                                                | List        |
| Any character   | Append to filter query                                           | Filter mode |
| `Backspace`     | Delete last filter character                                     | Filter mode |
| `Enter`         | Confirm filter and return to list                                | Filter mode |
| `Esc`           | Clear filter and return to list                                  | Filter mode |
| `Esc` / `q`     | Close graph and return to list                                   | Detail view |

### Disk Keys

| Key             | Action                                                           | Mode        |
| --------------- | ---------------------------------------------------------------- | ----------- |
| `↑` / `↓`       | Move device selection                                            | List        |
| `PageUp` / `PageDown` | Jump 10 devices                                            | List        |
| `Enter`         | Open per-device graph (auto-enters fullscreen if not already)    | List        |
| `/`             | Enter filter mode                                                | List        |
| Any character   | Append to filter query                                           | Filter mode |
| `Backspace`     | Delete last filter character                                     | Filter mode |
| `Enter`         | Confirm filter and return to list                                | Filter mode |
| `Esc`           | Clear filter and return to list                                  | Filter mode |
| `Esc` / `q`     | Close graph and return to list                                   | Detail view |

### Process Keys

| Key             | Action                                               | Mode         |
| --------------- | ---------------------------------------------------- | ------------ |
| `↑` / `↓`       | Move row selection                                   | List         |
| `PageUp` / `PageDown` | Jump 10 rows                                   | List         |
| `Enter`         | Open detailed process info                           | List         |
| `Esc` / `q`     | Close detail / filter / kill dialog; or quit app     | Any          |
| `/`             | Enter filter mode                                    | List         |
| Any character   | Append to filter query                               | Filter mode  |
| `Backspace`     | Delete last filter character                         | Filter mode  |
| `Enter`         | Confirm filter and return to list                    | Filter mode  |
| `s`             | Cycle sort column (left-to-right across visible columns) | List     |
| `S`             | Toggle sort direction (ascending ↔ descending)       | List         |
| `t`             | Toggle tree view (parent/child hierarchy)            | List         |
| `Space`         | Expand/collapse tree node                            | List (tree)  |
| `k`             | Kill selected process (prompts for confirmation)     | List         |
| `Tab`           | Toggle focus between Cancel and OK buttons           | Kill confirm |
| `Enter`         | Activate focused button (Cancel or OK)               | Kill confirm |
| `Esc` / `q`     | Cancel kill                                          | Kill confirm |

---

## 3. Components

### 3.1 CPU

The CPU panel shows per-core usage history as a scrolling line
chart. Each core gets its own color, drawn with braille characters
for smooth resolution.

**Compact layout** (left column of sidebar/classic presets):

```
┌─ CPU ───────────────────────┐
│⠈⠘⢸⡰⢿⡷⢿⡿⢿⣿⣿  cpu00  12%      │
│⠁⠸⡐⣀⢰⣿⡿⡷⡿⣟⢿  cpu01   8%      │
│⢀⡀⡀⣴⡿⣿⣾⡿⣿⡿⡿  cpu02   3%      │
│⡐⡄⢸⢰⣿⣿⣿⣿⣿⣿⣟  cpu03   1%      │
└─────────────────────────────┘
```

- The label column on the right shows the current percentage for
  each core. On Linux, per-core temperatures are shown alongside
  the percentages when sensor data is available.
- Up to 8 cores are visible in compact mode; use `↑`/`↓` or
  `PageUp`/`PageDown` to scroll.

**Filtering** (`/` when focused):

Press `/` to enter filter mode. The title changes to show the
active query:

```
┌─ [C]PU [/cpu0▌] ───────────────┐
```

Type a substring to narrow which cores are visible (case-insensitive
match against the core label, e.g. `cpu0` matches `cpu0`, `cpu00`,
`cpu01`…). Press `Enter` to keep the filter or `Esc` to clear it.
When a filter is active but not being edited the title shows
`[/query]` without the cursor.

**Fullscreen** (`f` when focused):

- A stats header appears at the top with: CPU brand,
  logical/physical core count, average frequency, temperature
  (Linux), and CPU governor (Linux).
- All cores are visible and scrollable.

### 3.2 Network

The Network panel lists all interfaces with live RX/TX rates. An
aggregate chart is shown at the top of the panel when there is
enough vertical space.

**List view:**

```
┌─ Net ───────────────────────────────────────────────────────────────────┐
│ (aggregate chart, if height ≥ 9 rows)                                   │
├─────────────────────────────────────────────────────────────────────────┤
│  Interface      TX             RX                                       │
│▶ eth0       1.2 MB/s       3.4 MB/s                                     │
│  lo           0 B/s          0 B/s                                      │
└─────────────────────────────────────────────────────────────────────────┘
```

When fullscreen or in a wide layout (≥ 100 columns), additional
columns appear: TX packets/sec, RX packets/sec, and IP address.

**Filtering** (`/` when focused):

Press `/` to enter filter mode. The title changes to show the query:

```
┌─ [N]ET [/eth▌] ─────────────────────────────────────────────────────────┐
```

Type a substring to narrow the interface list (case-insensitive).
Press `Enter` to keep the filter or `Esc` to clear it.

**Per-interface detail view** (`Enter` on a selected interface):

Pressing `Enter` opens a full-height graph for just that
interface. If the panel isn't already fullscreen, dreidel enters
fullscreen automatically.

```
┌─ eth0 ─────────────────────────────────────────────────────────────────┐
│  TX: 1.2 MB/s   RX: 3.4 MB/s   Pkts TX: 890/s   RX: 1.2k/s             │
│  IP: 192.168.1.42                                                      │
│                                                                        │
│  (full-height TX/RX line chart, last 100 samples)                      │
│                                                                        │
└────────────────────────────────────────────────────────────────────────┘
```

Two datasets are plotted simultaneously (TX and RX) in distinct
colors. The Y-axis auto-scales to the current peak rate. Press
`Esc` or `q` to return to the list (and exit fullscreen if it was
auto-opened).

### 3.3 Disk

The Disk panel lists storage devices with per-device read/write
rates and usage percentage. Usage is color-coded green → orange
(≥70%) → red (≥90%).

**List view:**

```
┌─ Disk ────────────────────────────────────────────────┐
│  Device        Read         Write    Usage            │
│▶ sda        2.1 MB/s     512 KB/s     72%             │
│  sdb          0 B/s        0 B/s       8%             │
└───────────────────────────────────────────────────────┘
```

**Filtering** (`/` when focused):

Press `/` to enter filter mode. The title changes to show the query:

```
┌─ [D]ISK [/sda▌] ──────────────────────────────────────────────────────┐
```

Type a substring to narrow the device list (case-insensitive).
Press `Enter` to keep the filter or `Esc` to clear it.

**Per-device detail view** (`Enter` on a selected device):

Same behavior as Network: pressing `Enter` opens a full-height
read/write graph for the selected device, auto-entering fullscreen
if needed. Press `Esc` or `q` to return.

### 3.4 Process

The Process panel is a live, sortable, filterable process table.

**Two display modes**, switching automatically based on terminal
width:

| Mode     | Columns                                                   | When                                |
| -------- | --------------------------------------------------------- | ----------------------------------- |
| Normal   | PID · Name · CPU% · MEM% · Status                        | Width < 120 columns                 |
| Extended | PID · User · PR · NI · VIRT · RES · SHR · S · %CPU · %MEM · TIME · Command | Width ≥ 120 columns or fullscreen |

**Sorting**

Press `s` to cycle through sortable columns in left-to-right
display order. The active sort column shows `▼` (descending) or
`▲` (ascending) in the header. Press `S` to flip direction.

Default sort order is CPU% descending (configurable via
`process.default_sort`).

**Tree view**

Press `t` to toggle between flat list and tree view. In tree
mode, processes are arranged in a parent/child hierarchy with
indentation showing depth. Press `Space` to collapse or expand a
node's children. On Linux, per-process threads appear as children
of their parent process.

To start in tree mode by default, set `show_tree = true` in the
`[process]` config section.

**Filtering**

Press `/` to open the filter prompt. The title bar changes to
show your current query:

```
[P]rocesses [filter: firefox▌]
```

The list updates in real time as you type.

| Input             | Filters by                                       |
| ----------------- | ------------------------------------------------ |
| _(empty)_         | No filter — all processes shown                  |
| `1234`            | Exact PID match                                  |
| `s:sleeping`      | Status substring (case-insensitive)              |
| `firefox`         | Name substring (case-insensitive)                |

Press `Esc` to clear the filter, or `Enter` to keep it and return
to the list.

**Detail view**

Press `Enter` on any process to open a two-column detail inspector:

```
 Name:     firefox
 Command:  firefox --new-window
 Exe:      /usr/bin/firefox
 CWD:      /home/alice
─────────────────────────────────────────────────────────────────
 PID             1234            PPID            1
 User            alice           Status          running
 Type            process         Session         500
 CPU             42.1%           CPU time        2h 03m 12s
 User CPU        1h 40m 00s      Sys CPU         0m 23s
 MEM             3.2% (536.9 MB) VIRT            2.1 GB
 SHR             134.2 MB        Swap            16.8 MB
 Threads         42              FDs             300
 ...
─────────────────────────────────────────────────────────────────
                          [Esc/q] back
```

The left column shows identity, CPU, memory, scheduling, and fault
data; the right column shows corresponding paired fields. Press
`Esc` or `q` to close.

**Killing a process**

Press `k` to kill the selected process. A confirmation dialog
appears with **Cancel** (focused by default) and **OK** buttons.
Use `Tab` to switch focus between buttons. Press `Enter` to
activate the focused button, or `Esc`/`q` to cancel. Confirming
sends `SIGTERM` to the process. If
the kill fails (e.g., insufficient permissions), an error dialog
appears; press `Enter`, `Esc`, or `Space` to dismiss.

### 3.5 Status Bar

The status bar can be positioned at the `top` (default), `bottom`,
or `hidden` via `--status-bar` or `layout.status_bar` in the
config file.

```
┌─ hostname ──────────────── 2d 4h 31m  ·  0.42 0.38 0.31  ·  14:52:07 ──────┐
│  RAM [████████░░░░░░░░░░░░] 4.2G/16G  26%  │  SWAP [░░░░░░░░] 0B/4G  0%    │
└────────────────────────────────────────────────────────────────────────────┘
```

- **Top row:** system uptime, 1/5/15-minute load averages, current
  time
- **Bottom row:** RAM and (if configured) SWAP gauges; SWAP turns
  orange when any swap is in use

### 3.6 Help Overlay

Press `?` from anywhere to open the help overlay. It shows all
key bindings, the config file path, and the log file path. Press
`?`, `h`, `Esc`, or `q` to close it. The dashboard continues
updating behind the overlay.

---

## 4. Fullscreen Mode

Pressing `f` while any component is focused expands it to fill
the entire terminal. Press `f` again, or `Esc`/`q`, to return to
the normal layout.

Several components also enter fullscreen automatically when you
open a detail view (Network, Disk). In that case, closing the
detail view also exits fullscreen.

**What changes in fullscreen:**

| Component | Fullscreen extras |
| --------- | ----------------- |
| CPU       | Stats header: brand, core count, frequency, temperature (Linux), governor (Linux) |
| Network   | Per-interface graph replaces the list; extended columns (packets/sec, IP) always visible |
| Disk      | Per-device graph replaces the list |
| Process   | Always uses the extended 12-column layout regardless of terminal width |

---

## 5. Layouts

Select a layout with `--preset <NAME>` or `layout.preset` in the
config file.

### sidebar _(default)_

Narrow left column (CPU, Net, Disk) beside a tall process list.

```
┌─────────────────────────────────────────────────────┐
│ CPU (adaptive height)  │                            │
├────────────────────────┤   Process                  │
│ Net                    │                            │
├────────────────────────┤                            │
│ Disk                   │                            │
└────────────────────────┴────────────────────────────┘
```

### classic

CPU top-left, Disk and Net stacked top-right, Process fills the
bottom.

```
┌─────────────────────────────────────────────────────┐
│ CPU (adaptive height)  │ Disk                       │
│                        ├────────────────────────────┤
│                        │ Net                        │
├────────────────────────┴────────────────────────────┤
│ Process                                             │
└─────────────────────────────────────────────────────┘
```

### dashboard

CPU strip across the top, Disk and Net side-by-side in the middle,
Process fills the bottom.

```
┌─────────────────────────────────────────────────────┐
│ CPU (adaptive height)                               │
├──────────────────────────┬──────────────────────────┤
│ Disk                     │ Net                      │
├──────────────────────────┴──────────────────────────┤
│ Process  (always extended columns — panel is wide)  │
└─────────────────────────────────────────────────────┘
```

### grid

Two-column layout: Disk and Net stacked on the left; CPU and
Process stacked on the right.

```
┌───────────────────────┬─────────────────────────────┐
│ Disk                  │ CPU (adaptive height)       │
├───────────────────────┤                             │
│ Net                   │ Process                     │
└───────────────────────┴─────────────────────────────┘
```

### Slot Overrides

You can reassign which component occupies each slot in the config
file:

```toml
[layout]
preset = "sidebar"
# sidebar slots: left_top, left_bot, left_extra, right
left_top = "net"   # put Net at the top of the left column instead of CPU
```

Available slot names per preset:

| Preset      | Slot names                                      |
| ----------- | ----------------------------------------------- |
| `sidebar`   | `left_top`, `left_bot`, `left_extra`, `right`   |
| `classic`   | `top_left`, `top_right_top`, `top_right_bot`, `bottom` |
| `dashboard` | `top`, `mid_left`, `mid_right`, `bottom`        |
| `grid`      | `grid_left_mid`, `grid_left_bot`, `grid_right_top`, `grid_right_bot` |

---

## 6. Themes

| Flag / Config value | Behavior                                                                                  |
| ------------------- | ----------------------------------------------------------------------------------------- |
| `auto` _(default)_  | Queries the terminal background color on startup (OSC 11); falls back to dark if no reply |
| `dark`              | Dark background, vivid colors                                                             |
| `light`             | Light background, muted colors optimized for contrast                                     |

Pass `--theme <VALUE>` on the command line or set `theme` under
`[general]` in the config file. The command-line flag takes
precedence.

---

## 7. CLI Reference

```
dreidel [OPTIONS]
```

| Flag                    | Default                         | Description                                                        |
| ----------------------- | ------------------------------- | ------------------------------------------------------------------ |
| `--theme <THEME>`       | `auto`                          | Color theme: `auto` \| `light` \| `dark`                           |
| `--refresh-rate <RATE>` | `1s`                            | Stats refresh interval, e.g. `500ms`, `2s`                         |
| `--thread-refresh <RATE>` | `5s`                          | Thread enumeration interval (Linux), e.g. `5s`, `10s`             |
| `--preset <LAYOUT>`     | `sidebar`                       | Layout: `sidebar` \| `classic` \| `dashboard` \| `grid`            |
| `--show <COMPONENTS>`   | _(all)_                         | Comma-separated list of components to show: `cpu,net,disk,process` |
| `--hide <COMPONENTS>`   | _(none)_                        | Components to hide (takes precedence over `--show`)                |
| `--status-bar <POS>`    | `top`                           | Status bar position: `top` \| `bottom` \| `hidden`                 |
| `--config <PATH>`       | `~/.config/dreidel/config.toml` | Path to an alternate config file                                   |
| `--init-config`         | —                               | Print a default config template to stdout and exit                 |
| `--detect-theme`        | —                               | Print terminal theme detection diagnostics and exit                |
| `-v` / `-vv`            | —                               | Increase log verbosity (INFO / DEBUG)                              |
| `--help`                | —                               | Show usage and exit                                                |
| `--version`             | —                               | Show version and exit                                              |

**Examples:**

```sh
# Dark theme, half-second refresh, dashboard layout
dreidel --theme dark --refresh-rate 500ms --preset dashboard

# Show only CPU and Process
dreidel --show cpu,process

# Monitor network only (hide everything else)
dreidel --hide cpu,disk,process

# Use an alternate config file
dreidel --config ~/my-dreidel.toml
```

---

## 8. Configuration File

dreidel reads `~/.config/dreidel/config.toml` on startup. All
sections and keys are optional; omit anything to keep the
built-in default.

Run `dreidel --init-config` to generate a commented template you
can save and edit.

### \[general\]

```toml
[general]
# Stats refresh interval. Humantime format: "500ms", "1s", "2s", etc.
refresh_rate = "1s"

# How often to enumerate per-process threads (Linux only).
# Thread enumeration is expensive; a slower cadence avoids thousands
# of syscalls every tick.
thread_refresh = "5s"

# Color theme: "auto" | "light" | "dark"
theme = "auto"

# Bounded action-channel capacity (internal tuning; rarely needs changing)
channel_capacity = 128
```

### \[layout\]

```toml
[layout]
# Base layout preset: "sidebar" | "classic" | "dashboard" | "grid"
preset = "sidebar"

# Status bar position: "top" | "bottom" | "hidden"
status_bar = "top"

# Which components to show. Omit a component to hide it entirely.
show = ["cpu", "net", "disk", "process"]

# Slot overrides — assign a different component to a named slot.
# Sidebar slots:   left_top, left_bot, left_extra, right
# Classic slots:   top_left, top_right_top, top_right_bot, bottom
# Dashboard slots: top, mid_left, mid_right, bottom
# left_top = "net"
```

### \[process\]

```toml
[process]
# Default sort column.
# Normal view:   "pid" | "name" | "cpu" | "mem" | "status"
# Extended view: also "user" | "priority" | "nice" | "virt"
#                     "res"  | "shr"      | "time"
default_sort = "cpu"

# Default sort direction: "asc" | "desc"
default_sort_dir = "desc"

# Start in tree view (parent/child hierarchy) instead of flat list.
show_tree = false
```

### \[keybindings\]

All keys are single characters. Remapping them does not affect
arrow keys, `Tab`/`Shift+Tab`, or `Esc`.

```toml
[keybindings]
focus_proc = "p"   # Focus Process panel
focus_cpu  = "c"   # Focus CPU panel
focus_net  = "n"   # Focus Network panel
focus_disk = "d"   # Focus Disk panel
fullscreen = "f"   # Toggle fullscreen for focused panel
help       = "?"   # Open help overlay
```

---

## 9. Troubleshooting

| Symptom | Fix |
| ------- | --- |
| Colors look wrong | Use `--theme dark` or `--theme light` to override auto-detection |
| Terminal garbled after exit | Run `reset` or `stty sane` to restore terminal state |
| Help overlay won't close | Press `Esc`, `?`, `h`, or `q` |
| Fullscreen won't exit | Press `f` to toggle, or `Esc`/`q` |
| Config file ignored | Verify path is `~/.config/dreidel/config.toml`; use `--config <path>` to specify an alternate |
| Process list empty | Check that `process` is in `layout.show` and not in `--hide` |
| Filter not matching | Press `/` first to enter filter mode; `Esc` clears the filter (works in CPU, Net, Disk, and Process panels) |
| Can't kill a process | You need to own the process or run dreidel as root; the error dialog will explain |
| High CPU use by dreidel | Increase `--refresh-rate` to `2s` or more |
| Temperature not shown | Temperature collection requires Linux and a supported CPU sensor |

**Log file:** `~/.local/share/dreidel/dreidel.log`

Run with `-vv` for verbose output when diagnosing unexpected
behavior.
