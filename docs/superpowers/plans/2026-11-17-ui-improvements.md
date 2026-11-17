# UI Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the visual hierarchy, make charts and panels degrade gracefully as the terminal shrinks, and coalesce resize bursts.

**Architecture:** Palette and border changes are local. Chart density moves into `HistoryChart` through a shared `visible_window()` helper that components also use for y-axis scaling. Each chart panel picks a rendering tier from its `Rect`. A new `fit_layout()` in `layout.rs` drops low-priority panels when slots fall below per-component minimum sizes. `App::render_to` computes the layout before `terminal.draw`, so the status bar can list dropped panels. Resize coalescing lives in the `Tui` event loop.

**Tech Stack:** Rust, ratatui 0.30, crossterm, tokio, insta.

**Spec:** `docs/dreidel-ui-improvements.md`

## Spec review: corrections and decisions

These came out of checking the spec against the code at HEAD (`39b3ca8`). The tasks below already include them.

1. **Snapshots don't record colors.** `TestBackend`'s `Display` output is symbols only, so the `dim` fix (spec §1) changes no snapshot. Task 1 adds a unit test on the palette instead.
2. **Disk's compact view has no chart.** Only the Disk detail view uses `HistoryChart`. Disk tiering (spec §3) applies only to the detail view.
3. **Net never renders an empty box.** Its compact list always draws, and the chart header only appears when `inner.height >= 9`. Net and Disk need two tiers (with or without legend), not four.
4. **The y-max must be computed outside `HistoryChart`.** Disk prints `y_max` as legend labels before render, so the window size has to be available to components. That is the job of `chart::visible_window()` (Task 3).
5. **`min_size` goes on `ComponentId`, not the `Component` trait.** Minimums are constants, and `layout.rs` already depends on `ComponentId` but never sees the components themselves. This deviates from the spec's suggestion.
6. **Dropped panels stay reachable by focus key.** Today `p/c/n/d` do nothing for ids not in `rendered_ids`. Decision: the focused panel is never dropped. Pressing the key of a dropped panel focuses it, and the next layout pass makes room for it by dropping a different panel.
7. **The status bar has no row for hints.** Dropped panels go in the status bar block's bottom border title. If the status bar is hidden, no hint is shown, but the keys still work.
8. **The status bar takes 6–7 rows.** At the spec's 40×10 floor, only 3–4 rows would be left for panels. Decision: hide the status bar automatically when showing it would leave fewer than 10 content rows.
9. **"Render once per tick" would lag by up to 1 s.** The render tick runs at `refresh_rate_ms`, 1000 ms by default. Task 9 debounces in the `Tui` event loop instead (25 ms).
10. **Contrast of the dark `dim`.** Tokyo Night comment `Rgb(86,95,137)` on `bg Rgb(26,27,38)` has a contrast of about 2.8:1. That is fine for axes but low for text that carries content: Net IP addresses (`net.rs:520`) and Process detail labels (`process/mod.rs:1176,1187`). Task 1 keeps the spec value and moves those spots to `fg` only if they read badly. If they do, an alternative is Tokyo Night `dark5` `Rgb(115,122,162)`, at about 4.1:1.

## Global Constraints

- VCS is jj: commit with `jj commit -m '<type>: <msg>'`, never `git commit`. Conventional-commit prefixes.
- Clippy runs with `-D warnings` after every `.rs` save. Finish the change inside a file before saving, or use a temporary `#[allow(dead_code)]` and remove it later.
- Use `.context("present tense phrase")` on every `?` and `expect("why")` instead of `unwrap()` outside tests.
- Platform-sensitive snapshots are gated by `#[cfg(target_os = ...)]`, following commit `d2d0a63`.
- Accept intentional snapshot changes with `INSTA_UPDATE=always cargo test`, then review every `.snap` diff.
- After each user-visible change, check `USER_GUIDE.md` and `ARCHITECTURE.md`, and the doc-screenshot suite (Linux-gated).
- Dark `dim = Rgb(86, 95, 137)`. Light `dim = Rgb(120, 120, 120)`.
- `HISTORY_LEN = 600`. Window = `(graph_width * 2).clamp(1, history_len)`.
- Drop priority, highest kept first: Process > CPU > Net > Disk.
- Absolute floor: terminal smaller than 40×10 → centered `terminal too small (need 40×10)`.
- Size matrix for snapshots: `200×50, 120×40, 80×24, 60×20, 40×12, 30×8`.

## Review Focus

1. **Focus on a dropped panel.** Pressing `d` when Disk was dropped must make Disk visible, not leave focus on an invisible panel. Test in Task 7.
2. **An old spike after narrowing.** If a burst happened outside the visible window, Net/Disk must not scale the y-axis to it. Test in Task 3.
3. **Fullscreen overlay while panels are dropped.** The fullscreen modal must still render the focused component, and dropped panels must not be drawn behind it. Test in Task 7.
4. **No data yet.** At every tier, CPU must render without a snapshot and without panicking (narrow/tiny tiers read `snap.aggregate`). Test in Task 4.
5. **CPU filter leaves 0 cores at the tiny tier.** The tiny-tier text loop over an empty `filtered` slice must not panic or index out of range. Test in Task 4.

---

### Task 1: Fix the `dim` palette role

**Files:**
- Modify: `src/theme.rs:48` (dark `dim`), `src/theme.rs:62` (light `dim`)
- Test: `src/theme.rs` (new `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: nothing new. The `ColorPalette` fields are unchanged.

- [ ] **Step 1: Write the failing test.** Add a test-only `fn luminance(c: Color) -> f64` (relative luminance: sRGB→linear, then `0.2126 R + 0.7152 G + 0.0722 B`; panic on non-`Rgb`) and two tests. The light test compares against literal white because light `bg` is the named `White`:
  ```rust
  #[test]
  fn dark_dim_is_darker_than_fg() {
      let p = ColorPalette::dark();
      assert!(luminance(p.dim) < luminance(p.fg));
      assert!(luminance(p.dim) > luminance(p.bg));
  }
  #[test]
  fn light_dim_is_lighter_than_fg() {
      let p = ColorPalette::light();
      assert!(luminance(p.dim) > luminance(p.fg));
      assert!(luminance(p.dim) < luminance(Color::Rgb(255, 255, 255)));
  }
  ```
- [ ] **Step 2: Run** `cargo test theme::` → `dark_dim_is_darker_than_fg` FAILS, because dim 220,225,240 is brighter than fg. `light_dim_is_lighter_than_fg` already passes (30 vs 20), which is fine: it guards the new value.
- [ ] **Step 3: Implement.** Set dark `dim: Rgb(86, 95, 137)` and light `dim: Rgb(120, 120, 120)`.
- [ ] **Step 4: Audit usages.** Run `grep -rn "\.dim" src | grep -v theme.rs` (35 hits). Run `cargo run` in both themes (`--theme light`, see `cargo run -- --help`) and look at the Net IP column, the Process detail labels, the status bar (`status_bar.rs:311`) and the CPU header labels. Switch to `palette.fg` only the spots where the text is content and unreadable. Record each switch in the commit body.
- [ ] **Step 5: Run** `cargo test` → PASS with no snapshot changes (colors aren't captured).
- [ ] **Step 6: Commit.** `jj commit -m 'fix: make dim palette role subordinate to fg in both themes'`

### Task 2: Rounded borders through a shared panel block

**Files:**
- Modify: `src/components/mod.rs:237-254` (`list_border_block` → `panel_block`), `src/components/cpu.rs:~478-495`, `src/components/process/mod.rs:~613-621`, `src/components/net.rs` and `src/components/disk.rs` (call sites of `list_border_block` / `border_block`), `src/components/help.rs:~90`, `src/components/status_bar.rs:~91`
- Test: existing snapshots in `src/components/snapshots/`, `src/components/process/snapshots/`

**Interfaces:**
- Produces: `pub(crate) fn panel_block(focus_key: char, rest: &str, palette: &ColorPalette, focused: bool) -> Block<'static>`. Same as today's `list_border_block` plus `.border_type(BorderType::Rounded)`. `list_border_block` is renamed, not kept alongside.

- [ ] **Step 1: Rename** `list_border_block` → `panel_block`, add `.border_type(BorderType::Rounded)`, and update its doc comment ("used by all main panels").
- [ ] **Step 2: Replace** the hand-built blocks in `CpuComponent::draw` and `ProcessComponent::draw` with `panel_block(self.focus_key, &title_rest, &self.palette, self.focused)`.
- [ ] **Step 3: Add** `.border_type(BorderType::Rounded)` to the help block, the status-bar block and the loading popup in `app.rs:~566`. Leave inner dialogs (kill confirm/error) and the separator lines (`Borders::TOP`/`LEFT`) as they are.
- [ ] **Step 4: Run** `INSTA_UPDATE=always cargo test`, then `jj diff --stat -- '**/*.snap'`. Expected: only the corner glyphs change, `┌┐└┘` → `╭╮╰╯`. Inspect at least the cpu, process, help and status-bar snapshots.
- [ ] **Step 5: Regenerate the doc screenshots** if on Linux. On macOS, note in the commit body that `docs/screenshots/` needs a Linux regen.
- [ ] **Step 6: Commit.** `jj commit -m 'feat: rounded panel borders via shared panel_block helper'`

### Task 3: History window tracks chart width

**Files:**
- Modify: `src/components/mod.rs:163` (`HISTORY_LEN = 600`), `src/components/chart.rs` (`render`, new helper, tests), `src/components/net.rs:535-575` and `:705-730` (y-max), `src/components/disk.rs:565-590` (y-max)
- Test: `src/components/chart.rs`, `src/components/net.rs`, `src/components/disk.rs`

**Interfaces:**
- Produces, in `chart.rs`:
  - `pub(crate) fn visible_window(area_width: u16, legend_width: u16, history_len: usize) -> usize`, which returns `((area_width - legend_width) as usize * 2).clamp(1, history_len)` using a saturating subtraction.
  - `pub(crate) fn window_max<'a>(series: impl IntoIterator<Item = &'a VecDeque<u64>>, window: usize) -> u64`, the max over the last `window` samples of each series. It returns 0 when every series is empty.
- `HistoryChart::render` uses `visible_window(area.width, self.legend_width, self.history_len)`. It keeps only the last `window` samples per series, right-aligned, with `x_max = window as f64`.

- [ ] **Step 1: Write failing tests** in `chart.rs`:
  ```rust
  #[test] fn window_is_two_samples_per_graph_column() {
      assert_eq!(visible_window(40, 11, 600), 58);
  }
  #[test] fn window_capped_by_history_len() {
      assert_eq!(visible_window(400, 11, 600), 600);
  }
  #[test] fn window_never_zero() {
      assert_eq!(visible_window(5, 11, 600), 1);
  }
  #[test] fn window_max_ignores_samples_outside_window() {
      let a: VecDeque<u64> = [9_000, 1, 2, 3].into();
      let b: VecDeque<u64> = [5, 4].into();
      assert_eq!(window_max([&a, &b], 3), 5);
  }
  ```
  In `net.rs`, add `compact_chart_ignores_spike_outside_window`. Push one sample of `10_000_000` followed by 100 samples of `2_000` into `agg_history`, render the list view at 40×20, and assert the snapshot (the TX legend shows `2.0 KB/s`, and the graph line is not flattened to the bottom row). Add the equivalent `disk_detail_ignores_spike_outside_window`, asserting that the top legend label is `fmt_rate(2_000)` and not `10.0 MB/s`.
- [ ] **Step 2: Run** `cargo test chart:: net:: disk::` → FAIL (functions missing).
- [ ] **Step 3: Implement** both helpers and the windowed `render`. Set `HISTORY_LEN = 600`. In Net (both charts) and Disk detail, compute `let window = visible_window(area.width, LEGEND_W, HISTORY_LEN);` and `y_max = window_max([tx, rx], window).max(MIN_CHART_FLOOR)`, replacing the whole-buffer `.max()` chains. Update the module doc example to show `HistoryChart::new(HISTORY_LEN)`.
- [ ] **Step 4: Run** `INSTA_UPDATE=always cargo test`. Expected snapshot diffs: chart line density changes in `chart_*`, `cpu_*`, `net_*` and `disk_graph_view`. Check that each graph now spans the full width it is given. The existing `len() <= HISTORY_LEN` assertions still hold.
- [ ] **Step 5: Commit.** `jj commit -m 'feat: size chart history window to chart width'`

### Task 4: Tiered CPU chart rendering

**Files:**
- Modify: `src/components/chart.rs` (legend width 0 means no legend, sparkline helper), `src/components/cpu.rs` (`draw_chart`, title in `draw`)
- Test: `src/components/chart.rs`, `src/components/cpu.rs`

**Interfaces:**
- Consumes: `visible_window` (Task 3), `panel_block` (Task 2).
- Produces:
  - In `chart.rs`: `pub(crate) const MIN_GRAPH_W: u16 = 12;`
  - `pub(crate) fn legend_fits(area_width: u16, legend_width: u16) -> bool`, true when `area_width >= legend_width + MIN_GRAPH_W`.
  - `pub(crate) fn spark_text(samples: &[f64], max: f64, width: usize) -> String`, which takes the last `width` samples and maps each to `▁▂▃▄▅▆▇█` by `v / max`, clamped.
  - `HistoryChart` with `legend_width == 0` renders the graph across the full area with no separator. Its early return becomes `area.width <= self.legend_width + 2 || area.height == 0`, which already holds when the width is 0.
  - In `cpu.rs`: `#[derive(Debug, PartialEq, Eq)] enum CpuTier { Wide, Medium, Narrow, Tiny }` and `fn cpu_tier(width: u16, height: u16, has_temps: bool) -> CpuTier`:
    - Tiny if `height <= 2 || width < MIN_GRAPH_W`
    - Wide if `has_temps && legend_fits(width, 19)`
    - Medium if `legend_fits(width, 12)`
    - Narrow otherwise

- [ ] **Step 1: Write failing tests.**
  ```rust
  // chart.rs
  #[test] fn spark_text_maps_to_eight_levels() {
      assert_eq!(spark_text(&[0.0, 50.0, 100.0], 100.0, 3), "▁▄█");
  }
  #[test] fn spark_text_keeps_newest_when_truncating() {
      assert_eq!(spark_text(&[100.0, 0.0, 0.0], 100.0, 2), "▁▁");
  }
  #[test] fn renders_without_legend() // snapshot "chart_no_legend", 30×6, legend_width(0)
  // cpu.rs
  #[test] fn tier_ladder() {
      assert_eq!(cpu_tier(40, 8, true), CpuTier::Wide);
      assert_eq!(cpu_tier(26, 8, true), CpuTier::Medium);   // sidebar @ 80 cols
      assert_eq!(cpu_tier(20, 8, false), CpuTier::Narrow);
      assert_eq!(cpu_tier(40, 2, false), CpuTier::Tiny);
      assert_eq!(cpu_tier(10, 8, false), CpuTier::Tiny);
  }
  ```
  For each tier, add a snapshot test that renders `CpuComponent` with `CpuSnapshot::stub()` at a size that selects it: `cpu_tier_medium` 28×10, `cpu_tier_narrow` 22×10, `cpu_tier_tiny` 30×4. Each test asserts that `has_content` is true inside the inner area. Gate the Wide snapshot with `#[cfg(target_os = "linux")]`, because temps exist only there. Review Focus items 4 and 5 add `cpu_tiers_render_without_data` (every tier size, no update, no panic) and `cpu_tiny_with_filter_matching_nothing` (set `filter = "zzz"`, render at 30×4, no panic).
- [ ] **Step 2: Run** `cargo test chart:: cpu::` → FAIL.
- [ ] **Step 3: Implement** the helpers in `chart.rs` and the `legend_width == 0` path.
- [ ] **Step 4: Implement the tiers in `cpu.rs`.** `draw_chart` replaces its early return with `match cpu_tier(area.width, area.height, has_temps)`:
  - Wide/Medium: today's path, with `label_inner_w` 18 or 11.
  - Narrow: `legend_width(0)`, no legend entries.
  - Tiny: one `Line` per visible core, `format!("cpu{:<2}{:>4.0}% ", idx, pct)` followed by `spark_text(&hist, 100.0, remaining_w)`, colored with `core_color(idx)`.
  
  In `draw`, when the tier is Narrow or Tiny and a snapshot exists, the title rest becomes `format!("PU {:.0}%", snap.aggregate)`, plus the existing filter suffix. The tier must be computed from `chart_area` before the block is built, so compute the layout from `block.inner(area)` first.
- [ ] **Step 5: Run** `INSTA_UPDATE=always cargo test`. Review the new snapshots: none of them is an empty bordered box.
- [ ] **Step 6: Commit.** `jj commit -m 'feat: tiered CPU chart rendering for narrow panels'`

### Task 5: Legend fallback for Net and Disk charts

**Files:**
- Modify: `src/components/net.rs:535-575` (`draw_compact_chart`), `:705-730` (detail graph), `src/components/disk.rs:565-590` (detail graph)
- Test: `src/components/net.rs`, `src/components/disk.rs`

**Interfaces:**
- Consumes: `legend_fits`, `visible_window`, `window_max`, `legend_width(0)` (Tasks 3–4).

- [ ] **Step 1: Write failing snapshot tests.** `net_compact_chart_narrow` renders the list view at 22×14. Inner width is 20 < 13+12, so today the chart rows are blank. Assert that `has_content` is true in the chart rows (inner rows 0–3). Add `net_detail_narrow` (22×16) and `disk_detail_narrow` (20×16) with the same assertion on the graph section.
- [ ] **Step 2: Run** `cargo test net:: disk::` → FAIL.
- [ ] **Step 3: Implement.** `let legend_w = if legend_fits(area.width, NET_LEGEND_W) { NET_LEGEND_W } else { 0 };` (and `DISK_LEGEND_W` for Disk). Pass `legend_w` to both `visible_window` and `.legend_width()`, and add the legend entries only when `legend_w > 0`. Remove Net's `area.width <= NET_LEGEND_W + 4` early return and keep the `tx_hist.is_empty()` check. The rates stay readable without the legend: the list columns in compact view, the summary line in detail view.
- [ ] **Step 4: Run** `INSTA_UPDATE=always cargo test` and review the diffs.
- [ ] **Step 5: Commit.** `jj commit -m 'feat: drop chart legend in narrow Net and Disk panels'`

### Task 6: Minimum sizes and `fit_layout`

**Files:**
- Modify: `src/components/mod.rs` (`impl ComponentId`), `src/layout.rs` (new fn and tests)
- Test: `src/layout.rs`

**Interfaces:**
- Produces:
  - `impl ComponentId { pub fn min_size(self) -> (u16, u16) }`, returning (width, height) including borders. Starting values: `Process (40, 6)`, `Cpu (16, 3)`, `Net (30, 5)`, `Disk (30, 5)`, `StatusBar (0, 0)`. Tune them in Task 8.
  - `pub const DROP_ORDER: [ComponentId; 4] = [Disk, Net, Cpu, Process];` (first entry is dropped first).
  - `pub struct FitResult { pub placed: Vec<(ComponentId, Rect)>, pub dropped: Vec<ComponentId> }`
  - `pub fn fit_layout(preset: LayoutPreset, area: Rect, overrides: &SlotOverrides, hints: &LayoutHints, visible: &[ComponentId], focused: ComponentId) -> FitResult`
- Algorithm:
  1. Start with `candidates = visible`. If there are 4 candidates, use `preset.compute`, keeping only slots whose component is in `candidates`. Otherwise use `compute_adaptive(area, &candidates)`.
  2. If every placed rect meets its `min_size`, return.
  3. Otherwise remove the first `DROP_ORDER` entry that is in `candidates` and is not `focused`, push it to `dropped`, and recompute with `compute_adaptive`, keeping `visible` order.
  4. Stop when 1 candidate remains and return it filling `area`, even if it is below its minimum (the too-small check in Task 7 covers that case).

- [ ] **Step 1: Write failing tests** in `layout.rs`:
  ```rust
  #[test] fn fit_keeps_all_four_at_200x50()   // dropped empty, placed.len() == 4
  #[test] fn fit_drops_disk_first()            // Sidebar, area where only Disk slot < (30,5); dropped == [Disk]
  #[test] fn fit_never_drops_focused()         // focused = Disk at 60×20 → Disk in placed
  #[test] fn fit_falls_back_to_focused_only()  // 40×8 → placed == [(focused, area)], dropped.len() == 3
  #[test] fn fit_respects_minimums_when_not_single() // for every preset × size matrix: if placed.len() > 1, every rect >= min_size
  #[test] fn fit_adaptive_when_fewer_than_four_visible() // visible [Cpu, Net] at 200×50 → same rects as compute_adaptive
  ```
- [ ] **Step 2: Run** `cargo test layout::` → FAIL.
- [ ] **Step 3: Implement** `min_size`, `DROP_ORDER`, `FitResult` and `fit_layout`. Leave `LayoutPreset::compute` and `compute_adaptive` unchanged.
- [ ] **Step 4: Run** `cargo test layout::` → PASS.
- [ ] **Step 5: Commit.** `jj commit -m 'feat: add fit_layout with per-component minimum sizes'`

### Task 7: Wire `fit_layout` into the app

**Files:**
- Modify: `src/app.rs` (`render_to`, focus-key guard in `handle_key_event`, new `dropped_ids` field), `src/components/status_bar.rs` (bottom-title hints), `USER_GUIDE.md`, `ARCHITECTURE.md`
- Test: `src/app.rs`, `src/components/status_bar.rs`

**Interfaces:**
- Consumes: `fit_layout`, `FitResult`, `ComponentId::min_size` (Task 6).
- Produces:
  - `App.dropped_ids: Vec<ComponentId>`
  - `StatusBarComponent::set_dropped(&mut self, hints: Vec<(char, ComponentId)>)`. It renders `.title_bottom(" +[N]et +[D]isk ")` styled like `keyed_title`, and renders nothing when the list is empty.
  - `const MIN_TERMINAL: (u16, u16) = (40, 10);` in `app.rs`.

- [ ] **Step 1: Write failing tests** in `app.rs`, using `feed_stubs` and `TestBackend`:
  ```rust
  #[test] fn small_terminal_drops_low_priority_panels()   // 60×20 sidebar: dropped contains Disk; rendered_ids excludes it
  #[test] fn focus_key_for_dropped_panel_brings_it_back()  // 60×20, render, press 'd' → FocusComponent(Disk); render again → Disk in rendered_ids, another id in dropped_ids
  #[test] fn tab_skips_dropped_panels()
  #[test] fn too_small_terminal_shows_message()             // 39×10 → buffer contains "terminal too small (need 40×10)"
  #[test] fn status_bar_hidden_when_content_would_be_under_10_rows() // 80×15 → no status bar border on row 0
  #[test] fn fullscreen_with_dropped_panels_renders_focused_modal() // 60×20, focus Disk (forces drop of another), FullScreen(Disk) → modal has content
  ```
  In `status_bar.rs`, add `renders_dropped_hints` as a snapshot test with `set_dropped(vec![('n', Net), ('d', Disk)])`.
- [ ] **Step 2: Run** `cargo test app:: status_bar::` → FAIL.
- [ ] **Step 3: Implement `render_to`** in this order:
  1. Read `terminal.size().context("reading terminal size")?`.
  2. If the size is below `MIN_TERMINAL`, draw only a centered `Paragraph` with `terminal too small (need 40×10)` and return.
  3. Show the status bar only if `total.height - status_height >= 10`.
  4. Call `fit_layout(preset, content_area, …, &visible, focused_id)`.
  5. Set `self.rendered_ids` from `placed`, ordered by `self.components`, and `self.dropped_ids` from `dropped`. Call `status_bar.set_dropped(...)`, mapping each id to its focus key from `config.keybindings.focus_{proc,cpu,net,disk}`.
  6. Call `terminal.draw` and draw the `placed` rects. Overlays are unchanged.

  This replaces both the zero-size `probe_map` hack and the separate adaptive branch.
- [ ] **Step 4: Change the focus-key guard** in `handle_key_event` to accept an id that is in `rendered_ids` or in `dropped_ids`. Keep the pre-first-render fallback to `visible`.
- [ ] **Step 5: Run** `cargo test` → PASS. Existing tests such as `normal_mode_renders_full_layout` run at 200 columns and must keep passing unchanged.
- [ ] **Step 6: Update the docs.** In `USER_GUIDE.md`, add a section on small terminals (drop order, `+[X]` hints, focus key restores a panel, 40×10 floor, auto-hidden status bar). In `ARCHITECTURE.md`, replace the probe description in the layout section with `fit_layout`. In `AGENTS.md`, update the "Key dispatch" paragraph about `rendered_ids`.
- [ ] **Step 7: Commit.** `jj commit -m 'feat: drop low-priority panels on small terminals'`

### Task 8: Size-matrix snapshot tests

**Files:**
- Modify: `src/app.rs` (tests), `src/layout.rs` (`min_size` values, if tuning is needed)
- Test: `src/snapshots/` (new `app` snapshots)

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Add** `fn render_matrix(preset: LayoutPreset)`. It builds `make_app()`, sets `app.preset = preset`, runs `feed_stubs`, renders at each of `200×50, 120×40, 80×24, 60×20, 40×12, 30×8` and calls `assert_snapshot!(format!("{preset}_{w}x{h}"), terminal.backend())`. Add one `#[test]` per preset (`size_matrix_sidebar`, `…_classic`, `…_dashboard`, `…_grid`). Gate them with `#[cfg(target_os = "linux")]`, because the status bar and CPU temps differ by platform (see `d2d0a63`).
- [ ] **Step 2: Run** `INSTA_UPDATE=always cargo test size_matrix`, then read every new snapshot. Acceptance: no unreadable sliver panels, no empty bordered box while data exists, `30×8` shows the too-small message. Where a panel at its minimum is unreadable, raise its `min_size` in `ComponentId::min_size`, rerun, and keep `fit_respects_minimums_when_not_single` passing.
- [ ] **Step 3: Run** `cargo test` → PASS.
- [ ] **Step 4: Commit.** `jj commit -m 'test: size-matrix snapshots for every layout preset'`

### Task 9: Coalesce resize bursts

**Files:**
- Modify: `src/tui.rs:95-120` (`event_loop`)

**Interfaces:**
- No public API change. `Event::Resize(w, h)` is still emitted, now at most once per quiet period of 25 ms.

- [ ] **Step 1: Implement.** Add `const RESIZE_DEBOUNCE: Duration = Duration::from_millis(25);`. In `event_loop`, a crossterm `Resize(x, y)` stores `pending = Some((x, y))` and sets `deadline = Instant::now() + RESIZE_DEBOUNCE` instead of sending right away. Add a `tokio::select!` branch `_ = sleep_until(deadline), if pending.is_some()` that sends `Event::Resize` with the latest size and clears `pending`. `App`'s `Action::Resize` handling stays as it is.
- [ ] **Step 2: Verify manually** (crossterm's `EventStream` can't be injected in unit tests). Run `cargo run`, drag-resize the window quickly, then check `~/.local/share/dreidel/dreidel.log` with debug logging on: one `action: Resize` per pause instead of dozens. The final layout matches the final size.
- [ ] **Step 3: Run** `cargo test` → PASS.
- [ ] **Step 4: Commit.** `jj commit -m 'perf: debounce terminal resize events'`
