use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::{
    action::Action, components::Component, stats::snapshots::DiskSnapshot, theme::ColorPalette,
};

#[derive(Debug)]
pub struct DiskComponent {
    palette: ColorPalette,
    latest: Option<DiskSnapshot>,
    list_state: ListState,
    focused: bool,
}

impl DiskComponent {
    pub fn new(palette: ColorPalette) -> Self {
        Self {
            palette,
            latest: None,
            list_state: ListState::default(),
            focused: false,
        }
    }
}

impl Default for DiskComponent {
    fn default() -> Self {
        Self::new(ColorPalette::dark())
    }
}

/// Format bytes/s as KB/s or MB/s for I/O rate display.
fn fmt_rate(bytes_per_sec: u64) -> String {
    const MB: u64 = 1_000_000;
    const KB: u64 = 1_000;
    if bytes_per_sec >= MB {
        format!("{:.1} MB/s", bytes_per_sec as f64 / MB as f64)
    } else if bytes_per_sec >= KB {
        format!("{:.1} KB/s", bytes_per_sec as f64 / KB as f64)
    } else {
        format!("{} B/s", bytes_per_sec)
    }
}

impl Component for DiskComponent {
    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        let Some(snap) = &self.latest else {
            return Ok(None);
        };
        let len = snap.devices.len();
        if len == 0 {
            return Ok(None);
        }
        match key.code {
            KeyCode::Up => {
                let i = self.list_state.selected().unwrap_or(0);
                self.list_state.select(Some(i.saturating_sub(1)));
            }
            KeyCode::Down => {
                let i = self.list_state.selected().unwrap_or(0);
                if i + 1 < len {
                    self.list_state.select(Some(i + 1));
                }
            }
            _ => {}
        }
        Ok(None)
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        if let Action::DiskUpdate(snap) = action {
            // Keep selection in bounds after refresh
            if let Some(sel) = self.list_state.selected()
                && sel >= snap.devices.len()
            {
                self.list_state.select(snap.devices.len().checked_sub(1));
            }
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
            .title(Span::styled(" DISK ", title_style))
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border_color));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let Some(snap) = &self.latest else {
            return Ok(());
        };

        let items: Vec<ListItem> = snap
            .devices
            .iter()
            .map(|dev| {
                // Color usage% by severity: normal → accent, high → warn, critical → critical
                let usage_color = if dev.usage_pct >= 90.0 {
                    self.palette.critical
                } else if dev.usage_pct >= 70.0 {
                    self.palette.warn
                } else {
                    self.palette.accent
                };
                let line = Line::from(vec![
                    Span::styled(
                        format!("{:<12}", dev.name),
                        Style::new().fg(self.palette.fg),
                    ),
                    Span::styled(" r:", Style::new().fg(self.palette.dim)),
                    Span::styled(
                        format!("{:>12}", fmt_rate(dev.read_bytes)),
                        Style::new().fg(self.palette.accent),
                    ),
                    Span::styled("  w:", Style::new().fg(self.palette.dim)),
                    Span::styled(
                        format!("{:>12}", fmt_rate(dev.write_bytes)),
                        Style::new().fg(self.palette.highlight),
                    ),
                    Span::styled(
                        format!("  {:>5.1}%", dev.usage_pct),
                        Style::new().fg(usage_color),
                    ),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .highlight_style(Style::new().bg(self.palette.border).fg(self.palette.fg));

        frame.render_stateful_widget(list, inner, &mut self.list_state);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{action::Action, stats::snapshots::DiskSnapshot};
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn renders_without_data() {
        let mut comp = DiskComponent::default();
        let mut terminal = Terminal::new(TestBackend::new(70, 8)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("disk_no_data", terminal.backend());
    }

    #[test]
    fn renders_with_disk_data() {
        let mut comp = DiskComponent::default();
        comp.update(Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(70, 8)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("disk_with_data", terminal.backend());
    }

    #[test]
    fn fmt_rate_bytes() {
        assert!(fmt_rate(500).contains("B/s"));
    }

    #[test]
    fn fmt_rate_kb() {
        assert!(fmt_rate(500_000).contains("KB/s"));
    }
}
