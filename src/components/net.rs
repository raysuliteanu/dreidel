use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::{
    action::Action, components::Component, stats::snapshots::NetSnapshot, theme::ColorPalette,
};

#[derive(Debug)]
pub struct NetComponent {
    palette: ColorPalette,
    latest: Option<NetSnapshot>,
    list_state: ListState,
}

impl NetComponent {
    pub fn new(palette: ColorPalette) -> Self {
        Self {
            palette,
            latest: None,
            list_state: ListState::default(),
        }
    }
}

impl Default for NetComponent {
    fn default() -> Self {
        Self::new(ColorPalette::dark())
    }
}

/// Format bytes/s as KB/s or MB/s for display in the interface list.
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

impl Component for NetComponent {
    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        let Some(snap) = &self.latest else {
            return Ok(None);
        };
        let len = snap.interfaces.len();
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
        if let Action::NetUpdate(snap) = action {
            // Keep selection in bounds after refresh
            if let Some(sel) = self.list_state.selected()
                && sel >= snap.interfaces.len()
            {
                self.list_state.select(snap.interfaces.len().checked_sub(1));
            }
            self.latest = Some(snap);
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let block = Block::default()
            .title(" NET ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(self.palette.border));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let Some(snap) = &self.latest else {
            return Ok(());
        };

        let items: Vec<ListItem> = snap
            .interfaces
            .iter()
            .map(|iface| {
                let line = Line::from(vec![
                    Span::styled(
                        format!("{:<12}", iface.name),
                        Style::new().fg(self.palette.fg),
                    ),
                    Span::styled(" ▲ ", Style::new().fg(self.palette.dim)),
                    Span::styled(
                        format!("{:>12}", fmt_rate(iface.tx_bytes)),
                        Style::new().fg(self.palette.accent),
                    ),
                    Span::styled("  ▼ ", Style::new().fg(self.palette.dim)),
                    Span::styled(
                        format!("{:>12}", fmt_rate(iface.rx_bytes)),
                        Style::new().fg(self.palette.highlight),
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
    use crate::{action::Action, stats::snapshots::NetSnapshot};
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn renders_without_data() {
        let mut comp = NetComponent::default();
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("net_no_data", terminal.backend());
    }

    #[test]
    fn renders_with_net_data() {
        let mut comp = NetComponent::default();
        comp.update(Action::NetUpdate(NetSnapshot::stub())).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("net_with_data", terminal.backend());
    }

    #[test]
    fn fmt_rate_mb_threshold() {
        assert!(fmt_rate(5_000_000).contains("MB/s"));
    }

    #[test]
    fn fmt_rate_kb_threshold() {
        assert!(fmt_rate(50_000).contains("KB/s"));
    }
}
