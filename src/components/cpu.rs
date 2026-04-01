// SPDX-License-Identifier: GPL-3.0-only

use std::collections::VecDeque;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType},
};

use crate::{
    action::Action,
    components::{Component, SERIES_COLORS, keyed_title},
    stats::snapshots::CpuSnapshot,
    theme::ColorPalette,
};

pub const HISTORY_LEN: usize = 100;

fn core_color(idx: usize) -> Color {
    SERIES_COLORS[idx % SERIES_COLORS.len()]
}

#[derive(Debug)]
pub struct CpuComponent {
    palette: ColorPalette,
    focus_key: char,
    latest: Option<CpuSnapshot>,
    /// Per-core usage history (0.0–100.0). Oldest at front, newest at back.
    pub per_core_history: Vec<VecDeque<f64>>,
    scroll_offset: usize,
    focused: bool,
    is_fullscreen: bool,
}

impl Default for CpuComponent {
    fn default() -> Self {
        Self {
            palette: ColorPalette::dark(),
            focus_key: 'c',
            latest: None,
            per_core_history: Vec::new(),
            scroll_offset: 0,
            focused: false,
            is_fullscreen: false,
        }
    }
}

impl CpuComponent {
    pub fn new(palette: ColorPalette, focus_key: char) -> Self {
        Self {
            palette,
            focus_key,
            ..Default::default()
        }
    }

    fn num_cores(&self) -> usize {
        self.latest.as_ref().map(|s| s.per_core.len()).unwrap_or(0)
    }

    /// Clamp scroll_offset so the last visible row never exceeds the last core.
    fn clamp_scroll(&mut self, visible: usize) {
        let n = self.num_cores();
        if n == 0 || visible >= n {
            self.scroll_offset = 0;
        } else {
            self.scroll_offset = self.scroll_offset.min(n - visible);
        }
    }

    fn draw_header(&self, frame: &mut Frame, area: Rect, snap: &CpuSnapshot) {
        let dim = Style::new().fg(self.palette.dim);
        let val = Style::new().fg(self.palette.fg);
        let ac = Style::new().fg(self.palette.accent);

        let [row0, row1] =
            Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);

        // Row 0: aggregate %, brand, logical core count (+ physical on Linux)
        let logical = snap.per_core.len();
        let mut row0_spans = vec![
            Span::styled("CPU: ", dim),
            Span::styled(format!("{:.1}%", snap.aggregate), ac),
            Span::styled("  Brand: ", dim),
            Span::styled(snap.cpu_brand.clone(), val),
            Span::styled("  Cores: ", dim),
            Span::styled(format!("{logical}"), val),
        ];
        #[cfg(target_os = "linux")]
        if let Some(phys) = snap.physical_core_count {
            row0_spans.push(Span::styled("/", dim));
            row0_spans.push(Span::styled(format!("{phys} phys"), val));
        }
        frame.render_widget(Line::from(row0_spans), row0);

        // Row 1: avg frequency, then Linux-only temperature and governor
        let avg_freq = if snap.frequency.is_empty() {
            0
        } else {
            snap.frequency.iter().sum::<u64>() / snap.frequency.len() as u64
        };
        let mut row1_spans = vec![
            Span::styled("Freq: ", dim),
            Span::styled(format!("{avg_freq} MHz"), val),
        ];
        #[cfg(target_os = "linux")]
        {
            if let Some(temp) = snap.temperature {
                row1_spans.push(Span::styled("  Temp: ", dim));
                row1_spans.push(Span::styled(format!("{temp:.0}°C"), val));
            }
            if let Some(ref gov) = snap.governor {
                row1_spans.push(Span::styled("  Governor: ", dim));
                row1_spans.push(Span::styled(gov.clone(), val));
            }
        }
        frame.render_widget(Line::from(row1_spans), row1);
    }

    fn draw_chart(&mut self, frame: &mut Frame, area: Rect, snap: &CpuSnapshot) {
        // Label column: "cpu00  100%" = 11 chars inner + 1 for Borders::LEFT = 12 total.
        const LABEL_INNER_W: u16 = 11;
        const LABEL_TOTAL_W: u16 = LABEL_INNER_W + 1;

        if area.width <= LABEL_TOTAL_W + 4 {
            return;
        }

        let [graph_area, label_area] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(LABEL_TOTAL_W)])
                .areas(area);

        let n_cores = snap.per_core.len();
        // Borders::LEFT only reduces width, not height.
        let visible = label_area.height as usize;
        self.clamp_scroll(visible);
        let first = self.scroll_offset;
        let last = n_cores.min(first + visible);

        // Build data vecs before constructing datasets; datasets borrow them.
        let hist_len = HISTORY_LEN as f64;
        let core_data: Vec<Vec<(f64, f64)>> = (first..last)
            .map(|i| {
                let hist = &self.per_core_history[i];
                let n = hist.len();
                // Right-align: newest sample sits at x = HISTORY_LEN - 1.
                hist.iter()
                    .enumerate()
                    .map(|(j, &v)| (hist_len - n as f64 + j as f64, v))
                    .collect()
            })
            .collect();

        let datasets: Vec<Dataset> = (first..last)
            .zip(core_data.iter())
            .map(|(i, data)| {
                Dataset::default()
                    .marker(symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::new().fg(core_color(i)))
                    .data(data)
            })
            .collect();

        let chart = Chart::new(datasets)
            .x_axis(
                Axis::default()
                    .bounds([0.0, hist_len])
                    .style(Style::new().fg(self.palette.dim)),
            )
            .y_axis(
                Axis::default()
                    .bounds([0.0, 100.0])
                    .style(Style::new().fg(self.palette.dim)),
            );
        frame.render_widget(chart, graph_area);

        // Left border of the label block acts as the y-axis separator.
        let label_block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::new().fg(self.palette.border));
        let label_inner = label_block.inner(label_area);
        frame.render_widget(label_block, label_area);

        let actual_visible = (last - first).min(label_inner.height as usize);
        if actual_visible == 0 {
            return;
        }
        let label_rows = Layout::vertical(
            (0..actual_visible)
                .map(|_| Constraint::Length(1))
                .collect::<Vec<_>>(),
        )
        .split(label_inner);

        for (row_idx, core_idx) in (first..first + actual_visible).enumerate() {
            let pct = snap.per_core[core_idx];
            let label = Span::styled(
                // "cpu00  100%" — index padded to 2, pct right-aligned in 5, one decimal
                format!("cpu{:<2}{:>5.1}%", core_idx, pct),
                Style::new().fg(core_color(core_idx)),
            );
            frame.render_widget(label, label_rows[row_idx]);
        }
    }
}

impl Component for CpuComponent {
    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn preferred_height(&self) -> Option<u16> {
        // 2 borders + one row per core, capped at 8 for the compact layout hint.
        let cores = self.num_cores().min(8);
        Some(2 + cores as u16)
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        let n = self.num_cores();
        match key.code {
            KeyCode::Up => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            KeyCode::Down => {
                if n > 0 {
                    self.scroll_offset = (self.scroll_offset + 1).min(n - 1);
                }
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(8);
            }
            KeyCode::PageDown => {
                if n > 0 {
                    self.scroll_offset = (self.scroll_offset + 8).min(n - 1);
                }
            }
            _ => {}
        }
        Ok(None)
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::CpuUpdate(snap) => {
                let n = snap.per_core.len();
                while self.per_core_history.len() < n {
                    self.per_core_history.push(VecDeque::new());
                }
                for (i, &pct) in snap.per_core.iter().enumerate() {
                    let hist = &mut self.per_core_history[i];
                    if hist.len() >= HISTORY_LEN {
                        hist.pop_front();
                    }
                    hist.push_back(pct as f64);
                }
                self.latest = Some(snap);
            }
            Action::ToggleFullScreen => {
                self.is_fullscreen = !self.is_fullscreen;
            }
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let border_color = if self.focused {
            self.palette.accent
        } else {
            self.palette.border
        };
        let title: Line = keyed_title(self.focus_key, "PU", &self.palette);
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border_color));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let Some(snap) = self.latest.clone() else {
            return Ok(());
        };

        // Show stats header only in fullscreen when there's enough room.
        let show_header = self.is_fullscreen && inner.height >= 6;
        let header_h: u16 = if show_header { 2 } else { 0 };
        let sep_h: u16 = if show_header { 1 } else { 0 };

        let [header_area, sep_area, chart_area] = Layout::vertical([
            Constraint::Length(header_h),
            Constraint::Length(sep_h),
            Constraint::Fill(1),
        ])
        .areas(inner);

        if show_header {
            self.draw_header(frame, header_area, &snap);
            frame.render_widget(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::new().fg(self.palette.border)),
                sep_area,
            );
        }

        if chart_area.height == 0 || chart_area.width == 0 {
            return Ok(());
        }

        self.draw_chart(frame, chart_area, &snap);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{action::Action, stats::snapshots::CpuSnapshot};
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn renders_without_data() {
        let mut comp = CpuComponent::default();
        let mut terminal = Terminal::new(TestBackend::new(60, 10)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("cpu_no_data", terminal.backend());
    }

    #[test]
    fn renders_with_cpu_data() {
        let mut comp = CpuComponent::default();
        comp.update(Action::CpuUpdate(CpuSnapshot::stub())).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(60, 10)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("cpu_with_data", terminal.backend());
    }

    #[test]
    fn renders_fullscreen_header() {
        let mut comp = CpuComponent::default();
        comp.update(Action::CpuUpdate(CpuSnapshot::stub())).unwrap();
        comp.update(Action::ToggleFullScreen).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("cpu_fullscreen", terminal.backend());
    }

    #[test]
    fn history_ring_buffer_bounded() {
        let mut comp = CpuComponent::default();
        for _ in 0..200 {
            comp.update(Action::CpuUpdate(CpuSnapshot::stub())).unwrap();
        }
        for hist in &comp.per_core_history {
            assert!(hist.len() <= HISTORY_LEN);
        }
    }

    #[test]
    fn scroll_clamps_to_valid_range() {
        let mut comp = CpuComponent::default();
        comp.update(Action::CpuUpdate(CpuSnapshot::stub())).unwrap(); // 4 cores
        // Scroll far past the end
        comp.update(Action::ToggleFullScreen).unwrap();
        for _ in 0..100 {
            comp.handle_key_event(crossterm::event::KeyEvent::new(
                KeyCode::Down,
                crossterm::event::KeyModifiers::NONE,
            ))
            .unwrap();
        }
        assert!(comp.scroll_offset < comp.num_cores());
    }

    #[test]
    fn core_color_cycles() {
        // Colors cycle at SERIES_COLORS.len() (32).
        assert_eq!(core_color(0), core_color(SERIES_COLORS.len()));
        assert_ne!(core_color(0), core_color(1));
    }
}
