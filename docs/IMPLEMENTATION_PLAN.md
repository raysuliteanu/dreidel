# Implementation Plan: Issue Resolution

Generated from the 29 issues in `docs/issues/`. Issues are grouped to minimise the number of
files touched across commits. Where a high-priority issue already requires editing a file,
all lower-priority issues in that same file are bundled into the same step.

---

## Guiding Principles

1. **Safety first** — reliability issues (silent error discard) before performance, DRY, or style.
2. **Broad changes isolated** — the `Component::update` trait signature change (H4) touches
   every component file; it gets its own dedicated step with no unrelated edits mixed in.
3. **DRY refactor last among mediums** — the net/disk restructure is the most complex change;
   it comes after the codebase is stable from the earlier steps.
4. **CI last** — config-only changes, no code risk, can land independently at any time.

---

## Step 1 — `src/tui.rs`: Fix blocking shutdown + clean up public surface

**Commits: 1**

| Issue | Priority |
|-------|----------|
| `high-async-blocking_sleep_tui_stop.md` | HIGH |
| `med-quality-tui_pub_fields.md` | MED |
| `low-quality-tui_size_dead_code.md` | LOW |

**Rationale**: All three issues live entirely in `src/tui.rs`. The MED and LOW items are
trivial alongside the H1 fix and add no review complexity.

**Work:**
1. Replace the spin-sleep loop in `Tui::stop()` with a `tokio::time::timeout` await.
   Return `Err` on timeout instead of silently returning `Ok(())`.
2. Make all struct fields private (or `pub(crate)`). Expose a `clear()` method on `Tui`
   to replace the one `tui.terminal.clear()` call in `app.rs`.
3. Remove the unused `size()` method and its `#[allow(dead_code)]` attribute.

**Tests**: Existing `tui.rs` tests must pass. Add a test for `stop()` timeout behaviour if
practical (may require mocking the join handle).

---

## Step 2 — `src/app.rs`: Reliability fixes + quality improvements

**Commits: 1**

| Issue | Priority |
|-------|----------|
| `high-reliability-draw_errors_silently_dropped.md` | HIGH |
| `high-reliability-key_handler_errors_discarded.md` | HIGH |
| `med-quality-silent_config_fallback.md` | MED |
| `med-performance-render_clones_per_frame.md` | MED |

**Rationale**: All four issues are in `src/app.rs`. H2 and H3 are the primary motivation;
bundling M-quality and M-performance saves a second pass over this file.

**Work:**
1. **(H2)** Replace every `let _ = comp.draw(...)` inside the `terminal.draw` closure with
   explicit error logging via `tracing::warn!`.
2. **(H3)** Replace `.ok().flatten()` in the focused-component key dispatch with an explicit
   `match` that logs errors at `tracing::warn!` before returning `None`.
3. **(M: config fallback)** Add `tracing::warn!` at both silent-fallback sites
   (`LayoutPreset::from_str` and the `status_bar` string match). Implement `FromStr` /
   `strum::EnumString` on `StatusBarPosition` to replace the raw string match.
4. **(M: render clones)** Store the resolved `ColorPalette` as a field in `App` (computed
   once in `App::new`, eliminating the per-render `palette()` call). Restructure the
   `render_to` draw closure to avoid cloning `visible` and `slot_overrides` per frame.

**Tests**: All existing `app.rs` tests must pass. Verify that the test suite still compiles
after `Tui::clear()` is used in place of `tui.terminal.clear()` (Step 1 dependency).

---

## Step 3 — Component trait: Fix `Action` clone fan-out

**Commits: 1**

| Issue | Priority |
|-------|----------|
| `high-performance-action_clone_fanout.md` | HIGH |
| `med-quality-blanket_allow_unused.md` (action.rs portion only) | MED |

**Rationale**: H4 requires changing the `Component::update` signature from
`fn update(&mut self, action: Action)` to `fn update(&mut self, action: &Action)`. This
mechanically touches every component file. Since `action.rs` must be opened anyway (to
update `Action` dispatch logic), clean up the blanket `#![allow(unused)]` at the same time.
The `snapshots.rs` half of the blanket-allow issue is deferred to Step 4 where snapshots.rs
is already being edited.

**Work:**
1. Change `Component::update` in `src/components/mod.rs` to take `&Action`.
2. Update the fan-out loop in `src/app.rs` to pass `&action` (no clone).
3. Update all `update` implementations mechanically:
   - `cpu.rs`, `net.rs`, `disk.rs`, `process/mod.rs`, `status_bar.rs`, `help.rs`
   - Match on `action` (now a reference); clone only the fields that need to be stored
     (e.g. `Action::CpuUpdate(snap) => { self.latest = Some(snap.clone()); }`).
4. In `action.rs`: remove `#![allow(unused)]`. Apply per-variant `#[allow(dead_code)]`
   with comments to the variants that are intentionally unused (`Error`, `Suspend`,
   `Resume`, `FocusGained`, `FocusLost`).

**Tests**: All 269 tests must pass after this purely mechanical change.

**Dependency**: Step 2 must be complete (the fan-out loop in `app.rs` is already being
modified in Step 2; this step extends that same function).

---

## Step 4 — `net.rs` / `disk.rs` DRY refactor + performance

**Commits: 3–4 (incremental, same branch/PR)**

| Issue | Priority |
|-------|----------|
| `med-dry-history_len_triplicated.md` | MED |
| `med-dry-fmt_rate_duplicated.md` | MED |
| `med-dry-netview_diskview_identical.md` | MED |
| `med-dry-name_matches_clamp_border_duplicated.md` | MED |
| `med-dry-filter_state_machine_duplicated.md` | MED (net/disk portions) |
| `med-dry-detail_view_pattern_duplicated.md` | MED |
| `med-performance-latest_clone_in_draw.md` | MED |
| `med-performance-tolowercase_double_alloc.md` | MED |
| `med-performance-view_state_clone_for_match.md` | MED (net/disk portions) |
| `med-quality-blanket_allow_unused.md` (snapshots.rs portion) | MED |
| `low-quality-disk_kind_string_not_enum.md` | LOW |

**Rationale**: `net.rs` (1,304 lines) and `disk.rs` (1,103 lines) share ~60% of their
structure. All DRY and performance issues for these two files are done in a single
focused effort. The `disk_kind` enum and `snapshots.rs` cleanup are included because
`snapshots.rs` must be opened for the `DiskDeviceSnapshot` change, and `disk.rs` is
already being refactored.

**Sub-steps (separate commits):**

### 4a — Shared utilities in `components/mod.rs`
- Move `HISTORY_LEN: usize = 100` to `mod.rs` as `pub(crate) const`.
- Move `fmt_rate(bytes_per_sec: u64) -> String` to `mod.rs` as `pub(crate) fn`.
- Remove local declarations in `cpu.rs`, `net.rs`, `disk.rs`; update call sites.
- *Files*: `components/mod.rs`, `cpu.rs`, `net.rs`, `disk.rs`

### 4b — Shared `ListView` enum and `FilterInput` helper in `components/mod.rs`
- Introduce `pub(crate) enum ListView { List, Filter { input: String }, Detail { name: String } }`.
- Introduce `pub(crate) struct FilterInput` with a `handle_key(KeyEvent) -> FilterEvent` method
  encapsulating the Esc/Enter/Backspace/Char state machine.
- *Files*: `components/mod.rs` only (no existing components changed yet).

### 4c — Refactor `NetComponent` to use shared types
- Replace `NetView` with `ListView`.
- Replace duplicated `name_matches`, `clamp_selection`, `border_block` with shared
  helpers or free functions.
- Delegate filter key handling to `FilterInput`.
- Fix `self.latest.clone()` in draw → use `as_ref()`.
- Fix `to_lowercase()` double-alloc in `name_matches` (store filter pre-lowercased).
- Fix `match &self.view.clone()` in draw dispatch → match on `&self.view` directly.
- *Files*: `net.rs`

### 4d — Refactor `DiskComponent` + snapshot cleanup
- Same refactor as 4c applied to `DiskComponent`.
- In `snapshots.rs`: Replace `DiskDeviceSnapshot::kind: String` with `DiskKindSummary` enum.
  Update `build_disk()` in `stats/mod.rs` to produce the enum.
- In `snapshots.rs`: Remove `#![allow(dead_code)]`. Apply per-field `#[allow(dead_code)]`
  with comments to genuinely unused `ProcessEntry` fields.
- *Files*: `disk.rs`, `stats/snapshots.rs`, `stats/mod.rs`

**Tests**: All snapshot tests must be regenerated (`INSTA_UPDATE=always cargo test`) after
any rendering change. All 269 tests must pass at end of step.

---

## Step 5 — `src/components/cpu.rs`: Performance + DRY + visibility

**Commits: 1**

| Issue | Priority |
|-------|----------|
| `med-performance-filtered_cores_repeated_alloc.md` | MED |
| `med-performance-view_state_clone_for_match.md` (cpu portion) | MED |
| `med-dry-filter_state_machine_duplicated.md` (cpu portion) | MED |
| `low-quality-cpu_per_core_history_pub.md` | LOW |

**Rationale**: All cpu.rs issues in one pass. The filter state machine fix (Step 4b's
`FilterInput`) is a prerequisite — this step consumes it.

**Dependency**: Step 4b must be complete (`FilterInput` must exist in `mod.rs`).

**Work:**
1. Add `filtered_core_count() -> usize` helper that counts without allocating a `Vec`.
   Use it in `preferred_height()` and `clamp_scroll()`. Keep `filtered_cores() -> Vec<usize>`
   for the draw path (which needs the index list).
2. Fix `match self.state.clone()` in `handle_key_event` using `std::mem::replace` or
   by matching on `&self.state` with a follow-up mutation.
3. Delegate filter key handling in `CpuState::FilterMode` to the shared `FilterInput` helper.
4. Change `pub per_core_history` to private. Add a `#[cfg(test)]` accessor method if
   the existing `history_ring_buffer_bounded` test requires it.

**Tests**: All cpu.rs unit tests must pass.

---

## Step 6 — `src/components/process/mod.rs`: Quality fixes

**Commits: 1**

| Issue | Priority |
|-------|----------|
| `med-quality-unused_name_clone_kill_confirm.md` | MED |
| `med-quality-kill_uses_subprocess.md` | MED |
| `med-performance-view_state_clone_for_match.md` (process portion) | MED |

**Rationale**: All process component issues in one pass. Small, well-isolated fixes.

**Work:**
1. Remove `let _name = name.clone()` from the `KillConfirm` branch entirely.
2. Replace `std::process::Command::new("kill")` with a direct syscall.
   Check whether `nix` is already a transitive dependency; if so use
   `nix::sys::signal::kill`. Otherwise use `libc::kill` (already transitively available).
3. Fix `match &self.state.clone()` in key handler and draw helpers using `std::mem::replace`
   or by extracting needed data before the match.

**Tests**: All process component tests must pass. Add a test for the kill path if one does
not already exist.

---

## Step 7 — `src/stats/mod.rs`: Annotate wildcard process status mapping

**Commits: 1**

| Issue | Priority |
|-------|----------|
| `med-quality-process_status_wildcard.md` | MED |

**Rationale**: Isolated single-file change; too small to bundle with anything else without
obscuring intent.

**Work:**
Add a comment above the `_ => ProcessStatus::Unknown` arm listing the current `sysinfo`
variants that fall through, so future maintainers know the mapping is intentional and can
update it after a `sysinfo` upgrade.

---

## Step 8 — `.github/workflows/ci.yml`: CI hardening

**Commits: 1**

| Issue | Priority |
|-------|----------|
| `low-ci-cargo_test_no_locked.md` | LOW |
| `low-ci-no_coverage_reporting.md` | LOW |
| `low-ci-fmt_job_no_cache.md` | LOW |
| `low-ci-actions_not_sha_pinned.md` | LOW |
| `low-ci-no_aarch64_check.md` | LOW |

**Rationale**: All CI changes are config-only, zero code risk, and fully independent of
all other steps. They can land as a standalone PR at any time; placing them last avoids
blocking on them.

**Work:**
1. Add `--locked` to all `cargo test`, `cargo clippy`, and `cargo fmt --check` invocations.
2. Add `Swatinem/rust-cache@v2` to the `fmt` job.
3. Add a `coverage` job using `cargo-llvm-cov` with `--fail-under-lines 80`.
4. Add a `cross-check` job running `cargo check --locked --target aarch64-unknown-linux-gnu`.
5. Pin all `uses:` references to commit SHAs. Use Dependabot or Renovate to keep them
   updated via automated PRs.

---

## Execution Order Summary

```
Step 1  src/tui.rs                  H1 + M(tui pub fields) + L(size dead code)
Step 2  src/app.rs                  H2 + H3 + M(config fallback) + M(render clones)
Step 3  Component trait + action.rs H4 + M(allow_unused / action.rs)
Step 4  net.rs + disk.rs            6× DRY + 3× perf + L(disk_kind) + M(allow_unused / snapshots.rs)
  4a    components/mod.rs           shared HISTORY_LEN + fmt_rate
  4b    components/mod.rs           ListView + FilterInput
  4c    net.rs                      NetComponent refactor
  4d    disk.rs + snapshots.rs      DiskComponent refactor + type cleanup
Step 5  cpu.rs                      3× perf/DRY + L(per_core_history pub)
Step 6  process/mod.rs              2× quality + perf(view_state_clone)
Step 7  stats/mod.rs                M(process_status wildcard comment)
Step 8  ci.yml                      5× CI (locked, coverage, cache, SHA pins, aarch64)
```

## Issue Coverage

| Step | Issues resolved | Files primary |
|------|----------------|---------------|
| 1 | 3 | tui.rs |
| 2 | 4 | app.rs |
| 3 | 2 | action.rs, mod.rs, all components (mechanical) |
| 4 | 11 | mod.rs, net.rs, disk.rs, snapshots.rs, stats/mod.rs |
| 5 | 4 | cpu.rs |
| 6 | 3 | process/mod.rs |
| 7 | 1 | stats/mod.rs |
| 8 | 5 | ci.yml |
| **Total** | **33** | *(29 issues; 4 split across steps 3+4, 4+5)* |

## Dependencies Between Steps

```
Step 1  ──► Step 2  (Tui::clear() used in app.rs)
Step 2  ──► Step 3  (app.rs action fan-out loop)
Step 4b ──► Step 5  (FilterInput used in cpu.rs)
```

Steps 6, 7, and 8 are fully independent and can be done in any order after Step 3.
