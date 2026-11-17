# dreidel — UI review and improvement plan

Context for Claude Code: this comes from a design discussion and a read-through of `raysuliteanu/dreidel` at HEAD on 2026-11-17 (ratatui 0.30, crossterm, tokio). None of the changes below have been compiled or run yet. Build, run the tests and review the insta snapshot diffs for each change.

## Summary of the discussion

**Is ratatui the right foundation?** We looked at alternatives:
- GPUI (Zed): GPU-only, with no terminal backend. Not an option.
- cursive: retained-mode, good for dialogs and forms, but it has no chart, sparkline or gauge widgets.
- iocraft: React-style components, no charting.
- termwiz: low-level.
- tui-realm and tuika: frameworks that sit on top of ratatui.

Conclusion: for a dashboard-style system monitor (refresh-tick redraws, time-series charts, tables, gauges), ratatui is the right fit. bottom (`btm`) reaches the same conclusion. It keeps ratatui as the buffer and layout engine and owns its key widgets: a modified Chart (TimeChart), a PipeGauge, and a DataTable with column-priority dropping. dreidel's `HistoryChart` (`src/components/chart.rs`) already follows this pattern.

**Braille markers.** Unicode U+2800–U+28FF characters are 2×4 dot cells, encoded as `0x2800 | mask`, which gives 2× horizontal and 4× vertical resolution per cell. The tradeoff is one foreground color per cell, so overlapping series collide. dreidel already uses `Marker::Braille` (`chart.rs:190`).

**Where the visual gap with bottom comes from.** It isn't the rendering library. It's the color hierarchy, the history window and density, the space the legend takes, and how panels degrade as the terminal shrinks. The degradation part is also the open todo item "handle resizing / what to show as the terminal gets smaller."

**Already fine:**
- Resize events flow `crossterm Event::Resize → Action::Resize → tui.resize() + render` (`app.rs:403`).
- `HistoryChart` is a clean shared abstraction.
- insta snapshot tests exist, which suits size-matrix testing.

## Recommended order

1 → 5 (both trivial, big visual payoff), then 2, 3, 4, and 6 last.

---

## 1. Fix the `dim` palette role (biggest visual issue)

**Where:** `src/theme.rs`

**Problem:** the dark theme `dim = Rgb(220,225,240)` is *brighter* than `fg = Rgb(192,202,245)`. In the light theme, `dim = Rgb(30,30,30)` is essentially the same as `fg = Rgb(20,20,20)`. `dim` is used in about 34 places, including chart axes (`cpu.rs` `axis_style`) and label prefixes like "Temp:", "Governor:". The result is a flat or inverted visual hierarchy.

**Change:**
- Dark: the palette is Tokyo Night. Use its comment color: `dim: Rgb(86, 95, 137)`.
- Light: use something like `dim: Rgb(120, 120, 120)`.

**Check:** grep the `palette.dim` usages. If any spot relied on `dim` being bright for readability, switch that spot to `fg` rather than keeping the role wrong.

**Acceptance:** labels and axes are visibly subordinate to values in both themes. Snapshot styles are unchanged except for colors.

## 2. Make the chart history window track the chart width

**Where:** `src/components/chart.rs` (`HistoryChart::render`), `src/components/mod.rs` (`HISTORY_LEN = 100`)

**Problem:** the x-range is fixed at 100 samples whatever the width. A 38-column chart has 76 braille dot columns, so 100 samples get squeezed. A 150-column fullscreen chart has 300 dot columns, so the samples get stretched. Line density and the time span shown vary with panel size.

**Change:**
- Raise the ring-buffer capacity, e.g. `HISTORY_LEN = 600`. It's cheap: f64 × cores.
- In `render`, after splitting off `graph_area`:
  ```rust
  let window = (graph_area.width as usize * 2).clamp(1, self.history_len);
  let x_max = window as f64;
  // per series: keep only the last `window` samples, then right-align
  let skip = data.len().saturating_sub(window);
  let pts = data.iter().skip(skip).enumerate()
      .map(|(j, &v)| (x_max - (data.len() - skip) as f64 + j as f64, v));
  ```
- Net and Disk auto-scale the y-axis. Make sure the max is computed over the *visible window*, not the whole buffer, or an old spike will flatten the visible graph.
- Update tests that assert `len() <= HISTORY_LEN` if needed.

**Acceptance:** about 2 samples per column at any width, and wider panels show more time.

## 3. Tiered rendering so the legend doesn't crowd out the chart

**Where:** `src/components/cpu.rs` (`draw_chart`, `label_inner_w` 18 with temps / 11 without), `src/components/net.rs` (`NET_LEGEND_W`, ~line 537), `src/components/disk.rs`

**Problem:** in the Sidebar preset at 80 columns, the left column is 35% (28 columns), 26 inside the borders. The Linux legend with temps takes 19 of that, leaving about 7 columns of chart. Below roughly 25 columns, `draw_chart` returns early (`area.width <= label_total_w + 4`) and the panel is an empty box.

**Change:** pick a tier from the available `Rect` instead of bailing out:
1. **Wide:** the current layout (chart + legend with temps).
2. **Medium:** drop temps from the legend (`label_inner_w` → 11).
3. **Narrow:** no legend. The chart gets the full width, and the aggregate value goes in the block title (e.g. ` [C]PU 37% `).
4. **Tiny (height ≤ 2 or very narrow):** one text line per visible core, e.g. `cpu0 42% ▃▅▂`, using `Sparkline` or plain text.

Apply the same ladder to Net (TX/RX legend) and Disk. Add one or two cells of hysteresis at tier boundaries if flicker shows up while resizing.

**Acceptance:** no panel ever renders as an empty bordered box while data exists.

## 4. Minimum sizes and graceful fallback in layout presets

**Where:** `src/layout.rs` (`LayoutPreset::split_area`, `compute_adaptive`), `src/app.rs` render path (~lines 430–500)

**Problem:** every preset splits by percentage with no floor, so on a small terminal every panel becomes a sliver.

**Change:**
- Give each component a minimum usable size, e.g. via a new `Component::min_size()` next to `preferred_height()`.
- In the layout step, if the preset's slots can't meet the minimums, drop the lowest-priority panels. Suggested priority: Process > CPU > Net > Disk. Drop them by falling back to `compute_adaptive` with the remaining components; that function already handles 1–3 panels.
- Below the size where even two panels fit, show only the focused panel (as in the existing fullscreen path).
- Show dropped panels as key hints in the status bar so `c`/`n`/`d`/`p` still reach them.
- Absolute floor: below about 40×10, render a centered "terminal too small (need 40×10)" message.

**Acceptance:** every preset degrades step by step from 200×50 down to 40×10 with no unreadable panels.

## 5. Rounded borders (cheap polish)

**Where:** `list_border_block` in `src/components/mod.rs` (~line 252), `src/components/cpu.rs` (~line 424), `src/components/process/mod.rs` (~line 619), plus the help and status-bar blocks.

**Change:** add `.border_type(BorderType::Rounded)` to the panel `Block`s. Keep the existing focused-accent / unfocused-`border` color logic. Consider factoring the panel block into a single helper so CPU and Process share it with Net and Disk.

**Acceptance:** snapshots show `╭╮╰╯` corners. Accept the snapshot updates.

## 6. Coalesce resize bursts (minor)

**Where:** `src/app.rs` (`Event::Resize` → `Action::Resize`, ~lines 256 and 403)

**Problem:** dragging a window sends a flood of resize events, and each one does `tui.resize()` plus a full render.

**Change:** on `Action::Resize`, record the latest size and set a dirty flag. Render once per tick, or after a short debounce (~16–30 ms), using the latest size. ratatui's `draw` also auto-resizes, so the explicit `resize` call can stay as it is or be folded into the debounced render.

**Acceptance:** smooth live resizing with no visible lag or tearing.

---

## Testing approach for 3 and 4

Add insta snapshot tests that render each layout preset, and each chart panel on its own, through `TestBackend` at a fixed matrix of sizes, e.g. `200×50, 120×40, 80×24, 60×20, 40×12, 30×8`. This captures the degradation behavior and keeps future changes reviewable as snapshot diffs.
