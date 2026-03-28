// SPDX-License-Identifier: GPL-3.0-only

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
    /// Detail view for a specific interface: stats header + TX/RX graph.
    Detail { name: String },
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
    is_fullscreen: bool,
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
            is_fullscreen: false,
        }
    }
}

impl Default for NetComponent {
    fn default() -> Self {
        Self::new(ColorPalette::dark(), 'n')
    }
}

/// Width of per-tick packet-rate columns shown in full-screen list mode.
const PKT_W: u16 = 10;

/// Width of the TX and RX metric columns (right-aligned).
const COL_W: u16 = 12;

/// Format a packet count for the packet-rate column (no "/s" — header provides context).
fn fmt_packets(pkts: u64) -> String {
    const K: u64 = 1_000;
    if pkts >= K {
        format!("{:.1}K", pkts as f64 / K as f64)
    } else {
        format!("{pkts}")
    }
}

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
        if !focused {
            self.is_fullscreen = false;
        }
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match &self.view.clone() {
            NetView::Detail { .. } => {
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
                            self.view = NetView::Detail {
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
        match action {
            Action::NetUpdate(snap) => {
                let mut snap = snap;
                snap.interfaces
                    .sort_by(|left, right| left.name.cmp(&right.name));
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
            Action::ToggleFullScreen if self.focused => {
                self.is_fullscreen = !self.is_fullscreen;
            }
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        match self.view.clone() {
            NetView::List => self.draw_list(frame, area),
            NetView::Detail { name } => self.draw_detail(frame, area, &name),
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

        // Show packet-rate and IP columns when fullscreen or when the panel is wide enough.
        // Name column is capped so it doesn't consume all the space; any remainder goes to IP.
        const MIN_WIDE_AREA: u16 = 100; // area.width threshold (borders included)
        const MAX_NAME_W: usize = 20; // cap so wide terminals don't bury the extra columns

        let wide = self.is_fullscreen || area.width >= MIN_WIDE_AREA;

        // Fixed columns (excluding name and IP): TX bytes + RX bytes + TX pkt + RX pkt
        let pkt_fixed = (PKT_W * 2) as usize;
        let byte_fixed = (COL_W * 2) as usize;
        let available = inner.width as usize;

        // ip_w is the TOTAL IP column width including the 2-char leading gap.
        // A full IPv6 address with /128 suffix is 43 chars; 46 = 2 gap + 44 content gives headroom.
        const MAX_IP_W: usize = 46;

        let (name_w, ip_w, extra_cols) = if wide && available > byte_fixed + pkt_fixed + 14 {
            // 14 = minimum useful display: 4 name + 10 ip (2 gap + 8 content)
            let for_name_ip = available.saturating_sub(byte_fixed + pkt_fixed);
            let ip_w = for_name_ip.saturating_sub(4).clamp(10, MAX_IP_W);
            let name_w = for_name_ip.saturating_sub(ip_w).clamp(4, MAX_NAME_W);
            (name_w, ip_w, true)
        } else {
            let name_w = available.saturating_sub(byte_fixed).max(4);
            (name_w, 0, false)
        };

        // Header row + list area
        let chunks = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).split(inner);

        let accent_bold = Style::new()
            .fg(self.palette.accent)
            .add_modifier(Modifier::BOLD);
        let mut header_spans = vec![
            Span::styled(format!("{:<width$}", "Iface", width = name_w), accent_bold),
            Span::styled(
                format!("{:>width$}", "TX (B/s)", width = COL_W as usize),
                accent_bold,
            ),
            Span::styled(
                format!("{:>width$}", "RX (B/s)", width = COL_W as usize),
                accent_bold,
            ),
        ];
        if extra_cols {
            header_spans.push(Span::styled(
                format!("{:>width$}", "TX Pkt/s", width = PKT_W as usize),
                accent_bold,
            ));
            header_spans.push(Span::styled(
                format!("{:>width$}", "RX Pkt/s", width = PKT_W as usize),
                accent_bold,
            ));
            // The 2-char gap is baked into ip_w; embed it in the span so it is inseparable.
            let ip_content_w = ip_w.saturating_sub(2);
            header_spans.push(Span::styled(
                format!("  {:<width$}", "IP", width = ip_content_w),
                accent_bold,
            ));
        }
        frame.render_widget(Line::from(header_spans), chunks[0]);

        let palette = &self.palette;
        let items: Vec<ListItem> = snap
            .interfaces
            .iter()
            .map(|iface| {
                let mut spans = vec![
                    Span::styled(
                        format!("{:<width$}", truncate(&iface.name, name_w), width = name_w),
                        Style::new().fg(palette.fg),
                    ),
                    Span::styled(
                        format!(
                            "{:>width$}",
                            fmt_rate_col(iface.tx_bytes),
                            width = COL_W as usize
                        ),
                        Style::new().fg(palette.accent),
                    ),
                    Span::styled(
                        format!(
                            "{:>width$}",
                            fmt_rate_col(iface.rx_bytes),
                            width = COL_W as usize
                        ),
                        Style::new().fg(palette.highlight),
                    ),
                ];
                if extra_cols {
                    spans.push(Span::styled(
                        format!(
                            "{:>width$}",
                            fmt_packets(iface.tx_packets),
                            width = PKT_W as usize
                        ),
                        Style::new().fg(palette.accent),
                    ));
                    spans.push(Span::styled(
                        format!(
                            "{:>width$}",
                            fmt_packets(iface.rx_packets),
                            width = PKT_W as usize
                        ),
                        Style::new().fg(palette.highlight),
                    ));
                    let ips = if iface.ip_addresses.is_empty() {
                        "-".to_string()
                    } else {
                        iface.ip_addresses.join("  ")
                    };
                    let ip_content_w = ip_w.saturating_sub(2);
                    spans.push(Span::styled(
                        format!(
                            "  {:<width$}",
                            truncate(&ips, ip_content_w),
                            width = ip_content_w
                        ),
                        Style::new().fg(palette.dim),
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items)
            .highlight_style(Style::new().bg(self.palette.border).fg(self.palette.fg));

        frame.render_stateful_widget(list, chunks[1], &mut self.list_state);
        Ok(())
    }

    fn draw_detail(&mut self, frame: &mut Frame, area: Rect, name: &str) -> Result<()> {
        let rest = format!("ET: {name}");
        let block = self.border_block(&rest);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // How many stats header rows to show (need at least 6 rows for a useful graph).
        let stats_rows: u16 = if inner.height >= 10 { 3 } else { 0 };

        let sections = Layout::vertical([
            Constraint::Length(stats_rows),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .split(inner);

        // --- Stats header ---
        if stats_rows > 0
            && let Some(snap) = &self.latest.clone()
            && let Some(iface) = snap.interfaces.iter().find(|i| i.name == name)
        {
            let dim = Style::new().fg(self.palette.dim);
            let val = Style::new().fg(self.palette.fg);
            let hi = Style::new().fg(self.palette.highlight);
            let ac = Style::new().fg(self.palette.accent);

            let ips = if iface.ip_addresses.is_empty() {
                "-".to_string()
            } else {
                iface.ip_addresses.join("  ")
            };

            let stat_lines = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(sections[0]);

            // Row 0: IPs
            frame.render_widget(
                Line::from(vec![Span::styled("IP: ", dim), Span::styled(ips, val)]),
                stat_lines[0],
            );

            // Row 1: MAC · MTU
            frame.render_widget(
                Line::from(vec![
                    Span::styled("MAC: ", dim),
                    Span::styled(iface.mac_address.clone(), val),
                    Span::styled("   MTU: ", dim),
                    Span::styled(iface.mtu.to_string(), val),
                    Span::styled("   Total TX: ", dim),
                    Span::styled(fmt_rate_col(iface.total_tx_bytes), ac),
                    Span::styled("  RX: ", dim),
                    Span::styled(fmt_rate_col(iface.total_rx_bytes), hi),
                ]),
                stat_lines[1],
            );

            // Row 2: errors (+ Linux drops)
            #[cfg(not(target_os = "linux"))]
            let err_line = Line::from(vec![
                Span::styled("Err TX: ", dim),
                Span::styled(iface.tx_errors.to_string(), val),
                Span::styled("  RX: ", dim),
                Span::styled(iface.rx_errors.to_string(), val),
            ]);
            #[cfg(target_os = "linux")]
            let err_line = Line::from(vec![
                Span::styled("Err TX: ", dim),
                Span::styled(iface.tx_errors.to_string(), val),
                Span::styled("  RX: ", dim),
                Span::styled(iface.rx_errors.to_string(), val),
                Span::styled("   Drop TX: ", dim),
                Span::styled(iface.tx_dropped.to_string(), val),
                Span::styled("  RX: ", dim),
                Span::styled(iface.rx_dropped.to_string(), val),
            ]);
            frame.render_widget(err_line, stat_lines[2]);
        }

        // --- Graph ---
        let (tx_hist, rx_hist) = match self.history.get(name) {
            Some(h) => h,
            None => return Ok(()),
        };

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
        frame.render_widget(chart, sections[1]);

        // --- Bottom summary line ---
        if let Some(snap) = &self.latest
            && let Some(iface) = snap.interfaces.iter().find(|i| i.name == name)
        {
            let summary = Line::from(vec![
                Span::styled("TX: ", Style::new().fg(self.palette.dim)),
                Span::styled(
                    fmt_rate(iface.tx_bytes),
                    Style::new().fg(self.palette.accent),
                ),
                Span::styled("  RX: ", Style::new().fg(self.palette.dim)),
                Span::styled(
                    fmt_rate(iface.rx_bytes),
                    Style::new().fg(self.palette.highlight),
                ),
                Span::styled("  TX Pkt/s: ", Style::new().fg(self.palette.dim)),
                Span::styled(
                    fmt_packets(iface.tx_packets),
                    Style::new().fg(self.palette.accent),
                ),
                Span::styled("  RX: ", Style::new().fg(self.palette.dim)),
                Span::styled(
                    fmt_packets(iface.rx_packets),
                    Style::new().fg(self.palette.highlight),
                ),
                Span::styled("   Esc: back", Style::new().fg(self.palette.dim)),
            ]);
            frame.render_widget(summary, sections[2]);
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
        stats::snapshots::{InterfaceSnapshot, NetSnapshot},
    };
    use crossterm::event::KeyModifiers;
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn interface(name: &str) -> InterfaceSnapshot {
        InterfaceSnapshot {
            name: name.into(),
            rx_bytes: 0,
            tx_bytes: 0,
            rx_packets: 0,
            tx_packets: 0,
            rx_errors: 0,
            tx_errors: 0,
            total_rx_bytes: 0,
            total_tx_bytes: 0,
            mac_address: String::new(),
            ip_addresses: vec![],
            mtu: 1500,
            #[cfg(target_os = "linux")]
            rx_dropped: 0,
            #[cfg(target_os = "linux")]
            tx_dropped: 0,
        }
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
        assert!(matches!(comp.view, NetView::Detail { .. }));
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
        snap.interfaces = (0..5).map(|i| interface(&format!("eth{i}"))).collect();

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

    #[test]
    fn sorts_interfaces_by_name_before_rendering() {
        let mut comp = NetComponent::default();
        comp.update(Action::NetUpdate(NetSnapshot {
            interfaces: vec![interface("wlan0"), interface("eth0"), interface("lo")],
        }))
        .unwrap();

        let names: Vec<&str> = comp
            .latest
            .as_ref()
            .expect("net snapshot should be stored")
            .interfaces
            .iter()
            .map(|iface| iface.name.as_str())
            .collect();

        assert_eq!(names, vec!["eth0", "lo", "wlan0"]);
    }

    /// Wide area (>= MIN_WIDE_AREA) triggers extra columns regardless of is_fullscreen.
    #[test]
    fn wide_area_shows_extra_columns() {
        let mut comp = NetComponent::default();
        comp.update(Action::NetUpdate(NetSnapshot::stub())).unwrap();
        // 120-col terminal: well above MIN_WIDE_AREA (100), so extra columns must appear.
        let mut terminal = Terminal::new(TestBackend::new(120, 10)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        let rendered = terminal.backend().to_string();
        assert!(
            rendered.contains("TX Pkt/s"),
            "wide area must show TX Pkt/s header; got:\n{rendered}"
        );
        assert!(
            rendered.contains("RX Pkt/s"),
            "wide area must show RX Pkt/s header; got:\n{rendered}"
        );
        assert!(
            rendered.contains("IP"),
            "wide area must show IP header; got:\n{rendered}"
        );
    }

    /// Narrow area (< MIN_WIDE_AREA) should NOT show extra columns.
    #[test]
    fn narrow_area_hides_extra_columns() {
        let mut comp = NetComponent::default();
        comp.update(Action::NetUpdate(NetSnapshot::stub())).unwrap();
        // 60-col terminal: below MIN_WIDE_AREA — only TX/RX byte columns shown.
        let mut terminal = Terminal::new(TestBackend::new(60, 10)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        let rendered = terminal.backend().to_string();
        assert!(
            !rendered.contains("TX Pkt/s"),
            "narrow area must not show TX Pkt/s; got:\n{rendered}"
        );
    }

    /// Wide list: name column must be capped so extra columns stay on screen.
    #[test]
    fn wide_list_name_column_is_capped() {
        let mut comp = NetComponent::default();
        comp.update(Action::NetUpdate(NetSnapshot::stub())).unwrap();
        let width = 160_u16;
        let mut terminal = Terminal::new(TestBackend::new(width, 10)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        let rendered = terminal.backend().to_string();
        // The stub IP "192.168.1.100/24" must appear somewhere — if name consumed all space it wouldn't.
        assert!(
            rendered.contains("192.168"),
            "IP address must be visible in wide list; got:\n{rendered}"
        );
    }

    /// is_fullscreen resets when focus is removed.
    #[test]
    fn set_focused_false_clears_fullscreen() {
        let mut comp = NetComponent::default();
        comp.set_focused(true);
        comp.update(Action::ToggleFullScreen).unwrap();
        assert!(comp.is_fullscreen);
        comp.set_focused(false);
        assert!(
            !comp.is_fullscreen,
            "fullscreen must clear when focus is lost"
        );
    }

    /// ToggleFullScreen ignored when component is not focused.
    #[test]
    fn toggle_fullscreen_ignored_when_not_focused() {
        let mut comp = NetComponent::default();
        comp.set_focused(false);
        comp.update(Action::ToggleFullScreen).unwrap();
        assert!(!comp.is_fullscreen);
    }

    /// Detail view shows MAC, IP, and error/drop stats.
    #[test]
    fn detail_view_shows_interface_stats() {
        let mut comp = NetComponent::default();
        for _ in 0..10 {
            comp.update(Action::NetUpdate(NetSnapshot::stub())).unwrap();
        }
        comp.list_state.select(Some(0));
        comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        let rendered = terminal.backend().to_string();
        assert!(rendered.contains("MAC:"), "detail must show MAC label");
        assert!(
            rendered.contains("aa:bb:cc:dd:ee:ff"),
            "detail must show MAC value"
        );
        assert!(rendered.contains("IP:"), "detail must show IP label");
        assert!(rendered.contains("192.168"), "detail must show IP address");
        assert!(rendered.contains("MTU:"), "detail must show MTU label");
        assert!(
            rendered.contains("TX Pkt/s:"),
            "bottom bar must show packet rates"
        );
    }
}
