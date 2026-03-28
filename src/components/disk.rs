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
    components::{Component, fmt_rate_col, keyed_title, truncate},
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
        const PAGE: usize = 10;
        match key.code {
            KeyCode::Up => {
                let i = self.list_state.selected().unwrap_or(0);
                self.list_state.select(Some(i.saturating_sub(1)));
                return Ok(Some(Action::Render));
            }
            KeyCode::Down => {
                let i = self.list_state.selected().unwrap_or(0);
                if i + 1 < len {
                    self.list_state.select(Some(i + 1));
                    return Ok(Some(Action::Render));
                }
            }
            KeyCode::PageUp => {
                let i = self.list_state.selected().unwrap_or(0);
                self.list_state.select(Some(i.saturating_sub(PAGE)));
                return Ok(Some(Action::Render));
            }
            KeyCode::PageDown => {
                let i = self.list_state.selected().unwrap_or(0);
                self.list_state
                    .select(Some((i + PAGE).min(len.saturating_sub(1))));
                return Ok(Some(Action::Render));
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
    use crate::{
        action::Action,
        components::{fmt_rate_col, truncate},
        stats::snapshots::DiskSnapshot,
    };
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

    #[test]
    fn up_down_return_render() {
        use crate::stats::snapshots::DiskDeviceSnapshot;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut comp = DiskComponent::default();
        // Two devices so Down can actually advance from row 0 to row 1.
        let two_devices = DiskSnapshot {
            devices: vec![
                DiskDeviceSnapshot {
                    name: "sda".into(),
                    read_bytes: 0,
                    write_bytes: 0,
                    usage_pct: 10.0,
                },
                DiskDeviceSnapshot {
                    name: "sdb".into(),
                    read_bytes: 0,
                    write_bytes: 0,
                    usage_pct: 20.0,
                },
            ],
        };
        comp.update(Action::DiskUpdate(two_devices)).unwrap();
        comp.list_state.select(Some(0));
        let down = comp
            .handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();
        assert!(
            matches!(down, Some(Action::Render)),
            "Down should trigger Render"
        );
        let up = comp
            .handle_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
            .unwrap();
        assert!(
            matches!(up, Some(Action::Render)),
            "Up should trigger Render"
        );
    }

    #[test]
    fn page_up_down_clamp_to_list_bounds() {
        use crate::stats::snapshots::DiskDeviceSnapshot;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let make_device = |name: &str| DiskDeviceSnapshot {
            name: name.into(),
            read_bytes: 0,
            write_bytes: 0,
            usage_pct: 0.0,
        };
        // 5 devices — fewer than PAGE (10) so clamping is exercised.
        let snap = DiskSnapshot {
            devices: (0..5).map(|i| make_device(&format!("sd{i}"))).collect(),
        };
        let mut comp = DiskComponent::default();
        comp.update(Action::DiskUpdate(snap)).unwrap();
        comp.list_state.select(Some(2));

        // PageDown from middle must jump to last (index 4, not 12).
        let action = comp
            .handle_key_event(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE))
            .unwrap();
        assert!(matches!(action, Some(Action::Render)));
        assert_eq!(
            comp.list_state.selected(),
            Some(4),
            "PageDown must clamp at last item"
        );

        // PageUp from last must jump to first (index 0, not wrap negative).
        let action = comp
            .handle_key_event(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE))
            .unwrap();
        assert!(matches!(action, Some(Action::Render)));
        assert_eq!(
            comp.list_state.selected(),
            Some(0),
            "PageUp must clamp at first item"
        );
    }
}
