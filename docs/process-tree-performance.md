# Process Tree Performance Analysis

Issue: [#8](https://github.com/raysuliteanu/dreidel/issues/8)

## Where time is spent (per refresh tick, ~1s default)

| Step                                  | What happens                                                                                             | Scaling                               | Allocations                                                                                                   |
| ------------------------------------- | -------------------------------------------------------------------------------------------------------- | ------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| **1. Stats collector** (`build_proc`) | `sysinfo` iterates all procs; on Linux, reads `/proc/<pid>/task/` for every process to enumerate threads | O(P + T) syscalls                     | Clones strings per process                                                                                    |
| **2. `refresh_display()`**            | Inserts new PIDs into `expanded` HashSet; calls `build_tree()`                                           | O(P+T) for 3 HashSets + HashMap + DFS | Clones every `ProcessEntry` into `children_map`, clones again into `TreeRow`, clones _again_ into `displayed` |
| **3. `draw()`**                       | Iterates `displayed` + `tree_rows`, builds `tree_prefix()` strings                                       | O(visible rows)                       | Prefix string alloc per row                                                                                   |

Where P = process count, T = thread count. On a typical dev machine: P≈584, T≈2208, total entries ≈2792.

## Real bottlenecks (in likely order)

1. **Thread enumeration syscalls** — Reading `/proc/<pid>/task/<tid>/stat` for every thread of every process is ~2200 `open()`+`read()`+`close()` syscall triplets per tick. This dominates wall-clock time and is the only part doing I/O.

2. **Excessive cloning** — `ProcessEntry` contains heap-allocated `String` (name, user) and `Vec<String>` (cmd). The current path clones each entry **3 times** per tick:
   - Once into `children_map` (`p.clone()`)
   - Once into `TreeRow` via DFS stack (`kid.clone()` / `root.clone()`)
   - Once into `displayed` (`r.entry.clone()`)

3. **HashSet/HashMap rebuild** — Three `HashSet<u32>` and one `HashMap<Option<u32>, Vec<ProcessEntry>>` are rebuilt from scratch every tick. At 2800 entries the hashing cost is modest (~50µs) but the memory churn triggers allocator pressure.

4. **Sort per group** — Each parent's child list is sorted independently. With many small groups this is fast, but worst-case (flat hierarchy under pid 1) it's a single O(n log n) sort.

## Rough memory estimates

| Process count | Tree build overhead |
| ------------- | ------------------- |
| 500           | ~384 KB             |
| 2,000         | ~1.5 MB             |
| 5,000         | ~3.8 MB             |
| 10,000        | ~7.7 MB             |

## Benchmarking strategy

Before optimizing, get numbers:

- **Micro-benchmark `build_tree()`** — Use `criterion` with synthetic stubs at 500/2000/5000/10000 entries. Measure wall time and allocation pressure.
- **End-to-end tick timing** — `std::time::Instant` around `refresh_display()`, log duration with real system data. If < 1ms on a 2800-entry system, the whole issue may be premature.
- **Flamegraph** — `cargo flamegraph` with tree mode active to see where actual CPU time goes (syscalls vs. hashing vs. cloning vs. sorting).

## Optimization alternatives

### Option 1: Reduce cloning (low-risk, moderate gain)

Switch `build_tree()` to work with indices or `&ProcessEntry` references instead of owned clones. Store `TreeRow` with an index into `self.raw` rather than a cloned `ProcessEntry`. Eliminate the separate `displayed` vec.

| Pros                                     | Cons                                                  |
| ---------------------------------------- | ----------------------------------------------------- |
| Eliminates 2–3 clones per entry per tick | Requires lifetime management or index indirection     |
| Simple, safe refactor                    | `draw()` becomes slightly more complex (index lookup) |
| Biggest bang-for-buck on memory churn    | Compact snapshot needs rethinking (can't store refs)  |

**Expected impact:** ~60–70% reduction in allocation volume per tick.

### Option 2: Incremental tree update (moderate-risk, high gain on delta)

Keep the previous tick's `children_map` and `tree_rows`. On new data, diff PIDs (added/removed/changed-parent), patch the tree structure, re-sort only affected groups.

| Pros                                                     | Cons                                               |
| -------------------------------------------------------- | -------------------------------------------------- |
| Near-zero work when process list is stable               | Complex diffing logic; edge cases with reparenting |
| Good for typical workloads (few procs change per second) | Still needs full rebuild on sort/filter change     |
| Saves both CPU and allocations                           | Harder to test correctness                         |

**Expected impact:** 80–95% reduction _when process list is stable_ (typical). Falls back to full rebuild otherwise.

### Option 3: Lazy child expansion (low-risk, moderate gain)

Only build subtrees for expanded nodes. Skip `children_map` insertion entirely for children of collapsed parents.

| Pros                                    | Cons                                                     |
| --------------------------------------- | -------------------------------------------------------- |
| Simple change in `build_tree()`         | Only helps when many nodes are collapsed                 |
| Reduces sort calls for collapsed groups | No benefit in default "all expanded" mode                |
| Easy to test                            | Minimal allocation savings (entries still in `self.raw`) |

**Expected impact:** Proportional to collapsed fraction. Zero benefit if all expanded.

### Option 4: Throttle thread enumeration (low-risk, high I/O gain) ✅ IMPLEMENTED

Dual-interval collector: fast interval (`refresh_rate_ms`, default 1s) for all
metrics; slow interval (`thread_refresh_ms`, default 5s) for thread enumeration
via `/proc/<pid>/task/`. Thread entries are cached and merged into every
`ProcUpdate`. Configurable via `general.thread_refresh` in config or
`--thread-refresh` CLI flag.

| Pros                                              | Cons                                                  |
| ------------------------------------------------- | ----------------------------------------------------- |
| Eliminates ~2000 syscalls per tick on this system | Thread CPU/state info becomes stale between refreshes |
| No algorithmic complexity                         | Stale threads may briefly appear after exit           |
| Config flag means users choose the tradeoff       | Slightly more complex collector state                 |

**Expected impact:** ~75% reduction in collector wall time (syscalls dominate).

### Option 5: Shared-ownership entries with `Rc`/`Arc` (moderate-risk, moderate gain)

Wrap `ProcessEntry` in `Rc<ProcessEntry>`. Tree builder, displayed list, and tree rows all share the same allocation.

| Pros                                 | Cons                                                  |
| ------------------------------------ | ----------------------------------------------------- |
| Eliminates all deep clones           | `Rc` not `Send` (but tree builder is single-threaded) |
| Minimal API change                   | Small per-entry overhead (Rc refcount)                |
| Works well with current architecture | —                                                     |

**Expected impact:** ~50% reduction in allocation, negligible CPU change.

## Recommended priority

1. **Measure first** — criterion benchmarks + timing logs
2. ~~**Option 4** — Throttle thread enumeration~~ ✅ Done
3. **Option 1** — Reduce cloning (biggest allocation win)
4. **Option 3** — Lazy expansion (free win when nodes collapsed)
5. **Option 2** — Incremental updates (only if above isn't enough)
