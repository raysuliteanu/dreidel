use anyhow::Result;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph},
};

use crate::{
    action::Action, components::Component, stats::snapshots::MemSnapshot, theme::ColorPalette,
};

// Right-column width: fits "SWAP  xx.x/xx.x GiB  xx.x%" with some margin.
const LABEL_WIDTH: u16 = 28;

#[derive(Debug)]
pub struct MemComponent {
    palette: ColorPalette,
    latest: Option<MemSnapshot>,
    focused: bool,
}

impl MemComponent {
    pub fn new(palette: ColorPalette) -> Self {
        Self {
            palette,
            latest: None,
            focused: false,
        }
    }
}

impl Default for MemComponent {
    fn default() -> Self {
        Self::new(ColorPalette::dark())
    }
}

/// Format a byte count as a human-readable string (GiB, MiB, KiB).
fn fmt_bytes(bytes: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    const KIB: u64 = 1024;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{} B", bytes)
    }
}

impl Component for MemComponent {
    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn preferred_height(&self) -> Option<u16> {
        // 2 border rows + RAM row + SWAP row
        Some(4)
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        if let Action::MemUpdate(snap) = action {
            self.latest = Some(snap);
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let border_color = if self.focused {
            self.palette.accent
        } else {
            self.palette.border
        };
        let title_style = if self.focused {
            Style::new()
                .fg(self.palette.fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(self.palette.fg)
        };
        let block = Block::default()
            .title(Span::styled(" MEM ", title_style))
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border_color));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let Some(snap) = &self.latest else {
            return Ok(());
        };

        // Layout: RAM gauge, swap gauge, optional Linux swap activity line
        #[cfg(target_os = "linux")]
        let show_swap_activity = snap.swap_in_bytes > 0 || snap.swap_out_bytes > 0;
        #[cfg(not(target_os = "linux"))]
        let show_swap_activity = false;

        let row_count = if show_swap_activity { 3 } else { 2 };
        let constraints: Vec<Constraint> = (0..row_count).map(|_| Constraint::Length(1)).collect();
        let rows = Layout::vertical(constraints).split(inner);

        // RAM row: bar on the left, label on the right
        let ram_ratio = if snap.ram_total > 0 {
            (snap.ram_used as f64 / snap.ram_total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let ram_cols = Layout::horizontal([Constraint::Fill(1), Constraint::Length(LABEL_WIDTH)])
            .split(rows[0]);
        let ram_gauge = Gauge::default()
            .ratio(ram_ratio)
            .label("")
            .gauge_style(Style::new().fg(self.palette.accent));
        frame.render_widget(ram_gauge, ram_cols[0]);
        let ram_label = Span::styled(
            format!(
                "RAM  {}/{} {:>5.1}%",
                fmt_bytes(snap.ram_used),
                fmt_bytes(snap.ram_total),
                ram_ratio * 100.0,
            ),
            Style::new().fg(self.palette.fg),
        );
        frame.render_widget(ram_label, ram_cols[1]);

        // SWAP row: bar on the left, label on the right
        let swap_ratio = if snap.swap_total > 0 {
            (snap.swap_used as f64 / snap.swap_total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // Warn color when swap is being actively used
        let swap_color = if snap.swap_used > 0 {
            self.palette.warn
        } else {
            self.palette.dim
        };
        let swap_cols = Layout::horizontal([Constraint::Fill(1), Constraint::Length(LABEL_WIDTH)])
            .split(rows[1]);
        let swap_gauge = Gauge::default()
            .ratio(swap_ratio)
            .label("")
            .gauge_style(Style::new().fg(swap_color));
        frame.render_widget(swap_gauge, swap_cols[0]);
        let swap_label = Span::styled(
            format!(
                "SWAP {}/{} {:>5.1}%",
                fmt_bytes(snap.swap_used),
                fmt_bytes(snap.swap_total),
                swap_ratio * 100.0,
            ),
            Style::new().fg(swap_color),
        );
        frame.render_widget(swap_label, swap_cols[1]);

        // Linux-only: swap activity rate
        #[cfg(target_os = "linux")]
        if show_swap_activity {
            let line = Line::from(vec![
                Span::styled("swap in: ", Style::new().fg(self.palette.dim)),
                Span::styled(
                    fmt_bytes(snap.swap_in_bytes),
                    Style::new().fg(self.palette.warn),
                ),
                Span::styled("  out: ", Style::new().fg(self.palette.dim)),
                Span::styled(
                    fmt_bytes(snap.swap_out_bytes),
                    Style::new().fg(self.palette.warn),
                ),
            ]);
            frame.render_widget(Paragraph::new(line), rows[2]);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{action::Action, stats::snapshots::MemSnapshot};
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn renders_without_data() {
        let mut comp = MemComponent::default();
        let mut terminal = Terminal::new(TestBackend::new(60, 6)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("mem_no_data", terminal.backend());
    }

    #[test]
    fn renders_with_mem_data() {
        let mut comp = MemComponent::default();
        comp.update(Action::MemUpdate(MemSnapshot::stub())).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(60, 6)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("mem_with_data", terminal.backend());
    }

    #[test]
    fn fmt_bytes_gib() {
        assert!(fmt_bytes(2 * 1024 * 1024 * 1024).contains("GiB"));
    }

    #[test]
    fn fmt_bytes_mib() {
        assert!(fmt_bytes(5 * 1024 * 1024).contains("MiB"));
    }
}
