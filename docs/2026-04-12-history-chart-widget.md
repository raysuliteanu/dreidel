# HistoryChart Widget Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract the duplicated right-aligned braille line chart + right-side legend pattern from CPU, Net, and Disk into a single `HistoryChart` custom ratatui `Widget`.

**Architecture:** A new `src/components/chart.rs` module defines `HistoryChart<'a>`, `LegendEntry<'a>`, and `LegendAnchor`. The widget implements `ratatui::widgets::Widget` and handles: right-aligning series data so "now" is at the right edge, rendering a braille `Chart`, splitting the area into graph + legend, drawing a `Borders::LEFT` separator, and placing legend entries at anchored vertical positions. Each component (CPU, Net, Disk) replaces its hand-rolled chart code with a `HistoryChart` builder call.

**Tech Stack:** Rust, ratatui 0.30 (`Widget` trait, `Buffer`, `Chart`, `Dataset`, `Block`), insta (snapshot testing)

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `src/components/chart.rs` | **Create** | `HistoryChart`, `LegendEntry`, `LegendAnchor`, `Widget` impl, unit tests |
| `src/components/mod.rs` | **Modify** | Add `pub(crate) mod chart;` declaration |
| `src/components/cpu.rs` | **Modify** | Replace `draw_chart` internals with `HistoryChart` |
| `src/components/net.rs` | **Modify** | Replace `draw_compact_chart` + `draw_detail` graph sections with `HistoryChart`; remove `LEGEND_INNER_W`/`LEGEND_TOTAL_W` constants |
| `src/components/disk.rs` | **Modify** | Replace `draw_detail` graph section with `HistoryChart`; remove `LEGEND_INNER_W`/`LEGEND_TOTAL_W` constants |

---

### Task 1: Create `chart.rs` with types and `Widget` impl

**Files:**
- Create: `src/components/chart.rs`
- Modify: `src/components/mod.rs:23` (add module declaration)

- [ ] **Step 1: Add module declaration to `mod.rs`**

In `src/components/mod.rs`, add after line 23 (`pub mod cpu;`):

```rust
pub(crate) mod chart;
```

- [ ] **Step 2: Create `src/components/chart.rs` with types**

```rust
// SPDX-License-Identifier: GPL-3.0-only

//! [`HistoryChart`] — a reusable right-aligned braille line chart with a
//! right-side legend, shared by the CPU, Net, and Disk panels.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Style,
    symbols,
    text::Span,
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Widget},
};

/// Vertical anchor for a legend entry inside the right-side label column.
#[derive(Debug, Clone, Copy)]
pub(crate) enum LegendAnchor {
    /// Stacks downward from row 0. Multiple `Top` entries fill rows 0, 1, 2, …
    Top,
    /// Placed at the vertical midpoint of the legend area.
    Center,
    /// Stacks upward from the last row. Multiple `Bottom` entries fill h−1, h−2, …
    Bottom,
}

/// A single label rendered in the right-side legend column.
#[derive(Debug, Clone)]
pub(crate) struct LegendEntry<'a> {
    span: Span<'a>,
    anchor: LegendAnchor,
}

impl<'a> LegendEntry<'a> {
    pub(crate) fn top(span: Span<'a>) -> Self {
        Self {
            span,
            anchor: LegendAnchor::Top,
        }
    }

    pub(crate) fn center(span: Span<'a>) -> Self {
        Self {
            span,
            anchor: LegendAnchor::Center,
        }
    }

    pub(crate) fn bottom(span: Span<'a>) -> Self {
        Self {
            span,
            anchor: LegendAnchor::Bottom,
        }
    }
}

/// A right-aligned braille line chart with a right-side legend column.
///
/// Handles the shared rendering pattern used by the CPU, Net, and Disk panels:
/// 1. Right-aligns series data so the newest sample sits at the right edge.
/// 2. Renders a braille `Chart` with no axis labels.
/// 3. Splits the area into graph + legend separated by a `Borders::LEFT` line.
/// 4. Places legend entries at anchored vertical positions (top / center / bottom).
pub(crate) struct HistoryChart<'a> {
    history_len: usize,
    series: Vec<(Vec<f64>, Style)>,
    y_bounds: [f64; 2],
    legend_entries: Vec<LegendEntry<'a>>,
    legend_width: u16,
    border_style: Style,
    axis_style: Style,
}

impl<'a> HistoryChart<'a> {
    pub(crate) fn new(history_len: usize) -> Self {
        Self {
            history_len,
            series: Vec::new(),
            y_bounds: [0.0, 100.0],
            legend_entries: Vec::new(),
            legend_width: 1,
            border_style: Style::default(),
            axis_style: Style::default(),
        }
    }

    /// Add a data series. `data` is consumed and right-aligned during render.
    pub(crate) fn series(mut self, data: impl IntoIterator<Item = f64>, style: Style) -> Self {
        self.series.push((data.into_iter().collect(), style));
        self
    }

    pub(crate) fn y_bounds(mut self, min: f64, max: f64) -> Self {
        self.y_bounds = [min, max];
        self
    }

    /// Total width of the right-side legend column including the `Borders::LEFT`
    /// separator (i.e. inner label width + 1).
    pub(crate) fn legend_width(mut self, w: u16) -> Self {
        self.legend_width = w;
        self
    }

    pub(crate) fn legend(mut self, entry: LegendEntry<'a>) -> Self {
        self.legend_entries.push(entry);
        self
    }

    pub(crate) fn border_style(mut self, style: Style) -> Self {
        self.border_style = style;
        self
    }

    pub(crate) fn axis_style(mut self, style: Style) -> Self {
        self.axis_style = style;
        self
    }
}

impl Widget for HistoryChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width <= self.legend_width + 2 || area.height == 0 {
            return;
        }

        // --- Area split ---
        let [graph_area, legend_area] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(self.legend_width),
        ])
        .areas(area);

        // --- Right-align series data ---
        let x_max = self.history_len as f64;
        let aligned: Vec<Vec<(f64, f64)>> = self
            .series
            .iter()
            .map(|(data, _)| {
                let n = data.len();
                data.iter()
                    .enumerate()
                    .map(|(j, &v)| (x_max - n as f64 + j as f64, v))
                    .collect()
            })
            .collect();

        // --- Chart ---
        let datasets: Vec<Dataset<'_>> = self
            .series
            .iter()
            .zip(aligned.iter())
            .map(|((_, style), data)| {
                Dataset::default()
                    .marker(symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(*style)
                    .data(data)
            })
            .collect();

        Chart::new(datasets)
            .x_axis(Axis::default().bounds([0.0, x_max]).style(self.axis_style))
            .y_axis(
                Axis::default()
                    .bounds(self.y_bounds)
                    .style(self.axis_style),
            )
            .render(graph_area, buf);

        // --- Legend separator ---
        let legend_block = Block::default()
            .borders(Borders::LEFT)
            .border_style(self.border_style);
        let legend_inner = legend_block.inner(legend_area);
        legend_block.render(legend_area, buf);

        // --- Legend entries ---
        let h = legend_inner.height;
        if h == 0 {
            return;
        }
        let w = legend_inner.width;

        let mut top_row: u16 = 0;
        let mut bottom_row: u16 = h.saturating_sub(1);

        for entry in &self.legend_entries {
            let row = match entry.anchor {
                LegendAnchor::Top => {
                    let r = top_row;
                    top_row = top_row.saturating_add(1);
                    r
                }
                LegendAnchor::Center => h / 2,
                LegendAnchor::Bottom => {
                    let r = bottom_row;
                    bottom_row = bottom_row.saturating_sub(1);
                    r
                }
            };

            if row < h {
                buf.set_span(legend_inner.x, legend_inner.y + row, &entry.span, w);
            }
        }
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | head -5`
Expected: compiles with no errors (possibly an unused warning for the new module — that is fine)

- [ ] **Step 4: Commit**

```
jj commit -m "feat: add HistoryChart custom widget with legend anchoring"
```

---

### Task 2: Add unit tests for `HistoryChart`

**Files:**
- Modify: `src/components/chart.rs` (append `#[cfg(test)]` module)

- [ ] **Step 1: Write tests for the widget**

Append to the bottom of `src/components/chart.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend, style::Color};

    fn dim() -> Style {
        Style::new().fg(Color::DarkGray)
    }

    fn border() -> Style {
        Style::new().fg(Color::Gray)
    }

    #[test]
    fn renders_single_series_with_top_legend() {
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        terminal
            .draw(|f| {
                let chart = HistoryChart::new(100)
                    .series(vec![50.0; 20], Style::new().fg(Color::Cyan))
                    .y_bounds(0.0, 100.0)
                    .legend_width(11)
                    .legend(LegendEntry::top(Span::styled(
                        "cpu0  50.0%",
                        Style::new().fg(Color::Cyan),
                    )))
                    .border_style(border())
                    .axis_style(dim());
                f.render_widget(chart, f.area());
            })
            .unwrap();
        assert_snapshot!("chart_single_series_top", terminal.backend());
    }

    #[test]
    fn renders_two_series_with_top_center_bottom_legend() {
        let mut terminal = Terminal::new(TestBackend::new(50, 10)).unwrap();
        terminal
            .draw(|f| {
                let chart = HistoryChart::new(100)
                    .series(
                        (0..30).map(|i| (i * 100) as f64),
                        Style::new().fg(Color::Green),
                    )
                    .series(
                        (0..30).map(|i| (i * 50) as f64),
                        Style::new().fg(Color::Yellow),
                    )
                    .y_bounds(0.0, 3000.0)
                    .legend_width(11)
                    .legend(LegendEntry::top(Span::styled("3000", dim())))
                    .legend(LegendEntry::center(Span::styled("1500", dim())))
                    .legend(LegendEntry::bottom(Span::styled("0", dim())))
                    .border_style(border())
                    .axis_style(dim());
                f.render_widget(chart, f.area());
            })
            .unwrap();
        assert_snapshot!("chart_two_series_anchored", terminal.backend());
    }

    #[test]
    fn renders_empty_series_without_panic() {
        let mut terminal = Terminal::new(TestBackend::new(30, 6)).unwrap();
        terminal
            .draw(|f| {
                let chart = HistoryChart::new(100)
                    .y_bounds(0.0, 100.0)
                    .legend_width(11)
                    .border_style(border())
                    .axis_style(dim());
                f.render_widget(chart, f.area());
            })
            .unwrap();
        assert_snapshot!("chart_empty", terminal.backend());
    }

    #[test]
    fn too_narrow_renders_nothing() {
        let mut terminal = Terminal::new(TestBackend::new(5, 4)).unwrap();
        terminal
            .draw(|f| {
                let chart = HistoryChart::new(100)
                    .series(vec![50.0; 10], Style::new().fg(Color::Cyan))
                    .y_bounds(0.0, 100.0)
                    .legend_width(11)
                    .border_style(border())
                    .axis_style(dim());
                f.render_widget(chart, f.area());
            })
            .unwrap();
        assert_snapshot!("chart_too_narrow", terminal.backend());
    }
}
```

- [ ] **Step 2: Run tests and accept initial snapshots**

Run: `INSTA_UPDATE=always cargo test components::chart 2>&1 | tail -10`
Expected: all 4 tests pass, snapshot files created under `src/components/snapshots/`

- [ ] **Step 3: Verify snapshots look correct**

Visually inspect the snapshot files in `src/components/snapshots/` starting with `dreidel__components__chart__`. Confirm:
- `chart_single_series_top`: braille data right-aligned, "cpu0  50.0%" label at top-right
- `chart_two_series_anchored`: two data series, "3000" at top, "1500" at center, "0" at bottom of legend
- `chart_empty`: just the separator line and empty graph area
- `chart_too_narrow`: completely blank (early return)

- [ ] **Step 4: Commit**

```
jj commit -m "test: add snapshot tests for HistoryChart widget"
```

---

### Task 3: Migrate CPU component to `HistoryChart`

**Files:**
- Modify: `src/components/cpu.rs:192-296` (`draw_chart` method)

The CPU component's `draw_chart` currently hand-rolls the right-aligned chart + label column. Replace the internals with a `HistoryChart` builder call. The method signature and the caller (`draw`) stay unchanged.

- [ ] **Step 1: Replace `draw_chart` internals**

Replace the body of `draw_chart` (lines 200–296) with:

```rust
    fn draw_chart(
        &self,
        frame: &mut Frame,
        area: Rect,
        snap: &CpuSnapshot,
        filtered: &[usize],
        first: usize,
        last: usize,
    ) {
        #[cfg(target_os = "linux")]
        let has_temps = snap.per_core_temp.iter().any(|t| t.is_some());
        #[cfg(not(target_os = "linux"))]
        let has_temps = false;

        let label_inner_w: u16 = if has_temps { 18 } else { 11 };
        let label_total_w: u16 = label_inner_w + 1;

        if area.width <= label_total_w + 4 {
            return;
        }

        let actual_visible = (last - first).min(area.height as usize);
        if actual_visible == 0 {
            return;
        }

        let mut chart = HistoryChart::new(HISTORY_LEN)
            .y_bounds(0.0, 100.0)
            .legend_width(label_total_w)
            .border_style(Style::new().fg(self.palette.border))
            .axis_style(Style::new().fg(self.palette.dim));

        for &core_idx in &filtered[first..last] {
            chart = chart.series(
                self.per_core_history[core_idx].iter().copied(),
                Style::new().fg(core_color(core_idx)),
            );
        }

        for &core_idx in &filtered[first..first + actual_visible] {
            let pct = snap.per_core[core_idx];
            let mut text = format!("cpu{:<2}{:>5.1}%", core_idx, pct);

            #[cfg(target_os = "linux")]
            if has_temps {
                if let Some(Some(temp)) = snap.per_core_temp.get(core_idx) {
                    text.push_str(&format!(" {:>4.0}°C", temp));
                } else {
                    text.push_str("       ");
                }
            }

            chart = chart.legend(LegendEntry::top(Span::styled(
                text,
                Style::new().fg(core_color(core_idx)),
            )));
        }

        frame.render_widget(chart, area);
    }
```

- [ ] **Step 2: Update imports in `cpu.rs`**

At the top of `cpu.rs`, the `ratatui` import block currently includes `Axis`, `Chart`, `Dataset`, `GraphType`, and `Layout`/`Constraint`. These are no longer used directly. Replace the ratatui import and add the chart import:

Remove from the ratatui import: `Axis`, `Chart`, `Dataset`, `GraphType`, `Constraint`, `Layout` (if no other code in the file uses them).

Add:
```rust
use crate::components::chart::{HistoryChart, LegendEntry};
```

**Important:** Check whether `Layout`/`Constraint` are used elsewhere in `cpu.rs` (e.g. in `draw` for fullscreen header layout). If so, keep them. Only remove imports that are now unused.

- [ ] **Step 3: Verify compilation**

Run: `cargo check 2>&1 | head -10`
Expected: compiles cleanly

- [ ] **Step 4: Run existing CPU tests**

Run: `INSTA_UPDATE=always cargo test components::cpu 2>&1 | tail -15`
Expected: all tests pass. Snapshots for `cpu_with_data` and `cpu_fullscreen` may need updating since the rendering is now done by the widget (braille output might shift by a cell if legend_inner sizing differs slightly). Accept any snapshot updates and verify the diffs are cosmetic only — the chart content and label column should look the same.

- [ ] **Step 5: Commit**

```
jj commit -m "refactor: migrate CPU chart rendering to HistoryChart widget"
```

---

### Task 4: Migrate Net component to `HistoryChart`

**Files:**
- Modify: `src/components/net.rs:115-118` (remove `LEGEND_INNER_W`/`LEGEND_TOTAL_W` constants)
- Modify: `src/components/net.rs:537-628` (`draw_compact_chart` method)
- Modify: `src/components/net.rs:748-838` (graph section of `draw_detail`)

The Net component has **two** chart sites: the compact aggregate chart in list view, and the per-interface detail chart. Both follow the same pattern.

- [ ] **Step 1: Remove `LEGEND_INNER_W` / `LEGEND_TOTAL_W` constants**

Delete lines 115–118:
```rust
/// Width of the graph legend column inner area: "TX  1.2 MB/s" = 12 chars.
const LEGEND_INNER_W: u16 = 12;
/// Total legend column width including the Borders::LEFT separator.
const LEGEND_TOTAL_W: u16 = LEGEND_INNER_W + 1;
```

Add a local constant at the top of the file to keep the legend sizing:
```rust
/// Legend column: "TX " (3) + rate (9) = 12 inner + 1 border = 13 total.
const NET_LEGEND_W: u16 = 13;
```

- [ ] **Step 2: Replace `draw_compact_chart` internals**

Replace the body of `draw_compact_chart` (approximately lines 537–628) with:

```rust
    fn draw_compact_chart(&self, frame: &mut Frame, area: Rect) {
        let (tx_hist, rx_hist) = &self.agg_history;
        if tx_hist.is_empty() || area.width <= NET_LEGEND_W + 4 {
            return;
        }

        let y_max = tx_hist
            .iter()
            .chain(rx_hist.iter())
            .copied()
            .max()
            .unwrap_or(0)
            .max(MIN_CHART_FLOOR) as f64;

        let tx_cur = tx_hist.back().copied().unwrap_or(0);
        let rx_cur = rx_hist.back().copied().unwrap_or(0);
        let rate_w = (NET_LEGEND_W - 1) as usize - 3; // inner width minus "TX " prefix

        let chart = HistoryChart::new(HISTORY_LEN)
            .series(
                tx_hist.iter().map(|&v| v as f64),
                Style::new().fg(self.palette.accent),
            )
            .series(
                rx_hist.iter().map(|&v| v as f64),
                Style::new().fg(self.palette.highlight),
            )
            .y_bounds(0.0, y_max)
            .legend_width(NET_LEGEND_W)
            .legend(LegendEntry::top(Span::styled(
                format!("TX {:>rate_w$}", fmt_rate(tx_cur)),
                Style::new().fg(self.palette.accent),
            )))
            .legend(LegendEntry::top(Span::styled(
                format!("RX {:>rate_w$}", fmt_rate(rx_cur)),
                Style::new().fg(self.palette.highlight),
            )))
            .border_style(Style::new().fg(self.palette.border))
            .axis_style(Style::new().fg(self.palette.dim));

        frame.render_widget(chart, area);
    }
```

- [ ] **Step 3: Replace the graph section of `draw_detail`**

In `draw_detail`, find the `// --- Graph ---` section (approximately lines 748–838). Replace it with:

```rust
        // --- Graph ---
        let (tx_hist, rx_hist) = match self.history.get(name) {
            Some(h) => h,
            None => return Ok(()),
        };

        let y_max = tx_hist
            .iter()
            .chain(rx_hist.iter())
            .copied()
            .max()
            .unwrap_or(0)
            .max(MIN_CHART_FLOOR) as f64;

        let tx_cur = tx_hist.back().copied().unwrap_or(0);
        let rx_cur = rx_hist.back().copied().unwrap_or(0);
        let rate_w = (NET_LEGEND_W - 1) as usize - 3;

        let chart = HistoryChart::new(HISTORY_LEN)
            .series(
                tx_hist.iter().map(|&v| v as f64),
                Style::new().fg(self.palette.accent),
            )
            .series(
                rx_hist.iter().map(|&v| v as f64),
                Style::new().fg(self.palette.highlight),
            )
            .y_bounds(0.0, y_max)
            .legend_width(NET_LEGEND_W)
            .legend(LegendEntry::top(Span::styled(
                format!("TX {:>rate_w$}", fmt_rate(tx_cur)),
                Style::new().fg(self.palette.accent),
            )))
            .legend(LegendEntry::top(Span::styled(
                format!("RX {:>rate_w$}", fmt_rate(rx_cur)),
                Style::new().fg(self.palette.highlight),
            )))
            .border_style(Style::new().fg(self.palette.border))
            .axis_style(Style::new().fg(self.palette.dim));

        frame.render_widget(chart, sections[2]);
```

- [ ] **Step 4: Update imports in `net.rs`**

Remove from the ratatui import: `Axis`, `Chart`, `Dataset`, `GraphType` (if no other code in the file uses them). Keep `Layout`, `Constraint` — they are used for other layout splits in `draw`.

Add:
```rust
use crate::components::chart::{HistoryChart, LegendEntry};
```

- [ ] **Step 5: Verify compilation and run tests**

Run: `cargo check 2>&1 | head -10`
Expected: compiles cleanly

Run: `INSTA_UPDATE=always cargo test components::net 2>&1 | tail -15`
Expected: all tests pass. Accept snapshot updates and verify the chart output is visually identical.

- [ ] **Step 6: Commit**

```
jj commit -m "refactor: migrate Net chart rendering to HistoryChart widget"
```

---

### Task 5: Migrate Disk component to `HistoryChart`

**Files:**
- Modify: `src/components/disk.rs:108-111` (remove `LEGEND_INNER_W`/`LEGEND_TOTAL_W` constants)
- Modify: `src/components/disk.rs:561-648` (graph section of `draw_detail`)

- [ ] **Step 1: Remove `LEGEND_INNER_W` / `LEGEND_TOTAL_W` constants**

Delete lines 108–111:
```rust
/// Width of the y-axis label text in the detail graph legend (e.g. "102.4 KB/s" = 10 chars).
const LEGEND_INNER_W: u16 = 10;
/// Total legend column width including the Borders::LEFT separator.
const LEGEND_TOTAL_W: u16 = LEGEND_INNER_W + 1;
```

Add a local constant:
```rust
/// Legend column: rate label (10) + 1 border = 11 total.
const DISK_LEGEND_W: u16 = 11;
```

- [ ] **Step 2: Replace the graph section of `draw_detail`**

In `draw_detail`, find the `// --- Graph ---` section (approximately lines 561–648). Replace it with:

```rust
        // --- Graph ---
        let (read_hist, write_hist) = match self.history.get(name) {
            Some(h) => h,
            None => return Ok(()),
        };

        let y_max = read_hist
            .iter()
            .chain(write_hist.iter())
            .copied()
            .max()
            .unwrap_or(0)
            .max(MIN_CHART_FLOOR) as f64;

        let chart = HistoryChart::new(HISTORY_LEN)
            .series(
                read_hist.iter().map(|&v| v as f64),
                Style::new().fg(self.palette.accent),
            )
            .series(
                write_hist.iter().map(|&v| v as f64),
                Style::new().fg(self.palette.highlight),
            )
            .y_bounds(0.0, y_max)
            .legend_width(DISK_LEGEND_W)
            .legend(LegendEntry::top(Span::styled(
                fmt_rate(y_max as u64),
                Style::new().fg(self.palette.dim),
            )))
            .legend(LegendEntry::center(Span::styled(
                fmt_rate(y_max as u64 / 2),
                Style::new().fg(self.palette.dim),
            )))
            .legend(LegendEntry::bottom(Span::styled(
                "0".to_string(),
                Style::new().fg(self.palette.dim),
            )))
            .border_style(Style::new().fg(self.palette.border))
            .axis_style(Style::new().fg(self.palette.dim));

        frame.render_widget(chart, sections[2]);
```

- [ ] **Step 3: Update imports in `disk.rs`**

Remove from the ratatui import: `Axis`, `Chart`, `Dataset`, `GraphType` (if no other code in the file uses them). Keep `Layout`, `Constraint` — they are used for other layout splits.

Add:
```rust
use crate::components::chart::{HistoryChart, LegendEntry};
```

- [ ] **Step 4: Verify compilation and run tests**

Run: `cargo check 2>&1 | head -10`
Expected: compiles cleanly

Run: `INSTA_UPDATE=always cargo test components::disk 2>&1 | tail -15`
Expected: all tests pass. The `disk_graph_view` snapshot may update — accept and verify the chart output is visually identical to before.

- [ ] **Step 5: Commit**

```
jj commit -m "refactor: migrate Disk chart rendering to HistoryChart widget"
```

---

### Task 6: Clean up unused imports and run full test suite

**Files:**
- Modify: `src/components/cpu.rs` (remove unused imports if any remain)
- Modify: `src/components/net.rs` (remove unused imports if any remain)
- Modify: `src/components/disk.rs` (remove unused imports if any remain)

- [ ] **Step 1: Run clippy to catch unused imports**

Run: `cargo clippy -- -D warnings 2>&1 | head -30`

Fix any `unused_imports` warnings in `cpu.rs`, `net.rs`, or `disk.rs` by removing the flagged items from their `use ratatui::{...}` blocks.

- [ ] **Step 2: Run the full test suite**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass (unit + integration + doc tests)

- [ ] **Step 3: Run format check**

Run: `cargo fmt --check 2>&1`
Expected: no formatting issues

- [ ] **Step 4: Commit (if any cleanup was needed)**

```
jj commit -m "refactor: remove unused ratatui imports after HistoryChart migration"
```
