# CPU Performance Plan

Goal: bring dreidel's steady-state CPU usage in line with btop. Observed on
macOS: btop reports itself at < 1% CPU, dreidel reports itself at 5–9%.

Related: [process-tree-performance.md](process-tree-performance.md) (earlier
analysis of the tree builder).

## Phase 0 — Fair baseline

Two confounders make the self-reported numbers not directly comparable:

- **Refresh rate.** btop defaults to `update_ms = 2000`; dreidel defaults to
  `refresh_rate_ms = 1000`, so dreidel does ~2× the work per second before any
  code differences.
- **Self-reported %.** Each tool computes its own CPU% differently. Use an
  external measurement instead: accumulated CPU time over a fixed window.

`scripts/cpu-bench` runs a command in a detached, fixed-size tmux session,
samples its accumulated CPU time after a warm-up and again after a measurement
window, and reports average CPU%. Compare at equal refresh intervals:

```bash
cargo build --release
scripts/cpu-bench -- target/release/dreidel --refresh-rate 1s
scripts/cpu-bench -- btop --update 1000
scripts/cpu-bench -- target/release/dreidel --refresh-rate 2s
scripts/cpu-bench -- btop --update 2000
```

This number is the target metric for every later phase.

## Phase 1 — Profile

1. Add a `[profile.profiling]` that inherits `release` with `debug = true`.
2. Sample with samply: `scripts/profile -- target/profiling/dreidel` records
   60 s in a detached tmux session, and `scripts/profile-report
   target/profile.json.gz` summarizes on-CPU time by thread and function
   (`--grep PAT…`, `--callers LEAF`). `xctrace` or `perf record -g` also work.
3. Add `tracing` debug spans with timings around each collector step
   (`refresh_specifics`, nets, disks, components, gpus, `enumerate_threads`,
   `build_proc`, `build_cpu`, `cached_threads.clone()`) and around
   `App::render`, so `dreidel.log` gives a per-tick breakdown without a
   profiler.

Output: a ranked table of % CPU by step, collector vs UI.

## Phase 2 — Microbenchmarks

Alongside the existing `benches/tree_build.rs`:

- `benches/collector.rs` — real sysinfo (noisy but informative):
  `System::refresh_specifics` with `ProcessRefreshKind::everything()` vs the
  current configuration vs a minimal one.
- `benches/process_update.rs` — `ProcessComponent::update(ProcUpdate)` with
  ~1000 synthetic entries (clone + filter + sort + tree).
- `benches/render.rs` — each component's `draw` into a `TestBackend` at a
  realistic size (e.g. 200×60).

## Phase 3 — Hypotheses (ranked by expected impact; verify before fixing)

1. **Full process refresh every tick** (`src/stats/mod.rs`,
   `refresh_specifics`). `ProcessRefreshKind::everything()` re-reads cmd,
   environ, exe, cwd, root, user and disk usage for every process every tick.
   On macOS cmd/environ cost a `sysctl(KERN_PROCARGS2)` per process. environ is
   never displayed. btop reads cmdline only once per new pid. Fix: drop
   environ; use `UpdateKind::OnlyIfNotSet` for cmd/exe/cwd/root.
2. **Unused sensor refresh on macOS.** `components.refresh(false)` runs every
   tick, but `macos::build_cpu_macos` never reads `components` — wasted
   SMC/IOKit work. Also measure `gpus.refresh` on macOS.
3. **Per-tick allocation churn.** `build_proc` allocates ~10 `String`s per
   process per tick; `ProcessComponent` then deep-clones the whole vec
   (`self.raw = snap.processes.clone()`), and `cached_threads.clone()` copies
   every thread entry each tick. Options: `Arc<str>` for stable strings,
   `Arc<[ProcessEntry]>` for cached threads, avoid the second clone.
4. **Thread enumeration spike every 5 s** (macOS `enumerate_threads`): one
   `pidinfo::<TaskAllInfo>` per process plus one `pidinfo::<ThreadInfo>` per
   thread. Options: enumerate only when tree view is active / for expanded
   nodes, or lengthen the cadence.
5. **Process view rebuilt every update.** `refresh_display` re-filters,
   re-sorts and rebuilds the tree each tick; cache when sort/filter/pid-set are
   unchanged, if benchmarks say it matters.
6. **Rendering.** Already 1 fps with ratatui diffing, likely minor. Confirm
   that mouse-move events (mouse capture is on) aren't waking the loop more
   than expected.

Work one change at a time: `cpu-bench` before/after, keep only if it moves the
number, and commit each with the numbers in the message.

## Phase 4 — Guard against regressions

- Document `scripts/cpu-bench`, the profiling profile, and the benches in
  `BUILDING.md`.
- Optionally keep criterion baselines (`--save-baseline`) to compare before
  releases.
- Record final before/after numbers in `CHANGELOG.md`.

## Results

### Phase 0 baseline (2026-11-21)

Apple M4 Pro (12 cores), macOS, ~594 processes, release build of v0.8.2,
200×50 terminal, 10 s warm-up, 60 s window, one run each:

| Command                                          | CPU time / 60 s | Avg CPU |
| ------------------------------------------------ | --------------: | ------: |
| `dreidel --refresh-rate 1s` (dreidel default)    |          5.79 s |   9.65% |
| `btop --update 1000`                             |          2.42 s |   4.03% |
| `dreidel --refresh-rate 2s`                      |          3.41 s |   5.68% |
| `btop --update 2000` (btop default)              |          1.20 s |   2.00% |
| `dreidel --refresh-rate 1s --thread-refresh 600s` |          5.10 s |   8.50% |

Takeaways:

- At equal intervals dreidel uses **~2.4× (1 s) to ~2.8× (2 s)** btop's CPU.
  Default-vs-default it's ~4.8×, half of which is just the faster default
  refresh rate.
- btop's self-reported "< 1%" understates its real usage (2% at defaults), so
  the gap seen in the process list is larger than the real one.
- Thread enumeration (every 5 s) costs **~1.15%** of the 9.65%. The remaining
  **~8.5%** is per-fast-tick work (~85 ms CPU per tick), so hypotheses 1–3
  (process refresh, sensor refresh, allocation) are where the bulk is.
- Single runs; repeat before relying on differences under ~0.5%.

### Phase 1 profile (2026-11-21)

Same machine. `cargo build --profile profiling`, 60 s under
`samply record --save-only --unstable-presymbolicate`, CPU attributed by
`threadCPUDelta` (on-CPU time, not wall time). Total 5.54 s CPU ≈ 92 ms/s.
Per-step wall times come from the `dreidel::perf` debug logs (`-vv`).

| Where                                                     | % of CPU | Wall/tick | Notes |
| --------------------------------------------------------- | -------: | --------: | ----- |
| **stats-collector thread**                                |  **84%** |    139 ms | |
| ↳ process refresh (`refresh_processes_specifics`)         |      37% |     33 ms | see below |
| &nbsp;&nbsp;↳ `KERN_PROCARGS2` sysctl in `get_process_infos` |    20% |           | per process, every tick |
| &nbsp;&nbsp;↳ `proc_pidinfo` task info                    |       8% |           | needed for cpu/mem |
| &nbsp;&nbsp;↳ `get_cwd_root`                              |       7% |           | |
| &nbsp;&nbsp;↳ `get_exe_and_name_backup`                   |       2% |           | |
| ↳ `Disks::refresh`                                        |      28% |     21 ms | CacheDelete IPC for "available capacity for important usage" |
| ↳ `macos::read_net_drops`                                 |       6% |           | full `NET_RT_IFLIST2` dump per interface → O(ifaces²) |
| ↳ `Components::refresh`                                   |       4% |     74 ms | mostly blocked in SMC IPC; result unused on macOS |
| ↳ `Networks::refresh`                                     |       3% |      2 ms | |
| ↳ `macos::enrich_process_entry`                           |       2% |           | |
| ↳ `macos::enumerate_threads` (every 5 s)                  |       1% |      4 ms | |
| **main thread** (`App::handle_actions`)                   |  **15%** |           | |
| ↳ `App::render`                                           |     8.5% |      8 ms | ProcessComponent draw ≈ 5% |
| ↳ `ProcessComponent::update`                              |     3.5% |      5 ms | `refresh_display` 2%, `ProcessEntry::clone` 1.8% |

Findings that change the Phase 3 plan:

- **Slow ticks do a full extra refresh.** The slow interval is its own
  `select!` arm, so each slow tick runs every refresh (sysinfo, disks,
  components…) in addition to the fast ticks: 78 ticks in ~65 s. That's ~20%
  extra collector work at the default 1 s / 5 s, and explains the fixed cost
  that doesn't scale with refresh rate in the Phase 0 numbers.
- **Disk refresh is the single most expensive call** (28%), driven by
  CacheDelete's purgeable-space computation for free space. Storage numbers
  barely change second to second.
- **`UpdateKind::OnlyIfNotSet` won't remove the `KERN_PROCARGS2` sysctl on
  macOS.** In sysinfo 0.40.4 `get_process_infos` issues the sysctl for every
  process on every refresh and only checks `cmd()`/`environ()` afterwards, to
  decide what to parse. `OnlyIfNotSet` for cwd/root (7%) and dropping environ
  still help; avoiding the 20% needs an upstream change or not refreshing
  every process every tick.
- **`Components::refresh` is 74 ms wall but only ~4% CPU** — it's blocked, not
  busy. Skip it on macOS anyway, since `build_cpu_macos` never reads it.
- UI work is only ~15%, so UI optimizations come last.

Revised Phase 3 order (estimated CPU saved, as % of the current total):

1. Slow tick should only enumerate threads, not repeat the full refresh (~15–20%).
2. Refresh disk storage on a slow cadence; keep I/O counters per tick (up to ~25%).
3. `read_net_drops`: one sysctl dump per tick, look up each interface (~5%).
4. Process refresh: `OnlyIfNotSet` for cwd/root/exe, no environ (~7–9%).
5. Skip `Components::refresh` on macOS (~4%, plus 74 ms less wall time per tick).
6. Look into the `KERN_PROCARGS2` cost upstream in sysinfo (~20%, needs an upstream change).
7. UI: avoid the `ProcUpdate` deep clone; cache `refresh_display` (~3%).
