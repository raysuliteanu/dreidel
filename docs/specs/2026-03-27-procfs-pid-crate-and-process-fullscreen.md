# Spec: procfs-pid Crate + Process Fullscreen Extended View

## Context
The process component needs a fullscreen extended table (top-style: PID, User, PR, NI,
VIRT, RES, SHR, S, %CPU, %MEM, TIME, Command). sysinfo doesn't expose priority, SHR,
thread count, or CPU time — these require reading `/proc/<pid>/stat` and
`/proc/<pid>/statm` directly. The work is split into a publishable standalone crate
(`procfs-pid`, name confirmed available on crates.io) and the toppers integration.

---

## Part 1 — Convert to Cargo Workspace

**File:** `Cargo.toml`

Add a `[workspace]` section to the existing root `Cargo.toml` (the toppers package
stays at the root — no directory moves needed):

```toml
[workspace]
members = [".", "crates/procfs-pid"]
resolver = "2"
```

---

## Part 2 — Create `crates/procfs-pid/`

### File layout
```
crates/procfs-pid/
├── Cargo.toml
└── src/
    ├── lib.rs      — public API, re-exports, ProcPid, ProcFs
    ├── error.rs    — ProcError (thiserror)
    ├── stat.rs     — /proc/<pid>/stat parser + Stat struct
    └── statm.rs    — /proc/<pid>/statm parser + Statm struct
```

Phase 2 additions slot in as new files: `status.rs`, `io.rs`, etc.

### `crates/procfs-pid/Cargo.toml`
```toml
[package]
name = "procfs-pid"
version = "0.1.0"
edition = "2024"
description = "Read and parse /proc/<pid>/stat and /proc/<pid>/statm on Linux"

[dependencies]
thiserror = "2"
# No libc dependency — page_size and clk_tck are read from /proc/self/auxv
```

### `src/error.rs`
```rust
#[derive(Debug, thiserror::Error)]
pub enum ProcError {
    #[error("process {pid} not found")]
    NotFound { pid: u32 },
    #[error("i/o error reading /proc/{pid}/{file}: {source}")]
    Io { pid: u32, file: &'static str, #[source] source: std::io::Error },
    #[error("parse error in /proc/{pid}/{file} field {field}: {msg}")]
    Parse { pid: u32, file: &'static str, field: &'static str, msg: String },
    #[cfg(not(target_os = "linux"))]
    #[error("procfs is only available on Linux")]
    NotSupported,
}
```

### `src/stat.rs` — key design points

**Parsing `/proc/<pid>/stat`:**
The `comm` field (field 2) can contain spaces and is wrapped in `()`. Parse by
finding the *last* `)` in the file content, then split the suffix on whitespace.
Fields are 1-indexed per the man page.

**Fields to capture (Phase 1):**

| stat field | name | type | notes |
|---|---|---|---|
| 14 | utime | u64 | user-mode CPU ticks |
| 15 | stime | u64 | kernel-mode CPU ticks |
| 18 | priority | i64 | kernel raw priority (see PR note below) |
| 19 | nice | i64 | nice value (-20 to 19) |
| 20 | num_threads | i64 | thread count |
| 23 | vsize | u64 | virtual size in bytes |
| 24 | rss | i64 | RSS in pages — convert: rss × page_size |

**Derived fields:**
- `rss_bytes = rss_pages * page_size`
- `cpu_time_secs = (utime + stime) as f64 / clk_tck`

Both `page_size` and `clk_tck` are read from `/proc/self/auxv` (the ELF auxiliary
vector the kernel writes for every process) — no `libc` dependency needed:

```rust
/// Read a value from the kernel's ELF auxiliary vector in /proc/self/auxv.
/// The file is a flat array of (usize, usize) pairs; AT_NULL (0) terminates it.
fn read_auxv(key: usize) -> Option<usize> {
    let data = std::fs::read("/proc/self/auxv").ok()?;
    let sz = std::mem::size_of::<usize>(); // 8 on 64-bit, 4 on 32-bit
    for chunk in data.chunks_exact(sz * 2) {
        let k = usize::from_ne_bytes(chunk[..sz].try_into().ok()?);
        let v = usize::from_ne_bytes(chunk[sz..].try_into().ok()?);
        if k == 0 { break; }
        if k == key { return Some(v); }
    }
    None
}

const AT_PAGESZ: usize = 6;   // page size in bytes
const AT_CLKTCK: usize = 17;  // clock ticks per second

fn page_size() -> u64 { read_auxv(AT_PAGESZ).unwrap_or(4096) as u64 }
fn clk_tck()   -> f64 { read_auxv(AT_CLKTCK).unwrap_or(100)  as f64 }
```

Fallbacks (4096, 100) are the correct universal values on modern Linux x86_64.

**PR column note:** stat field 18 `priority` is the kernel's internal priority value.
For normal processes it ranges 0–39 (where 20 = nice 0). For RT processes it is
negative. The display layer decides how to render this — the crate exposes the raw
value and documents the semantics.

**Struct:**
```rust
#[derive(Debug, Clone, Default)]
pub struct Stat {
    pub priority: i64,
    pub nice: i64,
    pub num_threads: i64,
    pub vsize_bytes: u64,
    pub rss_bytes: u64,
    pub utime_ticks: u64,
    pub stime_ticks: u64,
    pub cpu_time_secs: f64,
    // Phase 2: starttime, processor, rt_priority, ...
}

impl Stat {
    pub fn read(pid: u32) -> crate::Result<Self>;
}
```

### `src/statm.rs`

**Fields (Phase 1):** field 3 (shared pages).

```rust
#[derive(Debug, Clone, Default)]
pub struct Statm {
    pub shared_bytes: u64,
    // Phase 2: size_bytes, resident_bytes, text_bytes, data_bytes
}

impl Statm {
    pub fn read(pid: u32) -> crate::Result<Self>;
}
```

### `src/lib.rs` — public API

```rust
pub type Result<T> = std::result::Result<T, ProcError>;

/// All /proc/<pid>/ data for a single process.
/// Call refresh() to update fields in place (avoids reallocation).
pub struct ProcPid {
    pub pid: u32,
    pub stat: Stat,
    pub statm: Statm,
}

impl ProcPid {
    pub fn new(pid: u32) -> Self;
    /// Read /proc/<pid>/stat and /proc/<pid>/statm, updating fields in place.
    pub fn refresh(&mut self) -> Result<()>;
}

/// System-wide snapshot across all running processes.
/// Mirrors sysinfo's refresh() pattern — call refresh() on a timer,
/// then query via get() or processes().
pub struct ProcFs {
    processes: HashMap<u32, ProcPid>,
}

impl ProcFs {
    pub fn new() -> Self;
    /// Scan /proc/ for all numeric directories, updating existing entries
    /// and inserting new ones. Dead processes are not removed automatically
    /// — call remove_dead() separately if needed.
    pub fn refresh(&mut self) -> Result<()>;
    pub fn get(&self, pid: u32) -> Option<&ProcPid>;
    pub fn processes(&self) -> impl Iterator<Item = (&u32, &ProcPid)>;
    /// Remove entries for PIDs that no longer have a /proc/<pid> directory.
    /// Returns the count of removed entries.
    pub fn remove_dead(&mut self) -> usize;
}
```

**Platform guard:** All `impl` blocks and `libc` calls are gated with
`#[cfg(target_os = "linux")]`. On other platforms the structs exist but all methods
return `Err(ProcError::NotSupported)`.

---

## Part 3 — Integrate into Toppers

### `Cargo.toml` (root)
```toml
[dependencies]
procfs-pid = { path = "crates/procfs-pid" }
```

### `src/stats/snapshots.rs` — extend `ProcessEntry`
Add fields:
```rust
pub priority: i32,      // from /proc/<pid>/stat field 18
pub shr_bytes: u64,     // from /proc/<pid>/statm field 3 × page_size
pub cpu_time_secs: f64, // (utime + stime) / CLK_TCK
// fix existing (were hardcoded 0):
pub num_threads: u32,   // from /proc/<pid>/stat field 20
pub nice: i32,          // from /proc/<pid>/stat field 19
```
Update `ProcessEntry::stub()` with sensible non-zero defaults for the new fields.

### `src/stats/mod.rs` — populate in `build_proc()`
The collector task owns a `ProcFs` instance (initialized once, refreshed each tick
alongside sysinfo). In `build_proc()`, look up `proc_fs.get(pid)` and copy the new
fields into `ProcessEntry`. Gate with `#[cfg(target_os = "linux")]`; default to 0 on
other platforms.

### `src/components/process/mod.rs` — fullscreen extended table

**New field:** `is_fullscreen: bool`

**Toggle logic:**
- `set_focused(false)` → `is_fullscreen = false`
- `update(Action::ToggleFullScreen)` when `self.focused` → `is_fullscreen = !is_fullscreen`

**Fullscreen column layout (12 columns):**
```
PID(7) User(10) PR(4) NI(4) VIRT(10) RES(10) SHR(10) S(2) %CPU(6) %MEM(6) TIME(10) Command(Fill)
```

**TIME format:** `MM:SS` from `cpu_time_secs` (e.g. 123.4s → `02:03`)

**PR display:** raw `priority` field; negative values indicate RT processes.

In `draw()`, branch:
```rust
if self.is_fullscreen {
    self.draw_fullscreen(frame, inner)
} else {
    self.draw_normal(frame, inner)
}
```

Extract the current table render into `draw_normal()`, add `draw_fullscreen()`.

---

## Files to Create / Modify

| File | Action |
|------|--------|
| `Cargo.toml` | Add `[workspace]` section + `procfs-pid` path dependency |
| `crates/procfs-pid/Cargo.toml` | Create |
| `crates/procfs-pid/src/lib.rs` | Create |
| `crates/procfs-pid/src/error.rs` | Create |
| `crates/procfs-pid/src/stat.rs` | Create |
| `crates/procfs-pid/src/statm.rs` | Create |
| `src/stats/snapshots.rs` | Add 5 fields to ProcessEntry, update stub() |
| `src/stats/mod.rs` | Own ProcFs, populate new fields in build_proc() |
| `src/components/process/mod.rs` | is_fullscreen, draw_normal(), draw_fullscreen() |

## Implementation Order
1. Workspace conversion (`Cargo.toml`)
2. `crates/procfs-pid/` crate — error, stat, statm, lib, tests
3. Extend `ProcessEntry` + update `build_proc()` + update `stub()`
4. Process component fullscreen table

## Verification
```bash
cargo test -p procfs-pid        # unit tests for /proc parsing
cargo test                      # all toppers tests pass
cargo run -- --debug            # smoke test binary
# Focus process panel, press f → fullscreen extended 12-column table visible
# Verify PR/NI/SHR/TIME columns have real (non-zero) values for running processes
```
