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
        let [graph_area, legend_area] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(self.legend_width)])
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
            .y_axis(Axis::default().bounds(self.y_bounds).style(self.axis_style))
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
