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
    components::{
        Component, FilterEvent, FilterInput, HISTORY_LEN, ListView, fmt_rate, fmt_rate_col,
        handle_detail_key, list_border_block, truncate,
    },
    stats::snapshots::NetSnapshot,
    theme::ColorPalette,
};

#[derive(Debug)]
pub struct NetComponent {
    palette: ColorPalette,
    focus_key: char,
    latest: Option<NetSnapshot>,
    list_state: ListState,
    /// Per-interface ring buffers: (tx_bytes_per_sec, rx_bytes_per_sec).
    history: HashMap<String, (VecDeque<u64>, VecDeque<u64>)>,
    /// Aggregate TX/RX history: sum of all interface rates per tick.
    agg_history: (VecDeque<u64>, VecDeque<u64>),
    view: ListView,
    /// Active name-substring filter (stored lowercase). Empty string = no filter.
    filter: String,
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
            agg_history: (VecDeque::new(), VecDeque::new()),
            view: ListView::List,
            filter: String::new(),
            focused: false,
            is_fullscreen: false,
        }
    }

    fn name_matches(&self, name: &str) -> bool {
        // self.filter is stored lowercase, so only the name needs lowercasing.
        self.filter.is_empty() || name.to_lowercase().contains(&self.filter)
    }

    fn clamp_selection(&mut self) {
        let filtered_len = self.latest.as_ref().map(|snap| {
            if self.filter.is_empty() {
                snap.interfaces.len()
            } else {
                snap.interfaces
                    .iter()
                    .filter(|i| i.name.to_lowercase().contains(&self.filter))
                    .count()
            }
        });
        match filtered_len {
            None | Some(0) => self.list_state.select(None),
            Some(n) => {
                let sel = self.list_state.selected().unwrap_or(0).min(n - 1);
                self.list_state.select(Some(sel));
            }
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

/// Width of the graph legend column inner area: "TX  1.2 MB/s" = 12 chars.
const LEGEND_INNER_W: u16 = 12;
/// Total legend column width including the Borders::LEFT separator.
const LEGEND_TOTAL_W: u16 = LEGEND_INNER_W + 1;

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
impl Component for NetComponent {
    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        if !focused {
            self.is_fullscreen = false;
        }
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match &self.view {
            ListView::Detail { .. } => {
                return Ok(Some(handle_detail_key(
                    key,
                    self.is_fullscreen,
                    &mut self.view,
                )));
            }
            ListView::Filter { .. } => {
                // Take ownership of input without cloning the whole enum.
                let input = match std::mem::replace(&mut self.view, ListView::List) {
                    ListView::Filter { input } => input,
                    _ => unreachable!("variant confirmed above"),
                };
                match FilterInput::handle_key(input, key) {
                    FilterEvent::Clear => {
                        self.filter = String::new();
                        self.clamp_selection();
                        // view is already ListView::List from replace above
                    }
                    FilterEvent::Commit => {
                        // filter stays as-is (already updated per keypress); view stays ListView::List
                    }
                    FilterEvent::Update(s) => {
                        self.filter = s.to_lowercase(); // keep stored filter lowercased
                        self.view = ListView::Filter { input: s };
                        self.clamp_selection();
                    }
                    FilterEvent::Ignored(input) => {
                        // key not consumed — restore view
                        self.view = ListView::Filter { input };
                    }
                }
                return Ok(Some(Action::Render));
            }
            ListView::List => {
                let filtered_names: Vec<String> = match &self.latest {
                    None => return Ok(None),
                    Some(snap) => snap
                        .interfaces
                        .iter()
                        .filter(|i| self.name_matches(&i.name))
                        .map(|i| i.name.clone())
                        .collect(),
                };
                let len = filtered_names.len();
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
                        if let Some(name) = filtered_names.get(idx) {
                            let name = name.clone();
                            self.view = ListView::Detail { name };
                            // Open the fullscreen modal unless already fullscreen.
                            let action = if !self.is_fullscreen {
                                Action::ToggleFullScreen
                            } else {
                                Action::Render
                            };
                            return Ok(Some(action));
                        }
                    }
                    KeyCode::Char('/') => {
                        self.view = ListView::Filter {
                            input: self.filter.clone(),
                        };
                        return Ok(Some(Action::Render));
                    }
                    _ => {}
                }
            }
        }
        Ok(None)
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        match action {
            Action::NetUpdate(snap) => {
                let mut snap = snap.clone();
                snap.interfaces
                    .sort_by(|left, right| left.name.cmp(&right.name));
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
                // Accumulate aggregate TX/RX across all interfaces
                let total_tx: u64 = snap.interfaces.iter().map(|i| i.tx_bytes).sum();
                let total_rx: u64 = snap.interfaces.iter().map(|i| i.rx_bytes).sum();
                if self.agg_history.0.len() >= HISTORY_LEN {
                    self.agg_history.0.pop_front();
                    self.agg_history.1.pop_front();
                }
                self.agg_history.0.push_back(total_tx);
                self.agg_history.1.push_back(total_rx);
                self.latest = Some(snap);
                // Clamp selection to the filtered list length.
                self.clamp_selection();
            }
            Action::ToggleFullScreen if self.focused => {
                self.is_fullscreen = !self.is_fullscreen;
            }
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        match &self.view {
            ListView::List | ListView::Filter { .. } => self.draw_list(frame, area),
            ListView::Detail { name } => {
                let name = name.clone();
                self.draw_detail(frame, area, &name)
            }
        }
    }
}

impl NetComponent {
    fn border_block(&self, rest: &str) -> Block<'static> {
        list_border_block(self.focus_key, rest, &self.palette, self.focused)
    }

    fn draw_list(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let title_rest = match &self.view {
            ListView::Filter { input } => format!("ET [/{}▌]", input),
            _ if !self.filter.is_empty() => format!("ET [/{}]", self.filter),
            _ => "ET".to_string(),
        };
        let block = self.border_block(&title_rest);
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

        // Chart (when tall enough) + separator + header row + list
        let chart_h: u16 = if inner.height >= 9 { 4 } else { 0 };
        let sep_h: u16 = if chart_h > 0 { 1 } else { 0 };
        let layout = Layout::vertical([
            Constraint::Length(chart_h),
            Constraint::Length(sep_h),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .split(inner);

        if chart_h > 0 {
            self.draw_compact_chart(frame, layout[0]);
            frame.render_widget(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::new().fg(self.palette.border)),
                layout[1],
            );
        }

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
        frame.render_widget(Line::from(header_spans), layout[2]);

        let palette = &self.palette;
        let filter = self.filter.to_lowercase();
        let items: Vec<ListItem> = snap
            .interfaces
            .iter()
            .filter(|i| filter.is_empty() || i.name.to_lowercase().contains(&filter))
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

        // The app renders the normal (compact) layout first, then the fullscreen
        // overlay, both within the same terminal.draw closure.  The compact render
        // sets list_state.offset based on its small visible area; ratatui will not
        // reduce that offset even when the fullscreen area is large enough to show
        // all items from row 0.  Reset before the fullscreen render so ratatui
        // computes a fresh offset appropriate for the larger area.
        if self.is_fullscreen {
            let sel = self.list_state.selected();
            self.list_state = ListState::default();
            self.list_state.select(sel);
        }

        frame.render_stateful_widget(list, layout[3], &mut self.list_state);
        Ok(())
    }

    /// Draws the compact aggregate TX/RX chart used in the list view header.
    fn draw_compact_chart(&self, frame: &mut Frame, area: Rect) {
        let (tx_hist, rx_hist) = &self.agg_history;
        if tx_hist.is_empty() || area.width <= LEGEND_TOTAL_W + 4 {
            return;
        }

        let [graph_area, legend_area] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(LEGEND_TOTAL_W)])
                .areas(area);

        let hist_len = HISTORY_LEN as f64;
        let n = tx_hist.len();
        let tx_data: Vec<(f64, f64)> = tx_hist
            .iter()
            .enumerate()
            .map(|(j, &v)| (hist_len - n as f64 + j as f64, v as f64))
            .collect();
        let rx_data: Vec<(f64, f64)> = rx_hist
            .iter()
            .enumerate()
            .map(|(j, &v)| (hist_len - n as f64 + j as f64, v as f64))
            .collect();

        let y_max = tx_hist
            .iter()
            .chain(rx_hist.iter())
            .copied()
            .max()
            .unwrap_or(0)
            .max(1024) as f64;

        let datasets = vec![
            Dataset::default()
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::new().fg(self.palette.accent))
                .data(&tx_data),
            Dataset::default()
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::new().fg(self.palette.highlight))
                .data(&rx_data),
        ];

        let chart = Chart::new(datasets)
            .x_axis(
                Axis::default()
                    .bounds([0.0, hist_len])
                    .style(Style::new().fg(self.palette.dim)),
            )
            .y_axis(
                Axis::default()
                    .bounds([0.0, y_max])
                    .style(Style::new().fg(self.palette.dim)),
            );
        frame.render_widget(chart, graph_area);

        // Left border of the legend block acts as the y-axis separator.
        let legend_block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::new().fg(self.palette.border));
        let legend_inner = legend_block.inner(legend_area);
        frame.render_widget(legend_block, legend_area);

        let tx_cur = tx_hist.back().copied().unwrap_or(0);
        let rx_cur = rx_hist.back().copied().unwrap_or(0);
        let rows = (legend_inner.height as usize).min(2);
        if rows == 0 {
            return;
        }

        let label_rows =
            Layout::vertical((0..rows).map(|_| Constraint::Length(1)).collect::<Vec<_>>())
                .split(legend_inner);

        let rate_w = LEGEND_INNER_W as usize - 3; // "TX " prefix is 3 chars
        frame.render_widget(
            Span::styled(
                format!("TX {:>rate_w$}", fmt_rate(tx_cur)),
                Style::new().fg(self.palette.accent),
            ),
            label_rows[0],
        );
        if rows >= 2 {
            frame.render_widget(
                Span::styled(
                    format!("RX {:>rate_w$}", fmt_rate(rx_cur)),
                    Style::new().fg(self.palette.highlight),
                ),
                label_rows[1],
            );
        }
    }

    fn draw_detail(&mut self, frame: &mut Frame, area: Rect, name: &str) -> Result<()> {
        let rest = format!("ET: {name}");
        let block = self.border_block(&rest);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // How many stats header rows to show (need at least 6 rows for a useful graph).
        let stats_rows: u16 = if inner.height >= 10 { 2 } else { 0 };
        let sep_h: u16 = if stats_rows > 0 { 1 } else { 0 };

        let sections = Layout::vertical([
            Constraint::Length(stats_rows),
            Constraint::Length(sep_h),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .split(inner);

        // --- Stats header ---
        if stats_rows > 0
            && let Some(snap) = self.latest.as_ref()
            && let Some(iface) = snap.interfaces.iter().find(|i| i.name == name)
        {
            let dim = Style::new().fg(self.palette.dim);
            let val = Style::new().fg(self.palette.fg);
            let hi = Style::new().fg(self.palette.highlight);
            let ac = Style::new().fg(self.palette.accent);

            // Fixed column widths keep values aligned regardless of label length.
            const LW: usize = 10; // label column: "Total TX:" is longest at 9
            const VW: usize = 14; // value column: byte counts fit comfortably in 14

            // IP column is truncated to leave room for MAC and MTU on the same line.
            const IP_VW: usize = 30;
            const ERR_VW: usize = 8; // error/drop counts are small numbers

            let ips = if iface.ip_addresses.is_empty() {
                "-".to_string()
            } else {
                iface.ip_addresses.join("  ")
            };

            let stat_lines =
                Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(sections[0]);

            // Row 0: IP · MAC · MTU
            frame.render_widget(
                Line::from(vec![
                    Span::styled(format!("{:<LW$}", "IP:"), dim),
                    Span::styled(format!("{:<IP_VW$}", truncate(&ips, IP_VW)), val),
                    Span::styled(format!("{:<LW$}", "MAC:"), dim),
                    Span::styled(format!("{:<20}", iface.mac_address.clone()), val),
                    Span::styled(format!("{:<LW$}", "MTU:"), dim),
                    Span::styled(iface.mtu.to_string(), val),
                ]),
                stat_lines[0],
            );

            // Row 1: Total TX · Total RX · Err TX · Err RX (+ Linux: Drop TX · Drop RX)
            #[cfg(not(target_os = "linux"))]
            let traffic_line = Line::from(vec![
                Span::styled(format!("{:<LW$}", "Total TX:"), dim),
                Span::styled(format!("{:<VW$}", fmt_rate_col(iface.total_tx_bytes)), ac),
                Span::styled(format!("{:<LW$}", "Total RX:"), dim),
                Span::styled(format!("{:<VW$}", fmt_rate_col(iface.total_rx_bytes)), hi),
                Span::styled(format!("{:<LW$}", "Err TX:"), dim),
                Span::styled(format!("{:<ERR_VW$}", iface.tx_errors.to_string()), val),
                Span::styled(format!("{:<LW$}", "Err RX:"), dim),
                Span::styled(iface.rx_errors.to_string(), val),
            ]);
            #[cfg(target_os = "linux")]
            let traffic_line = Line::from(vec![
                Span::styled(format!("{:<LW$}", "Total TX:"), dim),
                Span::styled(format!("{:<VW$}", fmt_rate_col(iface.total_tx_bytes)), ac),
                Span::styled(format!("{:<LW$}", "Total RX:"), dim),
                Span::styled(format!("{:<VW$}", fmt_rate_col(iface.total_rx_bytes)), hi),
                Span::styled(format!("{:<LW$}", "Err TX:"), dim),
                Span::styled(format!("{:<ERR_VW$}", iface.tx_errors.to_string()), val),
                Span::styled(format!("{:<LW$}", "Err RX:"), dim),
                Span::styled(format!("{:<ERR_VW$}", iface.rx_errors.to_string()), val),
                Span::styled(format!("{:<LW$}", "Drop TX:"), dim),
                Span::styled(format!("{:<ERR_VW$}", iface.tx_dropped.to_string()), val),
                Span::styled(format!("{:<LW$}", "Drop RX:"), dim),
                Span::styled(iface.rx_dropped.to_string(), val),
            ]);
            frame.render_widget(traffic_line, stat_lines[1]);
        }

        // --- Separator ---
        if sep_h > 0 {
            frame.render_widget(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::new().fg(self.palette.border)),
                sections[1],
            );
        }

        // --- Graph ---
        let (tx_hist, rx_hist) = match self.history.get(name) {
            Some(h) => h,
            None => return Ok(()),
        };

        let hist_len = HISTORY_LEN as f64;
        let n = tx_hist.len();
        let tx_data: Vec<(f64, f64)> = tx_hist
            .iter()
            .enumerate()
            .map(|(j, &v)| (hist_len - n as f64 + j as f64, v as f64))
            .collect();
        let rx_data: Vec<(f64, f64)> = rx_hist
            .iter()
            .enumerate()
            .map(|(j, &v)| (hist_len - n as f64 + j as f64, v as f64))
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
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::new().fg(self.palette.accent))
                .data(&tx_data),
            Dataset::default()
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::new().fg(self.palette.highlight))
                .data(&rx_data),
        ];

        let chart = Chart::new(datasets)
            .x_axis(
                Axis::default()
                    .bounds([0.0, hist_len])
                    .style(Style::new().fg(self.palette.dim)),
            )
            .y_axis(
                Axis::default()
                    .bounds([0.0, y_max])
                    .style(Style::new().fg(self.palette.dim)),
            );

        let [graph_area, legend_area] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(LEGEND_TOTAL_W)])
                .areas(sections[2]);

        frame.render_widget(chart, graph_area);

        // Left border of the legend block acts as the y-axis separator.
        let legend_block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::new().fg(self.palette.border));
        let legend_inner = legend_block.inner(legend_area);
        frame.render_widget(legend_block, legend_area);

        let tx_cur = tx_hist.back().copied().unwrap_or(0);
        let rx_cur = rx_hist.back().copied().unwrap_or(0);
        let rows = (legend_inner.height as usize).min(2);
        if rows > 0 {
            let label_rows =
                Layout::vertical((0..rows).map(|_| Constraint::Length(1)).collect::<Vec<_>>())
                    .split(legend_inner);

            let rate_w = LEGEND_INNER_W as usize - 3; // "TX " prefix is 3 chars
            frame.render_widget(
                Span::styled(
                    format!("TX {:>rate_w$}", fmt_rate(tx_cur)),
                    Style::new().fg(self.palette.accent),
                ),
                label_rows[0],
            );
            if rows >= 2 {
                frame.render_widget(
                    Span::styled(
                        format!("RX {:>rate_w$}", fmt_rate(rx_cur)),
                        Style::new().fg(self.palette.highlight),
                    ),
                    label_rows[1],
                );
            }
        }

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
                Span::styled("   Esc/q: back", Style::new().fg(self.palette.dim)),
            ]);
            frame.render_widget(summary, sections[3]);
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
        comp.update(&Action::NetUpdate(NetSnapshot::stub()))
            .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("net_with_data", terminal.backend());
    }

    #[test]
    fn enter_switches_to_graph_esc_returns_to_list() {
        let mut comp = NetComponent::default();
        comp.update(&Action::NetUpdate(NetSnapshot::stub()))
            .unwrap();
        // Select first interface and press Enter
        comp.list_state.select(Some(0));
        comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        assert!(matches!(comp.view, ListView::Detail { .. }));
        // Esc returns to list
        comp.handle_key_event(key(KeyCode::Esc)).unwrap();
        assert!(matches!(comp.view, ListView::List));
    }

    #[test]
    fn q_closes_detail_view() {
        let mut comp = NetComponent::default();
        comp.update(&Action::NetUpdate(NetSnapshot::stub()))
            .unwrap();
        comp.list_state.select(Some(0));
        comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        assert!(matches!(comp.view, ListView::Detail { .. }));
        // q also returns to list
        comp.handle_key_event(key(KeyCode::Char('q'))).unwrap();
        assert!(matches!(comp.view, ListView::List));
    }

    #[test]
    fn enter_emits_toggle_fullscreen_when_not_fullscreen() {
        let mut comp = NetComponent::default();
        comp.set_focused(true);
        comp.update(&Action::NetUpdate(NetSnapshot::stub()))
            .unwrap();
        comp.list_state.select(Some(0));
        let action = comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        assert!(
            matches!(action, Some(Action::ToggleFullScreen)),
            "Enter must request fullscreen when not already fullscreen"
        );
    }

    #[test]
    fn esc_in_detail_emits_toggle_fullscreen_when_fullscreen() {
        let mut comp = NetComponent::default();
        comp.set_focused(true);
        comp.update(&Action::NetUpdate(NetSnapshot::stub()))
            .unwrap();
        // Simulate fullscreen being active (as the app would set via ToggleFullScreen).
        comp.is_fullscreen = true;
        comp.view = ListView::Detail {
            name: "lo".to_string(),
        };
        let action = comp.handle_key_event(key(KeyCode::Esc)).unwrap();
        assert!(
            matches!(action, Some(Action::ToggleFullScreen)),
            "Esc from detail must close fullscreen"
        );
        assert!(matches!(comp.view, ListView::List));
    }

    #[test]
    fn history_accumulates_per_interface() {
        let mut comp = NetComponent::default();
        for _ in 0..50 {
            comp.update(&Action::NetUpdate(NetSnapshot::stub()))
                .unwrap();
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
            comp.update(&Action::NetUpdate(NetSnapshot::stub()))
                .unwrap();
        }
        for (tx, rx) in comp.history.values() {
            assert!(tx.len() <= HISTORY_LEN);
            assert!(rx.len() <= HISTORY_LEN);
        }
    }

    #[test]
    fn agg_history_ring_buffer_bounded() {
        let mut comp = NetComponent::default();
        for _ in 0..200 {
            comp.update(&Action::NetUpdate(NetSnapshot::stub()))
                .unwrap();
        }
        assert!(comp.agg_history.0.len() <= HISTORY_LEN);
        assert!(comp.agg_history.1.len() <= HISTORY_LEN);
    }

    #[test]
    fn renders_list_with_chart() {
        let mut comp = NetComponent::default();
        for _ in 0..50 {
            comp.update(&Action::NetUpdate(NetSnapshot::stub()))
                .unwrap();
        }
        // 14 rows gives inner height 12, which triggers the compact chart (threshold 9).
        let mut terminal = Terminal::new(TestBackend::new(60, 14)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("net_list_with_chart", terminal.backend());
    }

    #[test]
    fn renders_graph_view() {
        let mut comp = NetComponent::default();
        for _ in 0..50 {
            comp.update(&Action::NetUpdate(NetSnapshot::stub()))
                .unwrap();
        }
        comp.list_state.select(Some(0));
        comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(130, 20)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("net_graph_view", terminal.backend());
    }

    #[test]
    fn detail_view_consumes_unhandled_keys() {
        // Keys not explicitly handled in detail mode must return Some so the
        // global app handler never sees them and cannot shift focus or close
        // the modal.
        let mut comp = NetComponent::default();
        comp.update(&Action::NetUpdate(NetSnapshot::stub()))
            .unwrap();
        comp.list_state.select(Some(0));
        comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        assert!(matches!(comp.view, ListView::Detail { .. }));

        for code in [
            KeyCode::Tab,
            KeyCode::BackTab,
            KeyCode::Char('p'),
            KeyCode::Char('i'),
            KeyCode::Char('f'),
            KeyCode::Char('d'),
        ] {
            let action = comp.handle_key_event(key(code)).unwrap();
            assert!(
                action.is_some(),
                "{code:?} must be consumed in detail view, got None"
            );
            assert!(
                matches!(comp.view, ListView::Detail { .. }),
                "{code:?} must not exit detail view"
            );
        }
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
        comp.update(&Action::NetUpdate(snap)).unwrap();
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
        comp.update(&Action::NetUpdate(NetSnapshot {
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
        comp.update(&Action::NetUpdate(NetSnapshot::stub()))
            .unwrap();
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
        comp.update(&Action::NetUpdate(NetSnapshot::stub()))
            .unwrap();
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
        comp.update(&Action::NetUpdate(NetSnapshot::stub()))
            .unwrap();
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
        comp.update(&Action::ToggleFullScreen).unwrap();
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
        comp.update(&Action::ToggleFullScreen).unwrap();
        assert!(!comp.is_fullscreen);
    }

    #[test]
    fn first_update_auto_selects_row_zero() {
        let mut comp = NetComponent::default();
        assert_eq!(
            comp.list_state.selected(),
            None,
            "no selection before first update"
        );
        comp.update(&Action::NetUpdate(NetSnapshot::stub()))
            .unwrap();
        assert_eq!(
            comp.list_state.selected(),
            Some(0),
            "first update must select row 0"
        );
    }

    #[test]
    fn selection_preserved_across_updates() {
        let snap = NetSnapshot {
            interfaces: vec![interface("eth0"), interface("eth1"), interface("eth2")],
        };
        let mut comp = NetComponent::default();
        comp.update(&Action::NetUpdate(snap.clone())).unwrap();
        comp.list_state.select(Some(2));
        comp.update(&Action::NetUpdate(snap)).unwrap();
        assert_eq!(
            comp.list_state.selected(),
            Some(2),
            "selection must survive re-update"
        );
    }

    #[test]
    fn selection_clamped_when_list_shrinks() {
        let three = NetSnapshot {
            interfaces: vec![interface("eth0"), interface("eth1"), interface("eth2")],
        };
        let one = NetSnapshot {
            interfaces: vec![interface("eth0")],
        };
        let mut comp = NetComponent::default();
        comp.update(&Action::NetUpdate(three)).unwrap();
        comp.list_state.select(Some(2));
        comp.update(&Action::NetUpdate(one)).unwrap();
        assert_eq!(
            comp.list_state.selected(),
            Some(0),
            "selection must clamp to last row"
        );
    }

    #[test]
    fn selection_cleared_when_list_becomes_empty() {
        let mut comp = NetComponent::default();
        comp.update(&Action::NetUpdate(NetSnapshot::stub()))
            .unwrap();
        assert_eq!(comp.list_state.selected(), Some(0));
        comp.update(&Action::NetUpdate(NetSnapshot { interfaces: vec![] }))
            .unwrap();
        assert_eq!(
            comp.list_state.selected(),
            None,
            "empty list must clear selection"
        );
    }

    /// Detail view shows MAC, IP, and error/drop stats.
    #[test]
    fn detail_view_shows_interface_stats() {
        let mut comp = NetComponent::default();
        for _ in 0..10 {
            comp.update(&Action::NetUpdate(NetSnapshot::stub()))
                .unwrap();
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

    #[test]
    fn slash_enters_filter_mode() {
        let mut comp = NetComponent::default();
        comp.update(&Action::NetUpdate(NetSnapshot::stub()))
            .unwrap();
        comp.handle_key_event(key(KeyCode::Char('/'))).unwrap();
        assert!(
            matches!(comp.view, ListView::Filter { .. }),
            "/ must enter filter mode"
        );
    }

    #[test]
    fn filter_mode_char_updates_filter_and_view() {
        let mut comp = NetComponent::default();
        comp.update(&Action::NetUpdate(NetSnapshot::stub()))
            .unwrap();
        comp.handle_key_event(key(KeyCode::Char('/'))).unwrap();
        comp.handle_key_event(key(KeyCode::Char('e'))).unwrap();
        comp.handle_key_event(key(KeyCode::Char('t'))).unwrap();
        assert_eq!(comp.filter, "et");
        assert!(matches!(comp.view, ListView::Filter { ref input } if input == "et"));
    }

    #[test]
    fn filter_mode_backspace_removes_char() {
        let mut comp = NetComponent::default();
        comp.update(&Action::NetUpdate(NetSnapshot::stub()))
            .unwrap();
        comp.handle_key_event(key(KeyCode::Char('/'))).unwrap();
        comp.handle_key_event(key(KeyCode::Char('e'))).unwrap();
        comp.handle_key_event(key(KeyCode::Char('t'))).unwrap();
        comp.handle_key_event(key(KeyCode::Backspace)).unwrap();
        assert_eq!(comp.filter, "e");
    }

    #[test]
    fn filter_mode_esc_clears_filter_and_returns_to_list() {
        let mut comp = NetComponent::default();
        comp.update(&Action::NetUpdate(NetSnapshot::stub()))
            .unwrap();
        comp.handle_key_event(key(KeyCode::Char('/'))).unwrap();
        comp.handle_key_event(key(KeyCode::Char('l'))).unwrap();
        comp.handle_key_event(key(KeyCode::Esc)).unwrap();
        assert_eq!(comp.filter, "", "Esc must clear filter");
        assert!(
            matches!(comp.view, ListView::List),
            "Esc must return to list"
        );
    }

    #[test]
    fn filter_mode_enter_keeps_filter_and_returns_to_list() {
        let mut comp = NetComponent::default();
        comp.update(&Action::NetUpdate(NetSnapshot::stub()))
            .unwrap();
        comp.handle_key_event(key(KeyCode::Char('/'))).unwrap();
        comp.handle_key_event(key(KeyCode::Char('l'))).unwrap();
        comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        assert_eq!(comp.filter, "l", "Enter must keep filter");
        assert!(
            matches!(comp.view, ListView::List),
            "Enter must return to list"
        );
    }

    #[test]
    fn filter_narrows_list_for_navigation() {
        // Three interfaces; filter to only "lo" (one match).
        let snap = NetSnapshot {
            interfaces: vec![interface("eth0"), interface("lo"), interface("wlan0")],
        };
        let mut comp = NetComponent::default();
        comp.update(&Action::NetUpdate(snap)).unwrap();
        comp.handle_key_event(key(KeyCode::Char('/'))).unwrap();
        comp.handle_key_event(key(KeyCode::Char('l'))).unwrap();
        comp.handle_key_event(key(KeyCode::Char('o'))).unwrap();
        comp.handle_key_event(key(KeyCode::Enter)).unwrap(); // exit filter mode
        assert_eq!(comp.filter, "lo");
        // Down must be a no-op since filtered list has only 1 item.
        let sel_before = comp.list_state.selected();
        comp.handle_key_event(key(KeyCode::Down)).unwrap();
        assert_eq!(
            comp.list_state.selected(),
            sel_before,
            "Down must not advance past the last filtered item"
        );
    }

    #[test]
    fn filter_enter_opens_filtered_interface() {
        let snap = NetSnapshot {
            interfaces: vec![interface("eth0"), interface("lo"), interface("wlan0")],
        };
        let mut comp = NetComponent::default();
        comp.update(&Action::NetUpdate(snap)).unwrap();
        // Filter to "wlan" then Enter to keep filter and navigate.
        comp.handle_key_event(key(KeyCode::Char('/'))).unwrap();
        for c in "wlan".chars() {
            comp.handle_key_event(key(KeyCode::Char(c))).unwrap();
        }
        comp.handle_key_event(key(KeyCode::Enter)).unwrap(); // exit filter mode
        // Row 0 of the filtered list is "wlan0". Enter must open it.
        comp.list_state.select(Some(0));
        comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        assert!(
            matches!(&comp.view, ListView::Detail { name } if name == "wlan0"),
            "Enter must open the filtered interface, got: {:?}",
            comp.view
        );
    }

    #[test]
    fn filter_mode_swallows_keys() {
        let mut comp = NetComponent::default();
        comp.update(&Action::NetUpdate(NetSnapshot::stub()))
            .unwrap();
        comp.handle_key_event(key(KeyCode::Char('/'))).unwrap();
        for code in [KeyCode::Tab, KeyCode::BackTab, KeyCode::F(1)] {
            let action = comp.handle_key_event(key(code)).unwrap();
            assert!(action.is_some(), "{code:?} must be consumed in filter mode");
            assert!(
                matches!(comp.view, ListView::Filter { .. }),
                "{code:?} must not exit filter mode"
            );
        }
    }
}
