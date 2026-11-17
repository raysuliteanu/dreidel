# macOS Platform Parity — Design Spec

**Date:** 2026-05-10 (amended 2026-09-15: CPU temperature via SMC, GPU section;
amended 2026-11-16: GPU section sourced from `sysinfo` 0.40's `Gpus` API, Linux included)
**Status:** Approved for implementation

## Goal

Bring dreidel's runtime data parity on macOS as close as possible to the Linux
experience, using macOS-native APIs where procfs equivalents exist. Some gaps are
accepted as genuinely platform-specific (scaling governor, per-process I/O syscall
counts, shared-memory byte count). All silently-zero fields that _can_ be populated
on macOS _will_ be populated.

Beyond parity, this spec adds a **GPU section in the CPU panel** (utilization, plus
memory where the platform reports it), analogous to btop's "gpu-totals" row in its CPU
box. It is sourced from `sysinfo`'s cross-platform `Gpus` API, so it works on Linux as
well as macOS with no platform-specific GPU code in dreidel.

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
objc2-io-kit = "0.3"
objc2-core-foundation = "0.3"
```

- **`libproc`** — safe Rust wrappers around the macOS `proc_pidinfo` family of
  syscalls. Used for all per-process enrichment (nice, priority, thread count,
  CPU time, page faults, context switches, fd count, TTY).
- **`libc`** — direct FFI for system-wide calls: `host_processor_info` (CPU mode
  ticks) and `host_statistics64` (memory breakdown and swap activity). Also used
  for the `sysctl(NET_RT_IFLIST2)` network drop counter path. Made an explicit
  macOS dependency even though it is already a transitive dep, to make intent
  clear.
- **`objc2-io-kit`** + **`objc2-core-foundation`** — IOKit user-client access for
  CPU temperature (`AppleSMC` user client). Both are already transitive deps of
  `sysinfo` (0.3.2 in `Cargo.lock`), so this adds no new crates to the build.

### `sysinfo` 0.40 with the `gpu` feature

```toml
sysinfo = { version = "0.40.4", features = ["gpu"] }
```

`sysinfo` was bumped from 0.38 to 0.40 (MSRV 1.88 → 1.95) as the first step of the GPU
work; 0.40 adds the `Gpus` API behind the opt-in `gpu` feature. Enabling the feature
pulls in no new crates on Linux (NVML and Vulkan are `dlopen`ed at runtime) and only
the `objc2-*` crates listed above on macOS.

---

## Data Model Changes (`src/stats/snapshots.rs`)

### `CpuSnapshot` — existing fields

`physical_core_count`, `cpu_modes`, `aggregate`, `per_core`, `frequency`, and
`cpu_brand` are all populated on macOS. `governor` remains `None` on macOS (not
applicable).

**CPU temperature is populated on macOS via the SMC.** The earlier assumption that
this needed private entitlements was wrong: btop 1.4.7 reads it unprivileged through
the `AppleSMC` IOKit user client, and so will we. `package_temp` comes from the
`TC0P` key (CPU proximity / package). `per_core_temp` is filled from the per-core
keys where present (`Tp0?` / `Tc0?` families on Apple Silicon); on machines where
per-core keys are absent, `per_core_temp` is `vec![None; n]` and only the package
row renders. The doc comments on these two fields change from "Linux-only" to
"Linux and macOS; `None` elsewhere".

`CpuModes` on macOS is populated with `user`, `system`, `idle`, and `nice` from
`host_processor_info`. The `iowait`, `irq`, `softirq`, and `steal` fields are set
to `0.0` — those CPU states do not exist on macOS.

### `CpuSnapshot` — one new field: `gpus`

```rust
/// One entry per GPU reported by `sysinfo::Gpus`, in sysinfo's order.
/// Empty when no GPU is found or the platform is unsupported.
pub gpus: Vec<GpuInfo>,

#[derive(Debug, Clone)]
pub struct GpuInfo {
    /// `Gpu::model()`, e.g. "Apple M4 Pro"; "GPU" if sysinfo returns `None`.
    pub name: String,
    /// `Gpu::usage()` in percent. `None` when the driver doesn't expose it
    /// (some Linux GPUs; always `Some` on macOS).
    pub usage: Option<f32>,
    /// `Gpu::used_memory()` in bytes (dedicated VRAM). Always `None` on macOS,
    /// where the GPU shares system memory.
    pub mem_used: Option<u64>,
    /// `Gpu::total_memory()` in bytes. Always `None` on macOS.
    pub mem_total: Option<u64>,
}
```

A `Vec` rather than an `Option` because Linux machines commonly have both an
integrated and a discrete GPU, and `sysinfo` reports each one.

The GPU is embedded in `CpuSnapshot` rather than given its own `Action` variant and
component. The request is a *section of the CPU panel*, and embedding avoids a new
`ComponentId`, layout slot, config key, focus hotkey, and Tab-cycle entry. A standalone
GPU panel can be split out later if it ever grows a graph of its own.

**Data source: `sysinfo::Gpus`.** What it reads per platform:

| Platform | Source | `model` | `usage` | `used/total_memory` |
|---|---|---|---|---|
| macOS | IOKit `IOAccelerator` → `PerformanceStatistics` ("Device Utilization %") | yes | yes | always `None` |
| Linux, AMD (and others exposing it) | `/sys/class/drm/card*/device/` (`gpu_busy_percent`, `mem_info_vram_*`) | yes | yes | yes |
| Linux, NVIDIA | NVML (`libnvidia-ml.so`, `dlopen`ed) | yes | yes | yes |
| Linux, fallback | Vulkan (`dlopen`ed) when sysfs/NVML don't cover a device | yes | driver-dependent | driver-dependent |

Verified on an M4 Pro Mac mini: `Gpus::new_with_refreshed_list()` returns one entry
(`model = "Apple M4 Pro"`, `vendor = "Apple Inc."`, `usage = Some(6.0)`, memory `None`);
a refresh takes under 1 ms. Readable without root or entitlements.

**Dropped from the 2026-09-15 design** (they came from hand-reading `IOAccelerator`,
which `sysinfo` doesn't expose): GPU core count, renderer/tiler utilization, and the
macOS "In use / Alloc system memory" figures. See Accepted Gaps.

**Not chosen (for now): IOReport.** btop 1.4.7 uses the undocumented `libIOReport.dylib`
("GPU Stats" residency channels + "GPU Energy") — that is how it shows GPU watts. It
has no Rust crate, needs ~60 lines of hand-written FFI against an unversioned API, is
Apple-Silicon-only, and gives no memory figures. Power and frequency are deferred to a
follow-on that adds IOReport if wanted. GPU temperature is not available on Apple
Silicon by any route (btop shows 0°C).

**Per-process GPU stays out of scope — and must be switched off.** With the `gpu`
feature on, `ProcessRefreshKind::everything()` (and therefore `sys.refresh_all()`)
also collects `Process::gpu_usage`/`gpu_memory`, which on Linux walks
`/proc/<pid>/fd` for every process on every fast tick. Replace `sys.refresh_all()` in
the collector with an explicit refresh that excludes it:

```rust
sys.refresh_specifics(
    RefreshKind::everything().with_processes(
        ProcessRefreshKind::everything()
            .without_gpu_usage()
            .without_gpu_memory(),
    ),
);
```

A per-process GPU column (sysinfo supports usage on macOS and Linux, memory on
Linux) is a possible follow-on and would revisit this.

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

read_cpu_temps(smc: &SmcConnection, ncpu: usize) -> (Option<f32>, Vec<Option<f32>>)
    IOServiceOpen(AppleSMC) once at collector startup → SmcConnection (held in
    the collector task alongside prev_cpu_total; None if open fails).
    IOConnectCallStructMethod(kSMCReadKey) for "TC0P" → package_temp.
    Per-core keys probed once at startup (which of Tp0?/Tc0? exist), then read
    each tick → per_core_temp. Missing keys → None for that slot.
    Returns (None, vec![None; ncpu]) if the SMC connection is unavailable.

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

**GPU collection is cfg-free and lives in `mod.rs`, not `macos.rs`:**

```
build_gpus(gpus: &Gpus) -> Vec<GpuInfo>
    Map each sysinfo Gpu to GpuInfo (model → name, usage, used/total_memory).
```

The collector owns a `Gpus` next to `Networks`/`Disks`/`Components`. Because `Gpus`
is not `Send` on Linux (it holds `dlopen`ed NVML/Vulkan handles), the collector
moved from a `tokio::spawn`ed task to a dedicated `stats-collector` OS thread running
a current-thread Tokio runtime. It is created
with `Gpus::new_with_refreshed_list()`; unlike those, a failure here is not fatal —
log at `warn!` and keep `None`, so the collector runs and `gpus` stays empty. On each
fast tick call `gpus.refresh(false)`, then set `cpu_snap.gpus = build_gpus(&gpus)`
after the per-platform `build_cpu*` call (so the three `build_cpu*` variants don't each
need the parameter).

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

**Temperature.** The package and per-core temperature rendering is currently
`#[cfg(target_os = "linux")]`. Expand to `#[cfg(any(target_os = "linux", target_os = "macos"))]`.
The rendering already handles `None` per-core slots, so Macs without per-core SMC
keys show only the package value. The governor row stays Linux-only.
`physical_core_count` rendering is already guarded by `if let Some(phys)` — it will
display automatically once macOS populates the field.

**GPU rows.** Render one row per entry in `snap.gpus` below the per-core block,
modelled on btop's CPU-box GPU line:

```
macOS: GPU  Apple M4 Pro      [████░░░░░░░░░░░░]   7%
Linux: GPU  RTX 4070          [██░░░░░░░░░░░░░░]  12%   1.2 GB / 12.0 GB
```

Fields: name, a gauge of `usage`, the percentage, and `mem_used / mem_total` when both
are `Some`. When `usage` is `None` the gauge and percentage render as `n/a`. The same
row is used in compact and fullscreen views.

*Deferred:* GPU usage history on a chart. The CPU panel has no aggregate graph to
add a series to — its chart is one line per core, with legend rows tied to cores —
so this needs its own design (e.g. a GPU line in the fullscreen view).

`preferred_height` grows by `gpus.len()`. The rows are cfg-free — they key on the
`Vec` — so platform differences come only from what `sysinfo` reports.

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
        gpus: vec![],
        governor: None,              // was Some("powersave")
        cpu_modes: None,             // was Some(CpuModes { … })
    }
}
```

Add a `stub_linux()` constructor that restores the Linux-specific values, used by
tests that specifically exercise Linux rendering paths (temperature column, governor
row). Add a `stub_macos()` constructor with `package_temp: Some(39.0)`,
`per_core_temp` populated and `governor: None`, used by the macOS temperature tests.
Since temperature rendering is now shared, the temperature-column test moves from
Linux-only to `#[cfg(any(linux, macos))]` using the platform-specific stub.

The base `stub()` gets `gpus: vec![]`. Because the GPU rows are cfg-free, the GPU
snapshot tests are **not** platform-gated: `cpu_with_gpu` and `cpu_fullscreen_gpu`
build a stub with two GPUs — one macOS-style (`"Apple M4 Pro"`, `usage: Some(7.0)`,
memory `None`) and one Linux-style (`"RTX 4070"`, `usage: Some(12.0)`, memory
`Some`) — plus a third with `usage: None` to cover the `n/a` path.

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
| CPU scaling governor | Not applicable on macOS |
| GPU memory on macOS | `sysinfo` returns `None` (unified memory). IOKit's "In use / Alloc system memory" could be read directly as a follow-on |
| GPU core count, renderer/tiler utilization | Not exposed by `sysinfo`; would need direct `IOAccelerator` reads (macOS) |
| GPU power / frequency | Needs IOReport (undocumented, Apple Silicon only); deferred to a follow-on |
| GPU temperature | Not exposed by SMC or IOReport on Apple Silicon (btop shows 0°C too) |
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
9. **CPU temperature** — add `objc2-io-kit`/`objc2-core-foundation` deps; implement
   `SmcConnection` + `read_cpu_temps()`; expand temperature cfg guards in `cpu.rs`;
   add `stub_macos()` and move the temperature snapshot test to `any(linux, macos)`.
10. **GPU section** — **done first:** bump `sysinfo` to 0.40 and MSRV to 1.95
    (CI, `README.md`, `BUILDING.md`), adapting to its `Result`-returning
    constructors/getters, `Cpu::usage`, and `OsString` interface names. Then: enable
    the `gpu` feature; replace `refresh_all()` with the GPU-free `refresh_specifics`
    above; add `GpuInfo` + `CpuSnapshot::gpus` and `build_gpus()`; own a `Gpus` in the
    collector; add the GPU rows, chart series, and `preferred_height` bump in
    `cpu.rs`; add the cross-platform `cpu_with_gpu` / `cpu_fullscreen_gpu` snapshots.
    Independent of step 9 (no shared IOKit code any more).
11. **Docs update** — revise `USER_GUIDE.md` (CPU panel: temperature on macOS, GPU
    rows on macOS and Linux), `ARCHITECTURE.md` (macOS data sources incl. SMC;
    GPU via `sysinfo::Gpus`), and the platform-guard note in `AGENTS.md`; update
    `Cargo.toml` description/keywords.
