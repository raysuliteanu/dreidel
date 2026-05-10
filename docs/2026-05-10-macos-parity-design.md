# macOS Platform Parity — Design Spec

**Date:** 2026-05-10
**Status:** Approved for implementation

## Goal

Bring dreidel's runtime data parity on macOS as close as possible to the Linux
experience, using macOS-native APIs where procfs equivalents exist. Some gaps are
accepted as genuinely platform-specific (CPU temperatures, scaling governor, per-process
I/O syscall counts, shared-memory byte count). All silently-zero fields that _can_ be
populated on macOS _will_ be populated.

---

## Architecture

### New file: `src/stats/macos.rs`

All macOS-specific collection logic lives here, behind an implicit
`#[cfg(target_os = "macos")]` module declaration in `src/stats/mod.rs`. The file
mirrors the structure of the Linux-only functions already in `mod.rs` (e.g.
`read_cpu_totals`, `enumerate_threads`). No new abstraction layers are introduced;
`mod.rs` gains `#[cfg(target_os = "macos")]` call sites that parallel the existing
`#[cfg(target_os = "linux")]` ones.

Non-Linux, non-macOS platforms (e.g. FreeBSD) keep the existing zero/None stubs.

### New Cargo dependencies (macOS-only)

```toml
[target.'cfg(target_os = "macos")'.dependencies]
libproc = "0.14"
libc    = "0.2"
```

- **`libproc`** — safe Rust wrappers around the macOS `proc_pidinfo` family of
  syscalls. Used for all per-process enrichment (nice, priority, thread count,
  CPU time, page faults, context switches, fd count, TTY).
- **`libc`** — direct FFI for system-wide calls: `host_processor_info` (CPU mode
  ticks) and `host_statistics64` (memory breakdown and swap activity). Also used
  for the `sysctl(NET_RT_IFLIST2)` network drop counter path. Made an explicit
  macOS dependency even though it is already a transitive dep, to make intent
  clear.

---

## Data Model Changes (`src/stats/snapshots.rs`)

### `CpuSnapshot` — no struct changes

`physical_core_count`, `cpu_modes`, `aggregate`, `per_core`, `frequency`, and
`cpu_brand` are all populated on macOS. The Linux-only fields (`package_temp`,
`per_core_temp`, `governor`) remain `None` on macOS; no stable public API for
CPU temperature exists on Apple Silicon without private entitlements.

`CpuModes` on macOS is populated with `user`, `system`, `idle`, and `nice` from
`host_processor_info`. The `iowait`, `irq`, `softirq`, and `steal` fields are set
to `0.0` — those CPU states do not exist on macOS.

### `MemSnapshot` — four new fields

```rust
/// macOS: recently-used (active) memory in bytes. 0 on other platforms.
pub ram_active: u64,
/// macOS: reclaimable (inactive) memory in bytes. 0 on other platforms.
pub ram_inactive: u64,
/// macOS: non-pageable kernel (wired) memory in bytes. 0 on other platforms.
pub ram_wired: u64,
/// macOS: memory held in the compressor in bytes. 0 on other platforms.
pub ram_compressed: u64,
```

These are sourced from `host_statistics64(HOST_VM_INFO64)` via `vm_statistics64_data_t`.

**Field mapping on macOS** (all multiplied by `sysconf(_SC_PAGESIZE)`):

| `MemSnapshot` field | macOS source |
|---|---|
| `ram_free` | `free_count` |
| `ram_active` | `active_count` |
| `ram_inactive` | `inactive_count` |
| `ram_wired` | `wire_count` |
| `ram_compressed` | `compressor_page_count` |
| `ram_available` | `free_count + inactive_count` |
| `swap_in_bytes` | `pageins × page_size` |
| `swap_out_bytes` | `pageouts × page_size` |
| `ram_buffers` | `0` (no macOS equivalent) |
| `ram_cached` | `0` (use `ram_inactive` in UI instead) |

On Linux the four new fields stay `0`.

### `InterfaceSnapshot` — no struct changes

`rx_dropped` and `tx_dropped` already exist. They will be populated on macOS via
`sysctl(NET_RT_IFLIST2)`, which returns 64-bit counters (unlike `getifaddrs`, which
overflows on modern traffic volumes).

### `ProcessEntry` — no struct changes

The existing optional fields cover everything libproc can provide. Mapping:

| `ProcessEntry` field | macOS libproc source |
|---|---|
| `priority` | `PROC_PIDBSDINFO` → `pbsi_priority` |
| `nice` | `PROC_PIDBSDINFO` → `pbsi_nice` |
| `tty` | `PROC_PIDBSDINFO` → `pbsi_tdev` (major:minor formatted) |
| `threads` | `PROC_PIDTASKALLINFO` → `pti_threadnum` |
| `user_cpu_time_secs` | `PROC_PIDTASKALLINFO` → `pti_total_user` (ns → s) |
| `system_cpu_time_secs` | `PROC_PIDTASKALLINFO` → `pti_total_system` (ns → s) |
| `cpu_time_secs` | sum of above |
| `minor_faults` | `PROC_PIDTASKALLINFO` → `pti_faults` |
| `major_faults` | `PROC_PIDTASKALLINFO` → `pti_pageins` |
| `voluntary_ctxt_switches` | `PROC_PIDTASKALLINFO` → `pti_csw` (aggregate; nonvoluntary stays None) |
| `fd_count` | `PROC_PIDLISTFDS` → count of returned descriptors |

Fields with no macOS equivalent remain `0` / `None`:
`shr_bytes`, `swap_bytes`, `io_read/write_calls`, `io_read/write_chars`,
`cancelled_write_bytes`, `nonvoluntary_ctxt_switches`.

**EPERM / error handling:** any libproc call failure is silently ignored and the
field stays at its default (0 or None). Failures are logged at `tracing::debug!`
level so they are visible under `-vv` without polluting normal output.

---

## `src/stats/macos.rs` — Function Inventory

```
read_physical_core_count() -> Option<u32>
    sysinfo::System::physical_core_count()

read_cpu_modes() -> Option<CpuModes>
    host_processor_info(PROCESSOR_CPU_LOAD_INFO) → per-CPU tick arrays
    Sum across CPUs, delta from previous call → percentages
    Populates: user, system, idle, nice. Zeros: iowait, irq, softirq, steal.
    Returns None on the first call (no prior snapshot for delta) and on error.

read_mem_details() -> MacosMemDetails  (a local struct or tuple)
    host_statistics64(HOST_VM_INFO64) → vm_statistics64_data_t
    Returns all fields needed to populate MemSnapshot (see mapping table above).

read_net_drops(iface_name: &str) -> (u64, u64)
    sysctl(NET_RT_IFLIST2) → walk if_msghdr2 entries
    Match on interface name → return (rx_drop, tx_drop)
    Returns (0, 0) on any error.

enrich_process_entry(entry: &mut ProcessEntry, pid: u32)
    proc_pidinfo(PROC_PIDBSDINFO)    → nice, priority, tty
    proc_pidinfo(PROC_PIDTASKALLINFO) → threads, cpu times, faults, ctxsw
    proc_pidinfo(PROC_PIDLISTFDS)    → fd_count
    EPERM or any Err → log debug, return early (partial enrichment is fine)

enumerate_threads(sys: &System) -> Vec<ProcessEntry>
    For each process in sys.processes():
        proc_listpidthreads(pid) → Vec<u64> of TIDs
        For each TID (excluding the main thread TID == PID):
            proc_pidinfo(PROC_PIDTHREADINFO) → thread cpu time, state
            Build ProcessEntry { is_thread: true, parent_pid: Some(pid), … }
    Called on the slow cadence, same as the Linux enumerate_threads.
    EPERM / errors per-process or per-thread are skipped silently.
```

### Changes to `src/stats/mod.rs`

Replace the blanket `#[cfg(not(target_os = "linux"))]` stubs with paired
`#[cfg(target_os = "macos")]` / `#[cfg(not(any(target_os = "linux", target_os = "macos")))]`
blocks:

```rust
#[cfg(target_os = "linux")]
let cpu_snap = build_cpu(&sys, &components, &mut prev_cpu_total);
#[cfg(target_os = "macos")]
let cpu_snap = build_cpu_macos(&sys);
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
let cpu_snap = build_cpu_stub(&sys);
```

Same pattern for `build_mem`, `build_net` (enriched with `read_net_drops`),
`build_proc` (enriched with `enrich_process_entry`), and `enumerate_threads`.

The `prev_cpu_total` analogue for macOS CPU mode deltas lives in `mod.rs` alongside
the Linux one, behind its own cfg guard.

---

## Component UI Changes

### Memory panel (`src/components/mem.rs`)

The draw function gains a platform-specific stats block. On Linux the existing rows
render unchanged. On macOS:

```
Total:       16.0 GB      Used:       8.2 GB
Free:         2.1 GB      Active:     5.8 GB
Inactive:     2.3 GB      Wired:      1.6 GB
Compressed:   1.4 GB      Swap used:  0 B
Swap in:      0 B         Swap out:   0 B
```

Implemented as `#[cfg(target_os = "macos")]` / `#[cfg(target_os = "linux")]` blocks
in `draw()`, mirroring the existing cpu.rs pattern.

### Net panel (`src/components/net.rs`)

The "Drop TX / Drop RX" stat row is currently `#[cfg(target_os = "linux")]`. Expand
the condition to `#[cfg(any(target_os = "linux", target_os = "macos"))]` since both
platforms now populate those fields.

### CPU panel (`src/components/cpu.rs`)

No changes. Temperature and governor rows stay Linux-only, which is correct.
`physical_core_count` rendering is already guarded by `if let Some(phys)` — it will
display automatically once macOS populates the field.

### Process panel (`src/components/process/`)

No structural changes. libproc enrichment means more fields are non-zero on macOS;
the existing detail view renders `None` / `0` gracefully. The tree view gains thread
children on macOS once `enumerate_threads` is wired up.

---

## Snapshot Test Strategy

### Problem

The four failing snapshot tests (`cpu_with_data`, `cpu_fullscreen`, `help_overlay`,
`net_graph_view`) were generated on Linux. The `CpuSnapshot::stub()` contains
Linux-specific data (temperatures, governor) that the macOS rendering path ignores,
producing different layout widths.

### Fix: platform-neutral base stubs

Change the base `stub()` constructors to omit Linux-only optional data:

```rust
// CpuSnapshot::stub() — cross-platform baseline
pub fn stub() -> Self {
    Self {
        aggregate: 35.0,
        per_core: vec![42.0, 18.0, 75.0, 5.0],
        frequency: vec![3400, 3400, 3400, 3400],
        physical_core_count: None,   // was Some(4)
        cpu_brand: "Intel(R) Core(TM) i7-9750H CPU @ 2.60GHz".into(),
        package_temp: None,          // was Some(62.0)
        per_core_temp: vec![None; 4],// was vec![Some(55.0), …]
        governor: None,              // was Some("powersave")
        cpu_modes: None,             // was Some(CpuModes { … })
    }
}
```

Add a `stub_linux()` constructor that restores the Linux-specific values, used by
tests that specifically exercise Linux rendering paths (temperature column, governor
row).

`MemSnapshot::stub()` gains values for the four new macOS fields (realistic non-zero
values) for use in macOS mem panel tests; Linux mem panel tests use a separate
`stub_linux()`.

After updating stubs, run `INSTA_UPDATE=always cargo test` on macOS to generate the
canonical cross-platform snapshots. Linux-specific rendering is covered by dedicated
`#[cfg(target_os = "linux")]` test cases using `stub_linux()`.

---

## Accepted Gaps (macOS vs Linux)

| Feature | Status |
|---|---|
| CPU temperature (package + per-core) | Not available without private entitlements |
| CPU scaling governor | Not applicable on macOS |
| Per-process I/O syscall counts (syscr/syscw) | Not exposed by macOS libproc |
| Per-process shared memory (SHR) | No direct macOS equivalent |
| Per-process swap usage | Would require walking all VM regions via `PROC_PIDREGIONINFO` — expensive, deferred |
| Voluntary / nonvoluntary context switch split | macOS gives aggregate only; nonvoluntary stays None |
| iowait / irq / softirq / steal CPU modes | Linux-only kernel states; zeroed on macOS |
| Memory Buffers field | No macOS equivalent; stays 0 |

---

## Implementation Order

Suggested sequencing to keep the build green at each step:

1. **Snapshot stubs** — make base stubs platform-neutral, add `stub_linux()` variants,
   regenerate snapshots. Tests pass on both platforms after this step.
2. **`Cargo.toml`** — add `libproc` and `libc` as macOS-only dependencies.
3. **`src/stats/macos.rs` skeleton** — empty module with stub functions returning
   zeros/None; wire into `mod.rs` cfg blocks. Build stays green.
4. **CPU modes** — implement `read_cpu_modes()` and `read_physical_core_count()`;
   wire into `build_cpu` macOS variant.
5. **Memory** — add four new fields to `MemSnapshot`; implement `read_mem_details()`;
   update mem component draw for macOS panel; update stubs.
6. **Network drops** — implement `read_net_drops()`; expand net panel cfg condition.
7. **Process enrichment** — implement `enrich_process_entry()`; wire into `build_proc`.
8. **Thread enumeration** — implement `enumerate_threads()` for macOS; wire into
   slow-tick path.
9. **Docs update** — revise `USER_GUIDE.md` and `ARCHITECTURE.md` to reflect
   macOS support; update `Cargo.toml` description/keywords.
