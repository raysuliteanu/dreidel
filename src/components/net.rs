use std::collections::{HashMap, VecDeque};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, List, ListItem, ListState},
};

use crate::{
    action::Action,
    components::{Component, fmt_rate_col, keyed_title, truncate},
    stats::snapshots::NetSnapshot,
    theme::ColorPalette,
};

pub const HISTORY_LEN: usize = 100;

/// Which view the net panel is currently showing.
#[derive(Debug, Clone)]
enum NetView {
    /// Text list of all interfaces with live TX/RX rates.
    List,
    /// Graph view for a specific interface (Enter to enter, Esc to leave).
    Graph { name: String },
}

#[derive(Debug)]
pub struct NetComponent {
    palette: ColorPalette,
    focus_key: char,
    latest: Option<NetSnapshot>,
    list_state: ListState,
    /// Per-interface ring buffers: (tx_bytes_per_sec, rx_bytes_per_sec).
    history: HashMap<String, (VecDeque<u64>, VecDeque<u64>)>,
    view: NetView,
    focused: bool,
}

impl NetComponent {
    pub fn new(palette: ColorPalette, focus_key: char) -> Self {
        Self {
            palette,
            focus_key,
            latest: None,
            list_state: ListState::default(),
            history: HashMap::new(),
            view: NetView::List,
            focused: false,
        }
    }
}

impl Default for NetComponent {
    fn default() -> Self {
        Self::new(ColorPalette::dark(), 'n')
    }
}

/// Width of the TX and RX metric columns (right-aligned).
const COL_W: u16 = 12;

/// Format a byte rate with "/s" suffix — used for graph axis labels.
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
    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match &self.view.clone() {
            NetView::Graph { .. } => {
                if key.code == KeyCode::Esc {
                    self.view = NetView::List;
                    return Ok(Some(Action::Render));
                }
            }
            NetView::List => {
                let Some(snap) = &self.latest else {
                    return Ok(None);
                };
                let len = snap.interfaces.len();
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
                        }
                        return Ok(Some(Action::Render));
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
                    KeyCode::Enter => {
                        let idx = self.list_state.selected().unwrap_or(0);
                        if let Some(iface) = snap.interfaces.get(idx) {
                            self.view = NetView::Graph {
                                name: iface.name.clone(),
                            };
                            return Ok(Some(Action::Render));
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(None)
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        if let Action::NetUpdate(snap) = action {
            // Keep list selection in bounds after refresh
            if let Some(sel) = self.list_state.selected()
                && sel >= snap.interfaces.len()
            {
                self.list_state.select(snap.interfaces.len().checked_sub(1));
            }
            // Accumulate per-interface rate history
            for iface in &snap.interfaces {
                let entry = self.history.entry(iface.name.clone()).or_default();
                if entry.0.len() >= HISTORY_LEN {
                    entry.0.pop_front();
                    entry.1.pop_front();
                }
                entry.0.push_back(iface.tx_bytes);
                entry.1.push_back(iface.rx_bytes);
            }
            self.latest = Some(snap);
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        match self.view.clone() {
            NetView::List => self.draw_list(frame, area),
            NetView::Graph { name } => self.draw_graph(frame, area, &name),
        }
    }
}

impl NetComponent {
    fn border_block(&self, rest: &str) -> Block<'static> {
        let border_color = if self.focused {
            self.palette.accent
        } else {
            self.palette.border
        };
        Block::default()
            .title(keyed_title(self.focus_key, rest, &self.palette))
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border_color))
    }

    fn draw_list(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let block = self.border_block("ET");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let Some(snap) = &self.latest else {
            return Ok(());
        };

        // Derive name column width from available space.
        let fixed = (COL_W * 2) as usize;
        let name_w = (inner.width as usize).saturating_sub(fixed);

        // Header row + list area
        let chunks = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).split(inner);

        let accent_bold = Style::new()
            .fg(self.palette.accent)
            .add_modifier(Modifier::BOLD);
        let header = Line::from(vec![
            Span::styled(format!("{:<width$}", "Iface", width = name_w), accent_bold),
            Span::styled(
                format!("{:>width$}", "TX (B/s)", width = COL_W as usize),
                accent_bold,
            ),
            Span::styled(
                format!("{:>width$}", "RX (B/s)", width = COL_W as usize),
                accent_bold,
            ),
        ]);
        frame.render_widget(header, chunks[0]);

        let items: Vec<ListItem> = snap
            .interfaces
            .iter()
            .map(|iface| {
                let line = Line::from(vec![
                    Span::styled(
                        format!("{:<width$}", truncate(&iface.name, name_w), width = name_w),
                        Style::new().fg(self.palette.fg),
                    ),
                    Span::styled(
                        format!(
                            "{:>width$}",
                            fmt_rate_col(iface.tx_bytes),
                            width = COL_W as usize
                        ),
                        Style::new().fg(self.palette.accent),
                    ),
                    Span::styled(
                        format!(
                            "{:>width$}",
                            fmt_rate_col(iface.rx_bytes),
                            width = COL_W as usize
                        ),
                        Style::new().fg(self.palette.highlight),
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

    fn draw_graph(&mut self, frame: &mut Frame, area: Rect, name: &str) -> Result<()> {
        let rest = format!("ET: {name}");
        let block = self.border_block(&rest);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let (tx_hist, rx_hist) = match self.history.get(name) {
            Some(h) => h,
            None => return Ok(()),
        };

        // Convert to Chart data points: x = sample index, y = bytes/s
        let tx_data: Vec<(f64, f64)> = tx_hist
            .iter()
            .enumerate()
            .map(|(i, &v)| (i as f64, v as f64))
            .collect();
        let rx_data: Vec<(f64, f64)> = rx_hist
            .iter()
            .enumerate()
            .map(|(i, &v)| (i as f64, v as f64))
            .collect();

        let y_max = tx_hist
            .iter()
            .chain(rx_hist.iter())
            .copied()
            .max()
            .unwrap_or(0)
            .max(1024) as f64; // floor at 1 KB/s so the axis is never zero-height

        let datasets = vec![
            Dataset::default()
                .name("TX")
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::new().fg(self.palette.accent))
                .data(&tx_data),
            Dataset::default()
                .name("RX")
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::new().fg(self.palette.highlight))
                .data(&rx_data),
        ];

        let x_max = HISTORY_LEN as f64;
        let chart = Chart::new(datasets)
            .x_axis(
                Axis::default()
                    .bounds([0.0, x_max])
                    .style(Style::new().fg(self.palette.dim)),
            )
            .y_axis(
                Axis::default()
                    .bounds([0.0, y_max])
                    .labels([
                        Span::styled("0", Style::new().fg(self.palette.dim)),
                        Span::styled(
                            fmt_rate(y_max as u64 / 2),
                            Style::new().fg(self.palette.dim),
                        ),
                        Span::styled(fmt_rate(y_max as u64), Style::new().fg(self.palette.dim)),
                    ])
                    .style(Style::new().fg(self.palette.dim)),
            );

        // Reserve one row at the bottom for the live TX/RX values
        let rows = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).split(inner);
        frame.render_widget(chart, rows[0]);

        // Live rate summary line
        if let Some(snap) = &self.latest
            && let Some(iface) = snap.interfaces.iter().find(|i| i.name == name)
        {
            let summary = Line::from(vec![
                Span::styled("TX: ", Style::new().fg(self.palette.dim)),
                Span::styled(
                    fmt_rate(iface.tx_bytes),
                    Style::new().fg(self.palette.accent),
                ),
                Span::styled("   RX: ", Style::new().fg(self.palette.dim)),
                Span::styled(
                    fmt_rate(iface.rx_bytes),
                    Style::new().fg(self.palette.highlight),
                ),
                Span::styled("   Esc: back", Style::new().fg(self.palette.dim)),
            ]);
            frame.render_widget(summary, rows[1]);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        action::Action,
        components::{fmt_rate_col, truncate},
        stats::snapshots::NetSnapshot,
    };
    use crossterm::event::KeyModifiers;
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

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
    fn enter_switches_to_graph_esc_returns_to_list() {
        let mut comp = NetComponent::default();
        comp.update(Action::NetUpdate(NetSnapshot::stub())).unwrap();
        // Select first interface and press Enter
        comp.list_state.select(Some(0));
        comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        assert!(matches!(comp.view, NetView::Graph { .. }));
        // Esc returns to list
        comp.handle_key_event(key(KeyCode::Esc)).unwrap();
        assert!(matches!(comp.view, NetView::List));
    }

    #[test]
    fn history_accumulates_per_interface() {
        let mut comp = NetComponent::default();
        for _ in 0..50 {
            comp.update(Action::NetUpdate(NetSnapshot::stub())).unwrap();
        }
        // All interfaces in the stub should have history
        for (tx, rx) in comp.history.values() {
            assert_eq!(tx.len(), 50);
            assert_eq!(rx.len(), 50);
        }
    }

    #[test]
    fn history_ring_buffer_bounded() {
        let mut comp = NetComponent::default();
        for _ in 0..200 {
            comp.update(Action::NetUpdate(NetSnapshot::stub())).unwrap();
        }
        for (tx, rx) in comp.history.values() {
            assert!(tx.len() <= HISTORY_LEN);
            assert!(rx.len() <= HISTORY_LEN);
        }
    }

    #[test]
    fn renders_graph_view() {
        let mut comp = NetComponent::default();
        for _ in 0..50 {
            comp.update(Action::NetUpdate(NetSnapshot::stub())).unwrap();
        }
        comp.list_state.select(Some(0));
        comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("net_graph_view", terminal.backend());
    }

    #[test]
    fn fmt_rate_mb_threshold() {
        // Graph labels keep "/s" suffix.
        assert!(fmt_rate(5_000_000).contains("MB/s"));
    }

    #[test]
    fn fmt_rate_kb_threshold() {
        assert!(fmt_rate(50_000).contains("KB/s"));
    }

    #[test]
    fn fmt_rate_col_no_suffix() {
        // Column cells drop "/s" — the header provides the unit context.
        let s = fmt_rate_col(5_000_000);
        assert!(s.contains("MB") && !s.contains("/s"), "got: {s}");
        let s = fmt_rate_col(50_000);
        assert!(s.contains("KB") && !s.contains("/s"), "got: {s}");
    }

    #[test]
    fn truncate_long_iface() {
        assert_eq!(truncate("wlp0s20f3u1u2", 10), "wlp0s20...");
    }

    #[test]
    fn truncate_short_iface() {
        assert_eq!(truncate("lo", 10), "lo");
    }

    #[test]
    fn page_up_down_clamp_to_list_bounds() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        // Build a snapshot with 5 interfaces — fewer than PAGE (10).
        let mut snap = NetSnapshot::stub();
        snap.interfaces = (0..5)
            .map(|i| crate::stats::snapshots::InterfaceSnapshot {
                name: format!("eth{i}"),
                rx_bytes: 0,
                tx_bytes: 0,
            })
            .collect();

        let mut comp = NetComponent::default();
        comp.update(Action::NetUpdate(snap)).unwrap();
        comp.list_state.select(Some(2));

        // PageDown from middle must clamp to last (index 4, not 12).
        let action = comp
            .handle_key_event(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE))
            .unwrap();
        assert!(matches!(action, Some(Action::Render)));
        assert_eq!(
            comp.list_state.selected(),
            Some(4),
            "PageDown must clamp at last item"
        );

        // PageUp from last must clamp to first (index 0).
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
