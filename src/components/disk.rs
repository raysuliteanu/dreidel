use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::{
    action::Action,
    components::{Component, keyed_title},
    stats::snapshots::DiskSnapshot,
    theme::ColorPalette,
};

#[derive(Debug)]
pub struct DiskComponent {
    palette: ColorPalette,
    focus_key: char,
    latest: Option<DiskSnapshot>,
    list_state: ListState,
    focused: bool,
}

impl DiskComponent {
    pub fn new(palette: ColorPalette, focus_key: char) -> Self {
        Self {
            palette,
            focus_key,
            latest: None,
            list_state: ListState::default(),
            focused: false,
        }
    }
}

impl Default for DiskComponent {
    fn default() -> Self {
        Self::new(ColorPalette::dark(), 'i')
    }
}

/// Width of each metric column (Read, Write) — right-aligned.
const COL_W: u16 = 12;
/// Width of the usage percentage column.
const USAGE_W: u16 = 7;

/// Format a byte rate without the "/s" suffix — the column header carries
/// the "(B/s)" unit context.
fn fmt_rate_col(bytes_per_sec: u64) -> String {
    const MB: u64 = 1_000_000;
    const KB: u64 = 1_000;
    if bytes_per_sec >= MB {
        format!("{:.1} MB", bytes_per_sec as f64 / MB as f64)
    } else if bytes_per_sec >= KB {
        format!("{:.1} KB", bytes_per_sec as f64 / KB as f64)
    } else {
        format!("{} B", bytes_per_sec)
    }
}

/// Truncate `s` to `max` chars, replacing the last 3 with `...` if truncated.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 3 {
        s[..max].to_string()
    } else {
        format!("{}...", &s[..max - 3])
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
        let block = Block::default()
            .title(keyed_title(self.focus_key, "DISK", &self.palette))
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border_color));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let Some(snap) = &self.latest else {
            return Ok(());
        };

        // Derive name column width from available space.
        let fixed = (COL_W * 2 + USAGE_W) as usize;
        let name_w = (inner.width as usize).saturating_sub(fixed);

        // Header row + list area
        let chunks = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).split(inner);

        let accent_bold = Style::new()
            .fg(self.palette.accent)
            .add_modifier(Modifier::BOLD);
        let header = Line::from(vec![
            Span::styled(format!("{:<width$}", "Device", width = name_w), accent_bold),
            Span::styled(
                format!("{:>width$}", "Read (B/s)", width = COL_W as usize),
                accent_bold,
            ),
            Span::styled(
                format!("{:>width$}", "Write (B/s)", width = COL_W as usize),
                accent_bold,
            ),
            Span::styled(
                format!("{:>width$}", "Use%", width = USAGE_W as usize),
                accent_bold,
            ),
        ]);
        frame.render_widget(header, chunks[0]);

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
                        format!("{:<width$}", truncate(&dev.name, name_w), width = name_w),
                        Style::new().fg(self.palette.fg),
                    ),
                    Span::styled(
                        format!(
                            "{:>width$}",
                            fmt_rate_col(dev.read_bytes),
                            width = COL_W as usize
                        ),
                        Style::new().fg(self.palette.accent),
                    ),
                    Span::styled(
                        format!(
                            "{:>width$}",
                            fmt_rate_col(dev.write_bytes),
                            width = COL_W as usize
                        ),
                        Style::new().fg(self.palette.highlight),
                    ),
                    Span::styled(
                        format!("{:>width$.1}%", dev.usage_pct, width = USAGE_W as usize - 1),
                        Style::new().fg(usage_color),
                    ),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .highlight_style(Style::new().bg(self.palette.border).fg(self.palette.fg));

        frame.render_stateful_widget(list, chunks[1], &mut self.list_state);
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
    fn fmt_rate_col_bytes() {
        // No "/s" suffix — unit context comes from the column header.
        let s = fmt_rate_col(500);
        assert!(s.contains('B') && !s.contains("/s"), "got: {s}");
    }

    #[test]
    fn fmt_rate_col_kb() {
        let s = fmt_rate_col(500_000);
        assert!(s.contains("KB") && !s.contains("/s"), "got: {s}");
    }

    #[test]
    fn truncate_long_name() {
        assert_eq!(truncate("nvme0n1p3_extra", 10), "nvme0n1...");
    }

    #[test]
    fn truncate_short_name() {
        assert_eq!(truncate("sda", 10), "sda");
    }
}
