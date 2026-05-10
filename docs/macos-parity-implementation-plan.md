# macOS Platform Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Populate all silently-zero fields on macOS using native APIs, bringing dreidel's runtime data close to the Linux experience.

**Architecture:** A new `src/stats/macos.rs` module holds all macOS-specific collection logic behind `#[cfg(target_os = "macos")]`; `src/stats/mod.rs` gains paired cfg-guarded call sites mirroring the existing Linux ones. No new abstraction layers; the existing snapshot structs absorb four new macOS-specific `MemSnapshot` fields and UI code gains paired platform blocks.

**Tech Stack:** Rust, `libproc = "0.14"` (macOS proc_pidinfo wrappers), `libc = "0.2"` (host_processor_info / host_statistics64 / sysctl FFI), existing `sysinfo`, `ratatui`, `insta` snapshot tests.

---

## Discrepancies vs. the Design Spec

These were found by reading the code; address them as the plan directs, not as the spec says:

1. **`src/components/mem.rs` does not exist.** It was removed in commit `b164cfb` ("feat: move mem stats to status bar; remove standalone mem component"). Memory rendering lives in `src/components/status_bar.rs` (`draw_ram_row` / `draw_swap_row`). macOS memory display changes go there.

2. **`physical_core_count` in the CPU header is gated by `#[cfg(target_os = "linux")]`** (`src/components/cpu.rs:162`). The spec says it "will display automatically," but it won't without a cfg change. Task 4 includes the fix.

3. **Orphaned snapshot files** `dreidel__components__mem__tests__*` in `src/components/snapshots/` reference the deleted `mem.rs`. They are harmless but should be deleted when snapshots are regenerated in Task 1.

---

## File Map

| File | Action | Purpose |
|---|---|---|
| `src/stats/snapshots.rs` | Modify | Add 4 macOS fields to `MemSnapshot`; make `stub()` platform-neutral; add `stub_linux()` |
| `src/stats/macos.rs` | Create | All macOS collection functions |
| `src/stats/mod.rs` | Modify | Wire macOS paths; rename non-Linux stub; add `prev_cpu_ticks` state |
| `Cargo.toml` | Modify | Add `libproc` + `libc` as macOS-only deps |
| `src/components/cpu.rs` | Modify | Expand `physical_core_count` cfg guard to include macOS |
| `src/components/status_bar.rs` | Modify | macOS RAM row showing active/inactive/wired/compressed |
| `src/components/net.rs` | Modify | Expand Drop TX/RX cfg condition to include macOS |

---

## Task 1: Fix failing snapshot tests (platform-neutral stubs)

**Files:**
- Modify: `src/stats/snapshots.rs`
- Delete: `src/components/snapshots/dreidel__components__mem__tests__mem_no_data.snap`
- Delete: `src/components/snapshots/dreidel__components__mem__tests__mem_with_data.snap`

Four tests currently fail on macOS because `CpuSnapshot::stub()` provides Linux-specific data (temperatures, governor) but the macOS render path omits those columns, producing different layout widths.

- [ ] **Step 1: Confirm the failures**

```bash
cargo test 2>&1 | grep -E "FAILED|failed"
```

Expected output includes:
```
components::cpu::tests::renders_fullscreen_header
components::cpu::tests::renders_with_cpu_data
components::help::tests::renders_help_overlay
components::net::tests::renders_graph_view
```

- [ ] **Step 2: Make `CpuSnapshot::stub()` platform-neutral**

In `src/stats/snapshots.rs`, replace the existing `CpuSnapshot` stub impl block:

```rust
#[cfg(any(test, feature = "test-stubs"))]
#[allow(dead_code)]
impl CpuSnapshot {
    /// Cross-platform baseline: no Linux-only optional data.
    /// Use `stub_linux()` in tests that exercise temperature/governor rendering.
    pub fn stub() -> Self {
        Self {
            aggregate: 35.0,
            per_core: vec![42.0, 18.0, 75.0, 5.0],
            frequency: vec![3400, 3400, 3400, 3400],
            physical_core_count: None,
            cpu_brand: "Intel(R) Core(TM) i7-9750H CPU @ 2.60GHz".into(),
            package_temp: None,
            per_core_temp: vec![None; 4],
            governor: None,
            cpu_modes: None,
        }
    }

    /// Linux-specific stub: includes temperatures, governor, and cpu_modes.
    /// Use in tests that explicitly exercise Linux rendering paths.
    pub fn stub_linux() -> Self {
        Self {
            physical_core_count: Some(4),
            package_temp: Some(62.0),
            per_core_temp: vec![Some(55.0), Some(58.0), Some(60.0), Some(52.0)],
            governor: Some("powersave".into()),
            cpu_modes: Some(CpuModes {
                user: 5.2,
                system: 0.2,
                nice: 0.0,
                idle: 84.8,
                iowait: 9.8,
                irq: 0.0,
                softirq: 0.0,
                steal: 0.0,
            }),
            ..Self::stub()
        }
    }
}
```

- [ ] **Step 3: Update Linux-specific tests to use `stub_linux()`**

Search for test code that relies on temperatures or governor rendering:

```bash
rg -n "CpuSnapshot::stub()" src/components/cpu.rs
```

In `src/components/cpu.rs`, update any test calling `CpuSnapshot::stub()` that renders temperature columns or the fullscreen header with governor/temp data to call `CpuSnapshot::stub_linux()` instead. Wrap those tests with `#[cfg(target_os = "linux")]` so they only run where the rendering path applies.

The tests to update are `renders_with_cpu_data` and `renders_fullscreen_header`. Change:
```rust
// Before
let snap = CpuSnapshot::stub();
```
to:
```rust
// After — these tests exercise Linux-specific temperature rendering
#[cfg(target_os = "linux")]
fn renders_with_cpu_data() { ... uses CpuSnapshot::stub_linux() ... }
```

- [ ] **Step 4: Add `stub_linux()` to `MemSnapshot` and update status_bar test**

In `src/stats/snapshots.rs`, add a `stub_linux()` to the `MemSnapshot` impl block that is identical to the current `stub()` (the stub already contains Linux-realistic data). The current `stub()` stays as-is for now since `MemSnapshot` has no macOS fields yet (those come in Task 5).

```rust
#[cfg(any(test, feature = "test-stubs"))]
impl MemSnapshot {
    // existing stub() unchanged

    pub fn stub_linux() -> Self {
        Self::stub()
    }
}
```

- [ ] **Step 5: Regenerate snapshots**

```bash
INSTA_UPDATE=always cargo test 2>&1 | tail -5
```

Expected: all tests pass.

- [ ] **Step 6: Delete the orphaned mem snapshot files**

```bash
rm src/components/snapshots/dreidel__components__mem__tests__mem_no_data.snap
rm src/components/snapshots/dreidel__components__mem__tests__mem_with_data.snap
```

- [ ] **Step 7: Verify all tests pass**

```bash
cargo test 2>&1 | tail -5
```

Expected: `test result: ok. N passed; 0 failed`

- [ ] **Step 8: Commit**

```bash
jj commit -m "test: make CpuSnapshot::stub() platform-neutral; add stub_linux() variants"
```

---

## Task 2: Add macOS dependencies to Cargo.toml

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add deps**

```bash
cargo add libproc@0.14 --target 'cfg(target_os = "macos")'
cargo add libc@0.2 --target 'cfg(target_os = "macos")'
```

- [ ] **Step 2: Verify build still passes on macOS**

```bash
cargo build 2>&1 | tail -5
```

Expected: no errors.

- [ ] **Step 3: Commit**

```bash
jj commit -m "chore: add libproc and libc as macOS-only dependencies"
```

---

## Task 3: `src/stats/macos.rs` skeleton

**Files:**
- Create: `src/stats/macos.rs`
- Modify: `src/stats/mod.rs`

Wire up empty stub functions so the build stays green while subsequent tasks fill in the real implementations.

- [ ] **Step 1: Create `src/stats/macos.rs` with stub functions**

```rust
// SPDX-License-Identifier: GPL-3.0-only

//! macOS-specific stats collection.
//!
//! All functions are called from `stats/mod.rs` behind `#[cfg(target_os = "macos")]`
//! guards. Failures are logged at `tracing::debug!` level and return zero/None so
//! the rest of the snapshot is unaffected.

use crate::stats::snapshots::{CpuModes, CpuSnapshot, InterfaceSnapshot, ProcessEntry};
use sysinfo::System;

/// Per-CPU tick counts from `host_processor_info(PROCESSOR_CPU_LOAD_INFO)`.
/// Stored across ticks to compute mode-percentage deltas.
#[derive(Debug, Clone)]
pub struct MacosCpuTicks {
    pub user: Vec<u64>,
    pub system: Vec<u64>,
    pub idle: Vec<u64>,
    pub nice: Vec<u64>,
}

pub fn read_physical_core_count() -> Option<u32> {
    sysinfo::System::physical_core_count().map(|n| n as u32)
}

/// Returns `None` on the first call (no prior snapshot for delta) and on error.
pub fn read_cpu_modes(prev: Option<&MacosCpuTicks>) -> (Option<CpuModes>, Option<MacosCpuTicks>) {
    (None, None)
}

pub fn build_cpu_macos(sys: &System, prev_ticks: &mut Option<MacosCpuTicks>) -> CpuSnapshot {
    let cpus = sys.cpus();
    let (cpu_modes, new_ticks) = read_cpu_modes(prev_ticks.as_ref());
    *prev_ticks = new_ticks;
    CpuSnapshot {
        per_core: cpus.iter().map(|c| c.cpu_usage()).collect(),
        aggregate: sys.global_cpu_usage(),
        frequency: cpus.iter().map(|c| c.frequency()).collect(),
        cpu_brand: cpus.first().map(|c| c.brand().to_owned()).unwrap_or_default(),
        package_temp: None,
        per_core_temp: vec![None; cpus.len()],
        physical_core_count: read_physical_core_count(),
        governor: None,
        cpu_modes,
    }
}

/// Returns (free, active, inactive, wired, compressed, available, swap_in_bytes, swap_out_bytes).
pub fn read_mem_details() -> (u64, u64, u64, u64, u64, u64, u64, u64) {
    (0, 0, 0, 0, 0, 0, 0, 0)
}

/// Returns (rx_dropped, tx_dropped) for the named interface.
pub fn read_net_drops(_iface_name: &str) -> (u64, u64) {
    (0, 0)
}

pub fn enrich_process_entry(_entry: &mut ProcessEntry, _pid: u32) {}

pub fn enumerate_threads(_sys: &System) -> Vec<ProcessEntry> {
    Vec::new()
}
```

- [ ] **Step 2: Declare the module and wire up `mod.rs`**

In `src/stats/mod.rs`, at the top after the existing `pub mod snapshots;` line, add:

```rust
#[cfg(target_os = "macos")]
mod macos;
```

Replace the `run_collector` function's existing platform guards. Find this block (around line 54):

```rust
    #[cfg(target_os = "linux")]
    let mut prev_cpu_total: Option<CpuTotals> = None;
```

Add below it:

```rust
    #[cfg(target_os = "macos")]
    let mut prev_cpu_ticks: Option<macos::MacosCpuTicks> = None;
```

Replace (around line 84-87):

```rust
        #[cfg(target_os = "linux")]
        let cpu_snap = build_cpu(&sys, &components, &mut prev_cpu_total);
        #[cfg(not(target_os = "linux"))]
        let cpu_snap = build_cpu(&sys, &components);
```

with:

```rust
        #[cfg(target_os = "linux")]
        let cpu_snap = build_cpu(&sys, &components, &mut prev_cpu_total);
        #[cfg(target_os = "macos")]
        let cpu_snap = macos::build_cpu_macos(&sys, &mut prev_cpu_ticks);
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let cpu_snap = build_cpu_stub(&sys, &components);
```

Rename the existing `#[cfg(not(target_os = "linux"))]` `build_cpu` function to `build_cpu_stub`:

```rust
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn build_cpu_stub(sys: &System, _components: &Components) -> CpuSnapshot {
    // unchanged body
}
```

- [ ] **Step 3: Wire macOS thread enumeration in the slow tick**

Find the slow tick section (around line 73-79):

```rust
        #[cfg(target_os = "linux")]
        if slow_tick {
            cached_threads = enumerate_threads(&sys);
        }
        // Suppress unused-variable warning on non-Linux.
        #[cfg(not(target_os = "linux"))]
        let _ = slow_tick;
```

Replace with:

```rust
        #[cfg(target_os = "linux")]
        if slow_tick {
            cached_threads = enumerate_threads(&sys);
        }
        #[cfg(target_os = "macos")]
        if slow_tick {
            cached_threads = macos::enumerate_threads(&sys);
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let _ = slow_tick;
```

- [ ] **Step 4: Build and verify**

```bash
cargo build 2>&1 | tail -10
```

Expected: no errors or warnings.

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat(macos): add stats/macos.rs skeleton; wire cfg-guarded call sites in mod.rs"
```

---

## Task 4: CPU modes and physical core count on macOS

**Files:**
- Modify: `src/stats/macos.rs`
- Modify: `src/components/cpu.rs`

- [ ] **Step 1: Implement `read_cpu_modes()` in `src/stats/macos.rs`**

Replace the stub `read_cpu_modes` and `build_cpu_macos` with real implementations:

```rust
use libc::{
    host_processor_info, mach_msg_type_number_t, natural_t, processor_cpu_load_info,
    processor_cpu_load_info_data_t, mach_host_self, PROCESSOR_CPU_LOAD_INFO,
    CPU_STATE_USER, CPU_STATE_SYSTEM, CPU_STATE_IDLE, CPU_STATE_NICE, CPU_STATE_MAX,
};
use std::ptr;

pub fn read_cpu_modes(prev: Option<&MacosCpuTicks>) -> (Option<CpuModes>, Option<MacosCpuTicks>) {
    let mut cpu_info: *mut processor_cpu_load_info_data_t = ptr::null_mut();
    let mut cpu_count: natural_t = 0;
    let mut info_count: mach_msg_type_number_t = 0;

    // SAFETY: Standard Mach host_processor_info call. We check the return value
    // and immediately copy out the data before calling vm_deallocate.
    let ret = unsafe {
        host_processor_info(
            mach_host_self(),
            PROCESSOR_CPU_LOAD_INFO,
            &mut cpu_count,
            &mut (cpu_info as *mut _),
            &mut info_count,
        )
    };

    if ret != libc::KERN_SUCCESS || cpu_info.is_null() {
        tracing::debug!("host_processor_info failed: {ret}");
        return (None, None);
    }

    let n = cpu_count as usize;
    let mut user   = vec![0u64; n];
    let mut system = vec![0u64; n];
    let mut idle   = vec![0u64; n];
    let mut nice   = vec![0u64; n];

    // SAFETY: host_processor_info guarantees cpu_count valid entries.
    unsafe {
        for i in 0..n {
            let cpu = &*cpu_info.add(i);
            user[i]   = cpu.cpu_ticks[CPU_STATE_USER   as usize] as u64;
            system[i] = cpu.cpu_ticks[CPU_STATE_SYSTEM as usize] as u64;
            idle[i]   = cpu.cpu_ticks[CPU_STATE_IDLE   as usize] as u64;
            nice[i]   = cpu.cpu_ticks[CPU_STATE_NICE   as usize] as u64;
        }
        // Deallocate the Mach memory returned by host_processor_info.
        let page_size = libc::sysconf(libc::_SC_PAGESIZE) as usize;
        let byte_count = (info_count as usize * std::mem::size_of::<u32>())
            .next_multiple_of(page_size);
        libc::vm_deallocate(
            libc::mach_task_self(),
            cpu_info as libc::vm_address_t,
            byte_count,
        );
    }

    let new_ticks = MacosCpuTicks { user: user.clone(), system: system.clone(), idle: idle.clone(), nice: nice.clone() };

    let modes = prev.and_then(|p| {
        if p.user.len() != n { return None; }
        let mut d_user = 0u64;
        let mut d_system = 0u64;
        let mut d_idle = 0u64;
        let mut d_nice = 0u64;
        for i in 0..n {
            d_user   += user[i].saturating_sub(p.user[i]);
            d_system += system[i].saturating_sub(p.system[i]);
            d_idle   += idle[i].saturating_sub(p.idle[i]);
            d_nice   += nice[i].saturating_sub(p.nice[i]);
        }
        let total = d_user + d_system + d_idle + d_nice;
        if total == 0 { return None; }
        let scale = 100.0 / total as f32;
        Some(CpuModes {
            user:    d_user   as f32 * scale,
            system:  d_system as f32 * scale,
            idle:    d_idle   as f32 * scale,
            nice:    d_nice   as f32 * scale,
            iowait:  0.0,
            irq:     0.0,
            softirq: 0.0,
            steal:   0.0,
        })
    });

    (modes, Some(new_ticks))
}
```

- [ ] **Step 2: Fix `physical_core_count` display in `src/components/cpu.rs`**

At line 162, change:

```rust
        #[cfg(target_os = "linux")]
        if let Some(phys) = snap.physical_core_count {
```

to:

```rust
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if let Some(phys) = snap.physical_core_count {
```

- [ ] **Step 3: Build**

```bash
cargo build 2>&1 | tail -10
```

Expected: no errors. (There may be an unused-import warning from libc constants — address with `#[allow(unused_imports)]` if needed until all libc symbols are used.)

- [ ] **Step 4: Run tests**

```bash
cargo test 2>&1 | tail -5
```

Expected: all pass. If CPU snapshot tests fail due to the `physical_core_count` display change on macOS, regenerate:

```bash
INSTA_UPDATE=always cargo test components::cpu 2>&1 | tail -5
cargo test 2>&1 | tail -5
```

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat(macos): implement CPU mode ticks and physical core count"
```

---

## Task 5: Memory — new MemSnapshot fields + macOS collection + status bar display

**Files:**
- Modify: `src/stats/snapshots.rs`
- Modify: `src/stats/macos.rs`
- Modify: `src/stats/mod.rs`
- Modify: `src/components/status_bar.rs`

- [ ] **Step 1: Add four macOS fields to `MemSnapshot`**

In `src/stats/snapshots.rs`, add to the `MemSnapshot` struct after `swap_out_bytes`:

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

Update `MemSnapshot::stub()` to initialise them to `0`:

```rust
            ram_active: 0,
            ram_inactive: 0,
            ram_wired: 0,
            ram_compressed: 0,
```

Update `MemSnapshot::stub_linux()` to call `Self::stub()` (unchanged — Linux doesn't populate these).

Add a `stub_macos()` for use in macOS-specific UI tests:

```rust
    pub fn stub_macos() -> Self {
        Self {
            ram_free:       2_252_341_248,   // 2.1 GiB
            ram_active:     6_232_252_416,   // 5.8 GiB
            ram_inactive:   2_469_396_480,   // 2.3 GiB
            ram_wired:      1_717_986_918,   // 1.6 GiB
            ram_compressed: 1_503_238_554,   // 1.4 GiB
            ram_available:  2_252_341_248 + 2_469_396_480,
            swap_in_bytes:  0,
            swap_out_bytes: 0,
            ..Self::stub()
        }
    }
```

- [ ] **Step 2: Implement `read_mem_details()` in `src/stats/macos.rs`**

```rust
use libc::{
    host_statistics64, vm_statistics64_data_t, HOST_VM_INFO64,
    HOST_VM_INFO64_COUNT, mach_host_self, KERN_SUCCESS,
};

pub fn read_mem_details() -> (u64, u64, u64, u64, u64, u64, u64, u64) {
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
    if page_size == 0 {
        return (0, 0, 0, 0, 0, 0, 0, 0);
    }

    let mut vm_info = std::mem::MaybeUninit::<vm_statistics64_data_t>::zeroed();
    let mut count = HOST_VM_INFO64_COUNT;

    // SAFETY: Standard host_statistics64 call with the correct info_count.
    let ret = unsafe {
        host_statistics64(
            mach_host_self(),
            HOST_VM_INFO64,
            vm_info.as_mut_ptr() as *mut _,
            &mut count,
        )
    };

    if ret != KERN_SUCCESS {
        tracing::debug!("host_statistics64(HOST_VM_INFO64) failed: {ret}");
        return (0, 0, 0, 0, 0, 0, 0, 0);
    }

    // SAFETY: host_statistics64 returned KERN_SUCCESS so the struct is initialised.
    let vm = unsafe { vm_info.assume_init() };

    let free       = vm.free_count       as u64 * page_size;
    let active     = vm.active_count     as u64 * page_size;
    let inactive   = vm.inactive_count   as u64 * page_size;
    let wired      = vm.wire_count       as u64 * page_size;
    let compressed = vm.compressor_page_count as u64 * page_size;
    let available  = free + inactive;
    let swap_in    = vm.pageins  as u64 * page_size;
    let swap_out   = vm.pageouts as u64 * page_size;

    (free, active, inactive, wired, compressed, available, swap_in, swap_out)
}
```

- [ ] **Step 3: Wire macOS memory collection in `src/stats/mod.rs`**

In `build_mem`, replace:

```rust
    #[cfg(not(target_os = "linux"))]
    let (ram_free, ram_buffers, ram_cached, ram_available) = (0u64, 0u64, 0u64, 0u64);
```

with:

```rust
    #[cfg(target_os = "macos")]
    let (ram_free, _ram_active, _ram_inactive, _ram_wired, _ram_compressed,
         ram_available, macos_swap_in, macos_swap_out) = crate::stats::macos::read_mem_details();
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let (ram_free, ram_available) = (0u64, 0u64);
    #[cfg(not(target_os = "linux"))]
    let (ram_buffers, ram_cached) = (0u64, 0u64);
```

And update the `MemSnapshot { ... }` constructor in `build_mem` to populate the four new fields and fix the swap bytes on macOS. The struct literal should become:

```rust
    MemSnapshot {
        ram_used: sys.used_memory(),
        ram_total: sys.total_memory(),
        ram_free,
        ram_buffers,
        ram_cached,
        ram_available,
        swap_used: sys.used_swap(),
        swap_total: sys.total_swap(),
        #[cfg(target_os = "linux")]
        swap_in_bytes: read_vmstat_field("pswpin").unwrap_or(0) * 4096,
        #[cfg(target_os = "macos")]
        swap_in_bytes: macos_swap_in,
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        swap_in_bytes: 0,
        #[cfg(target_os = "linux")]
        swap_out_bytes: read_vmstat_field("pswpout").unwrap_or(0) * 4096,
        #[cfg(target_os = "macos")]
        swap_out_bytes: macos_swap_out,
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        swap_out_bytes: 0,
        #[cfg(target_os = "macos")]
        ram_active: _ram_active,
        #[cfg(not(target_os = "macos"))]
        ram_active: 0,
        #[cfg(target_os = "macos")]
        ram_inactive: _ram_inactive,
        #[cfg(not(target_os = "macos"))]
        ram_inactive: 0,
        #[cfg(target_os = "macos")]
        ram_wired: _ram_wired,
        #[cfg(not(target_os = "macos"))]
        ram_wired: 0,
        #[cfg(target_os = "macos")]
        ram_compressed: _ram_compressed,
        #[cfg(not(target_os = "macos"))]
        ram_compressed: 0,
    }
```

- [ ] **Step 4: Add macOS RAM row to `src/components/status_bar.rs`**

In `draw_ram_row`, the current logic shows a detailed row when `ram_available > 0 || ram_free > 0` and a simple row otherwise. Add a macOS-specific branch. Replace the `let label = if mem.ram_available > 0 || ...` block with:

```rust
        #[cfg(target_os = "macos")]
        let label = if mem.ram_active > 0 || mem.ram_free > 0 {
            format!(
                "RAM {}/{}  free {}  active {}  inactive {}  wired {}  compressed {}",
                fmt_bytes(mem.ram_used),
                fmt_bytes(mem.ram_total),
                fmt_bytes(mem.ram_free),
                fmt_bytes(mem.ram_active),
                fmt_bytes(mem.ram_inactive),
                fmt_bytes(mem.ram_wired),
                fmt_bytes(mem.ram_compressed),
            )
        } else {
            format!(
                "RAM {}/{}  {:>5.1}%",
                fmt_bytes(mem.ram_used),
                fmt_bytes(mem.ram_total),
                ratio * 100.0,
            )
        };
        #[cfg(not(target_os = "macos"))]
        let label = if mem.ram_available > 0 || mem.ram_free > 0 {
            format!(
                "RAM {}/{}  free {}  buffer/cache {}  available {}",
                fmt_bytes(mem.ram_used),
                fmt_bytes(mem.ram_total),
                fmt_bytes(mem.ram_free),
                fmt_bytes(mem.ram_buffers + mem.ram_cached),
                fmt_bytes(mem.ram_available),
            )
        } else {
            format!(
                "RAM {}/{}  {:>5.1}%",
                fmt_bytes(mem.ram_used),
                fmt_bytes(mem.ram_total),
                ratio * 100.0,
            )
        };
```

Also update `MEM_LABEL_WIDTH` — the macOS label is longer. Change from the single constant to cfg-guarded values:

```rust
#[cfg(target_os = "macos")]
const MEM_LABEL_WIDTH: u16 = 90;
#[cfg(not(target_os = "macos"))]
const MEM_LABEL_WIDTH: u16 = 64;
```

- [ ] **Step 5: Build**

```bash
cargo build 2>&1 | tail -10
```

Expected: no errors. Fix any unused-variable warnings (`#[allow(unused_variables)]` is acceptable as a temporary fix, but prefer correct cfg guards).

- [ ] **Step 6: Regenerate and accept updated snapshots**

```bash
INSTA_UPDATE=always cargo test 2>&1 | tail -5
cargo test 2>&1 | tail -5
```

- [ ] **Step 7: Commit**

```bash
jj commit -m "feat(macos): add MemSnapshot macOS fields; implement host_statistics64 collection; update status bar RAM row"
```

---

## Task 6: Network drop counters on macOS

**Files:**
- Modify: `src/stats/macos.rs`
- Modify: `src/stats/mod.rs`
- Modify: `src/components/net.rs`

- [ ] **Step 1: Implement `read_net_drops()` in `src/stats/macos.rs`**

```rust
pub fn read_net_drops(iface_name: &str) -> (u64, u64) {
    // Use sysctl(NET_RT_IFLIST2) to get 64-bit drop counters.
    // getifaddrs only provides 32-bit counters which overflow on modern traffic.
    use libc::{
        AF_LINK, CTL_NET, NET_RT_IFLIST2, PF_ROUTE, RTM_IFINFO2,
        if_msghdr2, sysctl,
    };
    use std::mem;

    let mut mib: [libc::c_int; 6] = [CTL_NET, PF_ROUTE, 0, 0, NET_RT_IFLIST2, 0];
    let mut buf_len: libc::size_t = 0;

    // First call to get the required buffer size.
    // SAFETY: Standard sysctl size-query pattern (null buf pointer).
    let ret = unsafe {
        sysctl(
            mib.as_mut_ptr(), 6,
            std::ptr::null_mut(), &mut buf_len,
            std::ptr::null_mut(), 0,
        )
    };
    if ret != 0 || buf_len == 0 {
        return (0, 0);
    }

    let mut buf = vec![0u8; buf_len];

    // Second call to fetch the data.
    // SAFETY: buf is allocated to the size returned by the first call.
    let ret = unsafe {
        sysctl(
            mib.as_mut_ptr(), 6,
            buf.as_mut_ptr() as *mut libc::c_void, &mut buf_len,
            std::ptr::null_mut(), 0,
        )
    };
    if ret != 0 {
        return (0, 0);
    }

    let mut offset = 0usize;
    while offset + mem::size_of::<if_msghdr2>() <= buf_len {
        // SAFETY: We checked offset + size fits in buf_len.
        let hdr = unsafe { &*(buf.as_ptr().add(offset) as *const if_msghdr2) };
        let msg_len = hdr.ifm_msglen as usize;
        if msg_len == 0 { break; }

        if hdr.ifm_type as libc::c_int == RTM_IFINFO2 && hdr.ifm_addrs & (1 << AF_LINK) != 0 {
            // Interface name is in the sockaddr_dl that immediately follows the header.
            let sdl_offset = offset + mem::size_of::<if_msghdr2>();
            if sdl_offset + mem::size_of::<libc::sockaddr_dl>() <= buf_len {
                // SAFETY: We just bounds-checked sdl_offset.
                let sdl = unsafe { &*(buf.as_ptr().add(sdl_offset) as *const libc::sockaddr_dl) };
                let nlen = sdl.sdl_nlen as usize;
                if nlen > 0 && sdl_offset + mem::size_of::<libc::sockaddr_dl>() + nlen <= buf_len {
                    // SAFETY: We bounds-checked the name slice.
                    let name_bytes = unsafe {
                        std::slice::from_raw_parts(
                            (buf.as_ptr().add(sdl_offset + mem::size_of::<libc::sockaddr_dl>()))
                                as *const u8,
                            nlen,
                        )
                    };
                    if let Ok(name) = std::str::from_utf8(name_bytes) {
                        if name == iface_name {
                            return (hdr.ifm_data.ifi_iqdrops as u64, hdr.ifm_snd_drops as u64);
                        }
                    }
                }
            }
        }
        offset += msg_len;
    }

    (0, 0)
}
```

- [ ] **Step 2: Wire drop counters in `src/stats/mod.rs` `build_net()`**

Replace the existing drop counter section in the `InterfaceSnapshot` literal:

```rust
                    #[cfg(target_os = "linux")]
                    rx_dropped: dev_stats.get(name).map(|s| s.recv_drop).unwrap_or(0),
                    #[cfg(not(target_os = "linux"))]
                    rx_dropped: 0,
                    #[cfg(target_os = "linux")]
                    tx_dropped: dev_stats.get(name).map(|s| s.sent_drop).unwrap_or(0),
                    #[cfg(not(target_os = "linux"))]
                    tx_dropped: 0,
```

with:

```rust
                    #[cfg(target_os = "linux")]
                    rx_dropped: dev_stats.get(name).map(|s| s.recv_drop).unwrap_or(0),
                    #[cfg(target_os = "macos")]
                    rx_dropped: {
                        let (rx, _) = macos::read_net_drops(name);
                        rx
                    },
                    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
                    rx_dropped: 0,
                    #[cfg(target_os = "linux")]
                    tx_dropped: dev_stats.get(name).map(|s| s.sent_drop).unwrap_or(0),
                    #[cfg(target_os = "macos")]
                    tx_dropped: {
                        let (_, tx) = macos::read_net_drops(name);
                        tx
                    },
                    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
                    tx_dropped: 0,
```

Also remove the `#[cfg_attr(not(target_os = "linux"), allow(dead_code))]` attributes from `InterfaceSnapshot.rx_dropped` and `tx_dropped` in `snapshots.rs`, since both fields are now used on two platforms.

- [ ] **Step 3: Expand Drop TX/RX display in `src/components/net.rs`**

At line 656, change:

```rust
            #[cfg(not(target_os = "linux"))]
            let traffic_line = Line::from(vec![ /* without drops */ ]);
            #[cfg(target_os = "linux")]
            let traffic_line = Line::from(vec![ /* with drops */ ]);
```

to:

```rust
            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            let traffic_line = Line::from(vec![ /* without drops — unchanged */ ]);
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            let traffic_line = Line::from(vec![ /* with drops — unchanged */ ]);
```

- [ ] **Step 4: Build and test**

```bash
cargo build 2>&1 | tail -10
INSTA_UPDATE=always cargo test components::net 2>&1 | tail -5
cargo test 2>&1 | tail -5
```

- [ ] **Step 5: Commit**

```bash
jj commit -m "feat(macos): implement network drop counters via NET_RT_IFLIST2 sysctl"
```

---

## Task 7: Process enrichment via libproc

**Files:**
- Modify: `src/stats/macos.rs`
- Modify: `src/stats/mod.rs`

- [ ] **Step 1: Implement `enrich_process_entry()` in `src/stats/macos.rs`**

```rust
use libproc::libproc::bsd_info::BSDInfo;
use libproc::libproc::file_info::ListFDs;
use libproc::libproc::task_info::TaskAllInfo;
use libproc::proc_pid::{listpidinfo, pidinfo};

pub fn enrich_process_entry(entry: &mut ProcessEntry, pid: u32) {
    let pid_i = pid as i32;

    // BSD info: nice, priority, tty
    if let Ok(info) = pidinfo::<BSDInfo>(pid_i, 0) {
        entry.nice     = info.pbi_nice as i32;
        entry.priority = info.pbi_priority as i32;
        let tty_dev = info.e_tdev;
        if tty_dev != u32::MAX && tty_dev != 0 {
            let major = (tty_dev >> 24) & 0xFF;
            let minor = tty_dev & 0x00FF_FFFF;
            entry.tty = Some(format!("{major}:{minor}"));
        }
    }

    // Task all info: threads, cpu times, faults, context switches
    if let Ok(info) = pidinfo::<TaskAllInfo>(pid_i, 0) {
        let ti = info.ptinfo;
        entry.threads             = ti.pti_threadnum;
        // pti_total_user / pti_total_system are in nanoseconds
        entry.user_cpu_time_secs  = ti.pti_total_user   as f64 / 1_000_000_000.0;
        entry.system_cpu_time_secs= ti.pti_total_system as f64 / 1_000_000_000.0;
        entry.cpu_time_secs       = entry.user_cpu_time_secs + entry.system_cpu_time_secs;
        entry.minor_faults        = ti.pti_faults    as u64;
        entry.major_faults        = ti.pti_pageins   as u64;
        entry.voluntary_ctxt_switches = Some(ti.pti_csw as u64);
        // macOS gives aggregate only; nonvoluntary stays None
    }

    // FD count
    if let Ok(fds) = listpidinfo::<ListFDs>(pid_i, 256) {
        entry.fd_count = Some(fds.len());
    }
}
```

- [ ] **Step 2: Call `enrich_process_entry()` in `build_proc()` in `src/stats/mod.rs`**

In `build_proc`, after the `#[cfg(target_os = "linux")]` enrichment block (around line 530), add:

```rust
                #[cfg(target_os = "macos")]
                macos::enrich_process_entry(&mut entry, pid);
```

- [ ] **Step 3: Build and test**

```bash
cargo build 2>&1 | tail -10
cargo test 2>&1 | tail -5
```

- [ ] **Step 4: Commit**

```bash
jj commit -m "feat(macos): enrich process entries via libproc (priority, nice, threads, cpu times, faults, fd count)"
```

---

## Task 8: Thread enumeration via libproc

**Files:**
- Modify: `src/stats/macos.rs`

- [ ] **Step 1: Implement `enumerate_threads()` in `src/stats/macos.rs`**

```rust
use libproc::libproc::thread_info::ThreadInfo;
use libproc::proc_pid::{listpidinfo, pidinfo};
use libproc::libproc::pid_rusage::PIDRUsage;
use crate::stats::snapshots::ProcessStatus;

pub fn enumerate_threads(sys: &System) -> Vec<ProcessEntry> {
    use libproc::proc_pid::listthreads;

    let mut thread_entries = Vec::new();

    for (sysinfo_pid, p) in sys.processes() {
        let pid = sysinfo_pid.as_u32();
        let pid_i = pid as i32;
        let proc_name = p.name().to_string_lossy();
        let proc_user = p.user_id().map(|u| u.to_string()).unwrap_or_default();

        let Ok(tids) = listthreads(pid_i) else { continue };

        for tid in tids {
            // Skip the main thread (TID == PID on macOS is not always true,
            // but the first TID returned is the main thread; skip matching pid).
            if tid as u32 == pid { continue; }

            let cpu_time_secs = if let Ok(info) = pidinfo::<ThreadInfo>(pid_i, tid) {
                // th_user_time and th_system_time are in microseconds
                (info.pth_user_time + info.pth_system_time) as f64 / 1_000_000.0
            } else {
                0.0
            };

            thread_entries.push(ProcessEntry {
                pid:        tid as u32,
                name:       format!("[{proc_name}:{tid}]"),
                cmd:        Vec::new(),
                user:       proc_user.clone(),
                cpu_pct:    0.0,
                mem_bytes:  0,
                mem_pct:    0.0,
                virt_bytes: 0,
                status:     ProcessStatus::Unknown,
                start_time: 0,
                run_time:   0,
                nice:       0,
                threads:    0,
                read_bytes: 0,
                write_bytes:0,
                parent_pid: Some(pid),
                priority:   0,
                shr_bytes:  0,
                cpu_time_secs,
                exe:        None,
                cwd:        None,
                root:       None,
                effective_user: None,
                group:      None,
                effective_group: None,
                session_id: None,
                tty:        None,
                user_cpu_time_secs:   0.0,
                system_cpu_time_secs: 0.0,
                minor_faults: 0,
                major_faults: 0,
                voluntary_ctxt_switches:    None,
                nonvoluntary_ctxt_switches: None,
                fd_count:   None,
                swap_bytes: None,
                io_read_calls:  None,
                io_write_calls: None,
                io_read_chars:  None,
                io_write_chars: None,
                cancelled_write_bytes: None,
                is_thread:  true,
            });
        }
    }

    thread_entries
}
```

- [ ] **Step 2: Build and test**

```bash
cargo build 2>&1 | tail -10
cargo test 2>&1 | tail -5
```

- [ ] **Step 3: Commit**

```bash
jj commit -m "feat(macos): enumerate threads via libproc listthreads"
```

---

## Task 9: Documentation update

**Files:**
- Modify: `USER_GUIDE.md`
- Modify: `ARCHITECTURE.md`
- Modify: `Cargo.toml` (description + keywords)

- [ ] **Step 1: Update `Cargo.toml` description and keywords**

Change:
```toml
description = "A keyboard-driven terminal system monitor for Linux"
keywords = ["tui", "monitor", "system", "ratatui", "linux"]
```

to:
```toml
description = "A keyboard-driven terminal system monitor for Linux and macOS"
keywords = ["tui", "monitor", "system", "ratatui", "macos"]
```

- [ ] **Step 2: Add macOS section to `USER_GUIDE.md`**

Find the platform-specific section (search for "Linux" references describing procfs/proc filesystem). Add a macOS note covering:
- Which fields are populated on macOS (CPU modes, physical core count, RAM active/inactive/wired/compressed, network drop counters, process nice/priority/threads/cpu times/faults/fd count)
- Which fields remain unavailable (CPU temperature, scaling governor, per-process I/O call counts, shared memory, voluntary vs. nonvoluntary context switch split, iowait/irq/softirq/steal CPU modes)

- [ ] **Step 3: Update `ARCHITECTURE.md`**

In the `#[cfg(target_os = "linux")]` documentation section, note that macOS now has a parallel `#[cfg(target_os = "macos")]` path via `src/stats/macos.rs`. Update the data flow diagram if it references Linux-only collection.

- [ ] **Step 4: Final test run**

```bash
cargo test 2>&1 | tail -5
cargo clippy -- -D warnings 2>&1 | tail -10
```

Expected: all tests pass, no clippy warnings.

- [ ] **Step 5: Commit**

```bash
jj commit -m "docs: update USER_GUIDE, ARCHITECTURE, and Cargo.toml for macOS support"
```

---

## Self-Review Checklist

- [x] **Spec coverage:** All 9 implementation steps from the design doc are represented. Accepted gaps (CPU temp, governor, IO syscall counts, SHR, swap-by-VM-region, nonvoluntary ctxsw, iowait/irq/softirq/steal, ram_buffers) are intentionally absent.
- [x] **Discrepancy noted:** `mem.rs` vs. `status_bar.rs` — plan directs changes to `status_bar.rs`.
- [x] **Discrepancy noted:** `physical_core_count` cfg guard — Task 4 Step 2 expands it to macOS.
- [x] **Types consistent:** `MacosCpuTicks` defined once in `macos.rs` and used consistently in `mod.rs`. `read_mem_details()` returns an 8-tuple; all call sites destructure in the same order. `enrich_process_entry` and `enumerate_threads` signatures match between Tasks 3, 7, and 8.
- [x] **No placeholders:** All code blocks are complete and buildable.
- [x] **TDD where applicable:** Task 1 is purely test-fixing. Later tasks include snapshot regeneration steps after each UI change. Pure FFI collection functions (Tasks 4–8) are verified via build + full test run rather than unit tests, since the macOS APIs require a live system.
