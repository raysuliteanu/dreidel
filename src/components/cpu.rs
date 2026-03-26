use std::collections::VecDeque;

use anyhow::Result;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, Gauge, Sparkline},
    Frame,
};

use crate::{
    action::Action,
    components::Component,
    stats::snapshots::CpuSnapshot,
    theme::ColorPalette,
};

/// Maximum number of aggregate CPU samples retained for the sparkline.
pub const HISTORY_LEN: usize = 100;

#[derive(Debug)]
pub struct CpuComponent {
    palette: ColorPalette,
    latest: Option<CpuSnapshot>,
    /// Aggregate CPU usage history (0–100) for the sparkline.
    pub history: VecDeque<u64>,
}

impl Default for CpuComponent {
    fn default() -> Self {
        Self {
            palette: ColorPalette::dark(),
            latest: None,
            history: VecDeque::new(),
        }
    }
}

impl CpuComponent {
    pub fn new(palette: ColorPalette) -> Self {
        Self {
            palette,
            ..Default::default()
        }
    }
}

impl Component for CpuComponent {
    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        if let Action::CpuUpdate(snap) = action {
            if self.history.len() >= HISTORY_LEN {
                self.history.pop_front();
            }
            self.history.push_back(snap.aggregate as u64);
            self.latest = Some(snap);
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let block = Block::default()
            .title(" CPU ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(self.palette.border));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let Some(snap) = &self.latest else {
            return Ok(());
        };

        // Top rows: sparkline for aggregate history; bottom: per-core gauges
        let rows =
            Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]).split(inner);

        let data: Vec<u64> = self.history.iter().copied().collect();
        let sparkline = Sparkline::default()
            .data(&data)
            .style(Style::new().fg(self.palette.accent))
            .max(100);
        frame.render_widget(sparkline, rows[0]);

        // Render up to 8 per-core gauges so the panel doesn't overflow on many-core systems.
        let n = snap.per_core.len().min(8);
        if n == 0 {
            return Ok(());
        }
        let constraints: Vec<Constraint> = (0..n).map(|_| Constraint::Length(1)).collect();
        let core_rows = Layout::vertical(constraints).split(rows[1]);
        for (i, (pct, rect)) in snap.per_core.iter().zip(core_rows.iter()).enumerate() {
            let gauge = Gauge::default()
                .ratio((*pct as f64 / 100.0).clamp(0.0, 1.0))
                .label(format!("c{:<2} {:>5.1}%", i, pct))
                .gauge_style(Style::new().fg(self.palette.accent));
            frame.render_widget(gauge, *rect);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{action::Action, stats::snapshots::CpuSnapshot};
    use insta::assert_snapshot;
    use ratatui::{backend::TestBackend, Terminal};

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
    fn history_ring_buffer_bounded() {
        let mut comp = CpuComponent::default();
        for _ in 0..200 {
            comp.update(Action::CpuUpdate(CpuSnapshot::stub())).unwrap();
        }
        assert!(comp.history.len() <= HISTORY_LEN);
    }
}
