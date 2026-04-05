# BSD Port Analysis

This document summarizes what would be required to add BSD support to
`dreidel`, with a focus on reuse: what can come directly from existing crates,
what would need feature gating, and what would likely require BSD-specific
implementation work.

The immediate target should be **FreeBSD first**. `sysinfo` explicitly supports
FreeBSD and NetBSD today, while DragonFlyBSD appears less directly supported by
the current Rust systems-information ecosystem.

## Executive Summary

`dreidel` is not locked to Linux, but it is currently **Linux-shaped** in its
collector and snapshot model.

The good news:

- Most of the baseline monitoring stack already uses `sysinfo`
- `sysinfo` explicitly supports `FreeBSD` and `NetBSD`
- A useful first BSD port can likely reuse `sysinfo` for CPU, memory, load,
  network, disks, and most process metadata
- The main supplemental crate worth using is `sysctl`

The main blockers are not in the UI. They are concentrated in
`src/stats/mod.rs` and a handful of Linux-only fields in snapshot types:

- `/proc` parsing through `procfs`
- thread enumeration via `/proc/<pid>/task/`
- Linux-only swap activity counters from `/proc/vmstat`
- Linux-only CPU governor logic from `/sys`
- Linux-specific per-core temperature mapping built from hwmon/sysfs semantics
- Linux-only network dropped-packet counters from `/proc/net/dev`

Practical conclusion:

- A **BSD baseline port** is realistic with substantial reuse
- A **Linux-parity BSD port** is a larger follow-up project

## External Reuse Inventory

### `sysinfo`

`sysinfo` is the main reusable foundation.

What `sysinfo` explicitly supports:

- Android
- FreeBSD
- NetBSD
- iOS
- Linux
- macOS
- Raspberry Pi
- Windows

What `sysinfo` already exposes that is relevant to `dreidel`:

- CPU usage and per-CPU stats
- CPU frequency
- system load average
- system uptime and hostname
- total/used memory and swap
- network interfaces and counters
- disks and disk usage counters
- processes, including:
  - pid
  - name
  - command line
  - user id
  - cpu usage
  - memory / virtual memory
  - parent pid
  - status
  - start time / runtime
  - disk usage
  - kill/signals
- hardware components / temperature sensors

This already covers most of `dreidel`'s baseline needs.

### `sysctl`

`sysctl` is the most useful BSD-oriented supplement.

Why it matters:

- It supports FreeBSD and macOS
- It exposes the BSD `sysctl` interface directly
- It can be used to fill gaps where `sysinfo` is too generic

Likely uses in a BSD port:

- host metadata if needed
- temperature or CPU/core labeling refinements
- memory/swap counters beyond what `sysinfo` exposes
- BSD-specific counters not represented in `sysinfo`

### What does not appear to exist as a strong drop-in solution

There does not appear to be a mainstream Rust equivalent of Linux `procfs` for
BSD process introspection that would replace custom work outright.

In practice, the likely stack is:

- `sysinfo` for almost everything
- `sysctl` for targeted BSD-only data
- small BSD-specific code only where necessary

## Current Dreidel Portability Map

This section maps `dreidel`'s current data model and UI expectations to likely
reuse paths.

### System Snapshot

Source in `dreidel`:

- `src/stats/mod.rs::build_sys`
- `src/stats/snapshots.rs::SysSnapshot`

Fields:

- `hostname`
- `uptime`
- `load_avg`
- `timestamp`

Current implementation source:

- `System::host_name()`
- `System::uptime()`
- `System::load_average()`
- local clock from `chrono`

Reuse verdict:

- Reusable via `sysinfo` as-is

BSD implementation risk:

- Low

### CPU Snapshot

Source in `dreidel`:

- `src/stats/mod.rs::build_cpu`
- `src/stats/snapshots.rs::CpuSnapshot`
- `src/components/cpu.rs`

Fields and reuse assessment:

1. `per_core`

- Current source: `sys.cpus().iter().map(|c| c.cpu_usage())`
- Reuse verdict: reusable via `sysinfo`
- Risk: low

2. `aggregate`

- Current source: `sys.global_cpu_usage()`
- Reuse verdict: reusable via `sysinfo`
- Risk: low

3. `frequency`

- Current source: `cpu.frequency()`
- Reuse verdict: likely reusable via `sysinfo`
- Risk: low to medium, depending on per-platform semantics

4. `cpu_brand`

- Current source: `cpu.brand()`
- Reuse verdict: reusable via `sysinfo`
- Risk: low

5. `package_temp`

- Current source: `sysinfo::Components`, but only enabled for Linux in
  `CpuSnapshot`
- Reuse verdict: probably reusable on FreeBSD at a baseline level because
  `sysinfo` components exist on FreeBSD too
- Risk: medium because label semantics will differ

6. `per_core_temp`

- Current source: Linux-specific mapping from hwmon labels plus
  `/sys/devices/system/cpu/cpuN/topology/core_id`
- Reuse verdict: not reusable as-is
- BSD path: likely needs redesign, or should degrade to generic component temps
- Risk: high

7. `physical_core_count`

- Current source: parse `/proc/cpuinfo`
- Reuse verdict: not reusable as-is
- BSD path: use `sysinfo::System::physical_core_count()` if acceptable, or use
  `sysctl` if a more exact BSD source is needed
- Risk: low to medium

8. `governor`

- Current source: `/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor`
- Reuse verdict: Linux-only
- BSD path: feature gate out for BSD
- Risk: not a blocker if treated as Linux-only

UI implications:

- `src/components/cpu.rs` already guards some display fields with
  `#[cfg(target_os = "linux")]`
- The CPU panel can work well on BSD with:
  - usage
  - frequency
  - brand
  - optional generic temperature support

Recommendation:

- Keep CPU usage/frequency/brand cross-platform
- Replace Linux-specific fields with capability-driven optional fields rather
  than hardcoding OS-specific snapshot shapes long-term

### Memory Snapshot

Source in `dreidel`:

- `src/stats/mod.rs::build_mem`
- `src/stats/snapshots.rs::MemSnapshot`
- `src/components/status_bar.rs`

Fields and reuse assessment:

1. `ram_used`
2. `ram_total`
3. `swap_used`
4. `swap_total`

- Current source: `sysinfo`
- Reuse verdict: reusable via `sysinfo`
- Risk: low

5. `swap_in_bytes`
6. `swap_out_bytes`

- Current source: `/proc/vmstat` via `read_vmstat_field("pswpin")` and
  `read_vmstat_field("pswpout")`
- Reuse verdict: not reusable as-is
- BSD path:
  - maybe via `sysctl`, if equivalent counters are exposed and worth using
  - otherwise omit on BSD
- Risk: medium, but not a blocker because the current status bar does not depend
  on these fields for core rendering

Recommendation:

- BSD baseline can ship with RAM/swap usage only
- treat swap-in/out as an optional advanced metric

### Network Snapshot

Source in `dreidel`:

- `src/stats/mod.rs::build_net`
- `src/stats/snapshots.rs::InterfaceSnapshot`
- `src/components/net.rs`

Fields and reuse assessment:

Reused directly from `sysinfo`:

- interface name
- RX/TX bytes
- RX/TX packets
- RX/TX errors
- total RX/TX bytes
- MAC address
- IP addresses
- MTU

Linux-only extras:

- `rx_dropped`
- `tx_dropped`

Current source for dropped counters:

- `procfs::net::dev_status()` from `/proc/net/dev`

Reuse verdict:

- baseline network panel is reusable via `sysinfo`
- dropped counters are not reusable as-is

BSD path:

- either omit dropped counters on BSD
- or backfill them later via `sysctl` or BSD-native interfaces if useful

Risk:

- low for a first port

### Disk Snapshot

Source in `dreidel`:

- `src/stats/mod.rs::build_disk`
- `src/stats/snapshots.rs::DiskSnapshot`
- `src/components/disk.rs`

Fields:

- read/write bytes per interval
- total read/write bytes
- usage percentage
- disk kind
- filesystem
- mount point
- removable/read-only flags
- total/available space

Current source:

- entirely from `sysinfo::Disks`

Reuse verdict:

- reusable via `sysinfo`

Risk:

- low to medium depending on exact per-platform semantics of per-interval disk
  counters

Notes:

- The current duplicate-device suppression by disk name is platform-agnostic and
  should continue to work, though it may need validation on BSD mount naming.

### Process Snapshot

Source in `dreidel`:

- `src/stats/mod.rs::build_proc`
- `src/stats/snapshots.rs::ProcessEntry`
- `src/components/process/mod.rs`
- `src/components/process/tree.rs`

This is where the split between reusable and Linux-specific fields matters most.

#### Process fields likely reusable via `sysinfo`

- `pid`
- `name`
- `cmd`
- `user`
- `cpu_pct`
- `mem_bytes`
- `mem_pct`
- `virt_bytes`
- `status`
- `start_time`
- `run_time`
- `read_bytes`
- `write_bytes`
- `parent_pid`

These come directly from `sysinfo::Process` and `sysinfo::Process::disk_usage()`.

This is enough to support:

- process list rendering
- sorting and filtering
- detail overlay
- kill support
- basic process tree via `parent_pid`

#### Process fields currently Linux-only or Linux-enriched

1. `nice`
2. `threads`
3. `priority`
4. `shr_bytes`
5. `cpu_time_secs`

Current source:

- `procfs::process::Process::stat()`
- `procfs::process::Process::statm()`

Reuse verdict:

- not reusable as-is

BSD path:

- best initial approach is to make these optional or defaulted on BSD
- later, fill them via:
  - `sysinfo` if enough is available per-platform after testing
  - `sysctl` or BSD-native code for the missing subset

Risk:

- medium for UI parity
- low for basic functionality, since the process list can still work without
  full htop-style columns

2. `is_thread`

- Current source: Linux thread enumeration via `/proc/<pid>/task/`
- Reuse verdict: not reusable as-is
- BSD path: optional future work only
- Risk: high for parity, low for baseline port

#### Thread enumeration

Current implementation:

- `enumerate_threads(sys)` in `src/stats/mod.rs`
- Linux-only
- merges standalone thread entries into `ProcSnapshot`

Why it matters:

- the process tree mode on Linux can show threads as children

BSD reuse path:

- no clear crate equivalent to Linux `procfs` thread walking was identified
- `sysinfo::Process` does expose `tasks()` and `thread_kind()`, but parity and
  behavior on BSD would need real testing before relying on it

Recommendation:

- first BSD port should **not** block on per-thread tree entries
- keep tree mode for processes
- hide thread-as-child behavior behind a capability flag

### Status Bar

Source in `dreidel`:

- `src/components/status_bar.rs`

Depends on:

- `SysSnapshot`
- `MemSnapshot`

Reuse verdict:

- reusable almost entirely via `sysinfo`

BSD limitations:

- none significant for baseline support

### CPU Component UI

Source in `dreidel`:

- `src/components/cpu.rs`

Relevant Linux-specific assumptions:

- package temp display
- per-core temp-driven label width
- governor display
- physical core count from Linux-only snapshot fields

Reuse verdict:

- panel itself is portable
- a few fields must become optional/capability-driven rather than Linux-shaped

### Process UI

Source in `dreidel`:

- `src/components/process/mod.rs`

Reuse verdict:

- process filtering, sorting, detail overlays, kill confirm flow, and normal
  tree rendering are reusable

BSD limitations:

- wide-layout htop-style columns may need some cells blank or hidden when data
  is unavailable
- thread-specific hierarchy should be treated as optional

## Reuse Matrix

| Area | Current source | BSD reuse path | Notes |
| --- | --- | --- | --- |
| Hostname / uptime / load | `sysinfo` | `sysinfo` | Direct reuse |
| CPU usage | `sysinfo` | `sysinfo` | Direct reuse |
| CPU frequency | `sysinfo` | `sysinfo` | Validate semantics |
| CPU brand | `sysinfo` | `sysinfo` | Direct reuse |
| Generic temperatures | `sysinfo::Components` | `sysinfo::Components` | Likely reusable, labels differ |
| Per-core temp mapping | Linux hwmon + sysfs | custom or omit | Linux-specific today |
| Governor | `/sys` | omit | Linux-only |
| Physical core count | `/proc/cpuinfo` | `sysinfo` or `sysctl` | Small adaptation |
| RAM / swap totals | `sysinfo` | `sysinfo` | Direct reuse |
| Swap in/out activity | `/proc/vmstat` | `sysctl` later or omit | Optional metric |
| Network counters | `sysinfo` | `sysinfo` | Direct reuse |
| Network drops | `/proc/net/dev` | maybe `sysctl`, else omit | Optional metric |
| Disk counters / metadata | `sysinfo` | `sysinfo` | Direct reuse |
| Process basics | `sysinfo` | `sysinfo` | Direct reuse |
| Process parent tree | `sysinfo` parent pid | `sysinfo` parent pid | Direct reuse |
| Process kill | `sysinfo` | `sysinfo` | Direct reuse |
| Priority / nice / threads / SHR / CPU time | `procfs` | maybe `sysinfo`, else BSD-specific work | Feature degradation likely |
| Thread-as-child tree entries | Linux `/proc/<pid>/task/` | custom later | Do not block first port |

## What A First FreeBSD Port Could Realistically Support

Without major custom low-level BSD work, a first FreeBSD target could likely
support:

- CPU panel
  - per-core usage
  - aggregate usage
  - frequency
  - brand
  - maybe generic temperatures
- Status bar
  - hostname
  - uptime
  - load average
  - RAM and swap gauges
- Network panel
  - per-interface RX/TX rates
  - packets/errors
  - MAC/IP/MTU details
- Disk panel
  - per-device read/write rates
  - usage percentage
  - filesystem and mount details
- Process panel
  - list, sorting, filtering
  - detail overlay
  - kill support
  - parent/child process tree

What would likely be reduced or omitted initially:

- Linux-specific per-core temperature mapping
- governor display
- swap-in / swap-out activity counters
- network drop counters
- full htop-style priority/nice/SHR parity if not available cleanly
- thread entries shown as children in tree mode

That would still be a useful BSD port.

## Recommended Implementation Strategy

### 1. Introduce platform capability flags

Current code relies heavily on `#[cfg(target_os = "linux")]`, which is fine for
raw collection, but too rigid for long-term UX.

Introduce a runtime capability model, for example:

```rust
pub struct PlatformCapabilities {
    pub temperatures: bool,
    pub per_core_temperatures: bool,
    pub cpu_governor: bool,
    pub swap_activity: bool,
    pub network_drop_counters: bool,
    pub process_extended_stats: bool,
    pub process_threads_as_rows: bool,
}
```

This lets the UI degrade cleanly without becoming OS-fragmented.

### 2. Keep `sysinfo` as the default collector backend

Do not replace the collector architecture with BSD-specific code upfront.

Instead:

- keep `sysinfo` as the common baseline backend
- move Linux-only enrichments behind separate helpers
- add BSD-only enrichments only if clearly needed

### 3. Separate baseline process data from Linux enrichments

Today `build_proc` mixes:

- common `sysinfo` data
- Linux-only `procfs` enrichments

Refactor into:

- baseline process entry from `sysinfo`
- optional enrichments per platform

This would make BSD support much easier.

### 4. Treat thread enumeration as optional

Do not make `/proc/<pid>/task/` semantics part of the cross-platform core.

Recommended policy:

- process tree is cross-platform if `parent_pid` exists
- thread-as-child display is platform-dependent

### 5. Use `sysctl` only for targeted gaps

The most likely BSD-specific additions worth implementing are:

- physical core count if `sysinfo` is insufficient
- better temperature labeling / sensor selection
- optional swap or network counters if they materially improve the UI

Avoid building a custom BSD low-level collector unless there is a concrete gap
that materially harms usability.

## Suggested Milestones

### Milestone 1: FreeBSD baseline build support

- ensure crate compiles on FreeBSD
- keep collector and components building with Linux-only pieces disabled

### Milestone 2: FreeBSD baseline runtime support

- CPU, memory, system, network, disk, and process panels all function
- process filter/sort/detail/kill work
- process tree works at process level

### Milestone 3: FreeBSD UX cleanup

- hide unsupported columns and help text
- make optional metrics render cleanly
- normalize temperature presentation if available

### Milestone 4: FreeBSD enhancements

- add `sysctl`-backed improvements for missing counters
- investigate whether `sysinfo::Process::tasks()` can support thread views well

### Milestone 5: broader BSD follow-up

- evaluate NetBSD using the same approach
- assess DragonFlyBSD separately, likely with more custom work if `sysinfo`
  support is weaker there

## Bottom Line

The reuse story for BSD support is better than it first appears.

Use:

- `sysinfo` for the baseline port
- `sysctl` for selective FreeBSD refinements

Do not start by writing a BSD collector from scratch.

For `dreidel` specifically, the portability work is mostly about separating
common snapshots from Linux-only enrichments in `src/stats/mod.rs`, then letting
the UI gracefully tolerate reduced feature sets on non-Linux systems.

That should make a **useful FreeBSD port** realistic without requiring full
Linux feature parity on day one.

## Source Notes

Primary sources used for this analysis:

- `dreidel` source tree, especially:
  - `src/stats/mod.rs`
  - `src/stats/snapshots.rs`
  - `src/components/cpu.rs`
  - `src/components/process/mod.rs`
  - `src/components/status_bar.rs`
- `sysinfo` crate documentation
- `sysctl` crate documentation
- crates.io search for BSD-related Rust system crates
