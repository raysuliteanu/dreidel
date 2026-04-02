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
    stats::snapshots::DiskSnapshot,
    theme::ColorPalette,
};

#[derive(Debug)]
struct DiskCompactSnapshot {
    selected: Option<usize>,
    filter: String,
}

#[derive(Debug)]
pub struct DiskComponent {
    palette: ColorPalette,
    focus_key: char,
    latest: Option<DiskSnapshot>,
    list_state: ListState,
    /// Per-device ring buffers: (read_bytes_per_sec, write_bytes_per_sec).
    history: HashMap<String, (VecDeque<u64>, VecDeque<u64>)>,
    view: ListView,
    /// Active name-substring filter (stored lowercase). Empty string = no filter.
    filter: String,
    focused: bool,
    is_fullscreen: bool,
    compact_snapshot: Option<DiskCompactSnapshot>,
    /// One-shot flag set by `begin_overlay_render()`.  Consumed at the start of
    /// `draw()` to distinguish the compact background pass from the overlay pass.
    rendering_as_overlay: bool,
}

impl DiskComponent {
    pub fn new(palette: ColorPalette, focus_key: char) -> Self {
        Self {
            palette,
            focus_key,
            latest: None,
            list_state: ListState::default(),
            history: HashMap::new(),
            view: ListView::List,
            filter: String::new(),
            focused: false,
            is_fullscreen: false,
            compact_snapshot: None,
            rendering_as_overlay: false,
        }
    }

    fn name_matches(&self, name: &str) -> bool {
        // self.filter is stored lowercase, so only the name needs lowercasing.
        self.filter.is_empty() || name.to_lowercase().contains(&self.filter)
    }

    fn clamp_selection(&mut self) {
        let filtered_len = self.latest.as_ref().map(|snap| {
            if self.filter.is_empty() {
                snap.devices.len()
            } else {
                snap.devices
                    .iter()
                    .filter(|d| d.name.to_lowercase().contains(&self.filter))
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

impl Default for DiskComponent {
    fn default() -> Self {
        Self::new(ColorPalette::dark(), 'i')
    }
}

/// Width of each metric column (Read, Write) — right-aligned.
const COL_W: u16 = 12;
/// Width of the usage percentage column.
const USAGE_W: u16 = 7;

/// Format an absolute byte count with SI suffixes (no "/s").
fn fmt_bytes(bytes: u64) -> String {
    const TB: u64 = 1_000_000_000_000;
    const GB: u64 = 1_000_000_000;
    const MB: u64 = 1_000_000;
    if bytes >= TB {
        format!("{:.1} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        fmt_rate_col(bytes)
    }
}

/// Format a byte rate with "/s" suffix — used for graph axis labels.
impl Component for DiskComponent {
    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        if !focused && self.is_fullscreen {
            self.restore_compact_snapshot();
        }
    }

    fn begin_overlay_render(&mut self) {
        self.rendering_as_overlay = true;
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
                        .devices
                        .iter()
                        .filter(|d| self.name_matches(&d.name))
                        .map(|d| d.name.clone())
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
            Action::DiskUpdate(snap) => {
                let mut snap = snap.clone();
                snap.devices
                    .sort_by(|left, right| left.name.cmp(&right.name));
                // sysinfo can report the same device multiple times (one per mount
                // point). Sort puts duplicates adjacent; dedup removes them.
                snap.devices.dedup_by(|a, b| a.name == b.name);
                // Accumulate per-device rate history
                for dev in &snap.devices {
                    let entry = self.history.entry(dev.name.clone()).or_default();
                    if entry.0.len() >= HISTORY_LEN {
                        entry.0.pop_front();
                        entry.1.pop_front();
                    }
                    entry.0.push_back(dev.read_bytes);
                    entry.1.push_back(dev.write_bytes);
                }
                self.latest = Some(snap);
                // Clamp selection to the filtered list length.
                self.clamp_selection();
            }
            Action::ToggleFullScreen if self.focused => {
                if !self.is_fullscreen {
                    self.compact_snapshot = Some(DiskCompactSnapshot {
                        selected: self.list_state.selected(),
                        filter: self.filter.clone(),
                    });
                    self.is_fullscreen = true;
                } else {
                    self.restore_compact_snapshot();
                }
            }
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        // One-shot overlay flag: consumed here so the compact background pass
        // and the overlay pass can be distinguished.
        let is_overlay = std::mem::replace(&mut self.rendering_as_overlay, false);

        // Compact background pass: render from frozen snapshot state.
        if self.is_fullscreen && !is_overlay {
            return self.draw_compact_background(frame, area);
        }

        match &self.view {
            ListView::List | ListView::Filter { .. } => self.draw_list(frame, area),
            ListView::Detail { name } => {
                let name = name.clone();
                self.draw_detail(frame, area, &name)
            }
        }
    }
}

impl DiskComponent {
    fn restore_compact_snapshot(&mut self) {
        if let Some(snap) = self.compact_snapshot.take() {
            self.filter = snap.filter;
            self.view = ListView::List;
            let mut ls = ListState::default();
            ls.select(snap.selected);
            self.list_state = ls;
        }
        self.is_fullscreen = false;
    }

    /// Render the compact sidebar appearance using the frozen snapshot state.
    ///
    /// Temporarily swaps live fields with snapshot values, calls `draw()` with
    /// `is_fullscreen = false`, then restores live state.  The recursive `draw()`
    /// call does not re-enter this method because `is_fullscreen` is false.
    fn draw_compact_background(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let Some(snap) = self.compact_snapshot.take() else {
            return Ok(()); // no snapshot yet — render nothing
        };

        let live_filter = std::mem::replace(&mut self.filter, snap.filter.clone());
        let live_view = std::mem::replace(&mut self.view, ListView::List);
        let live_fs = std::mem::replace(&mut self.is_fullscreen, false);
        let mut tmp_state = ListState::default();
        tmp_state.select(snap.selected);
        let live_list = std::mem::replace(&mut self.list_state, tmp_state);
        // rendering_as_overlay is already false (consumed at top of draw()).

        let result = self.draw(frame, area);

        self.filter = live_filter;
        self.view = live_view;
        self.is_fullscreen = live_fs;
        self.list_state = live_list;
        self.compact_snapshot = Some(snap);

        result
    }

    fn border_block(&self, rest: &str) -> Block<'static> {
        list_border_block(self.focus_key, rest, &self.palette, self.focused)
    }

    fn draw_list(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let title_rest = match &self.view {
            ListView::Filter { input } => format!("DISK [/{}▌]", input),
            _ if !self.filter.is_empty() => format!("DISK [/{}]", self.filter),
            _ => "DISK".to_string(),
        };
        let block = self.border_block(&title_rest);
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

        let palette = &self.palette;
        let filter = &self.filter; // already lowercase
        let items: Vec<ListItem> = snap
            .devices
            .iter()
            .filter(|d| filter.is_empty() || d.name.to_lowercase().contains(filter.as_str()))
            .map(|dev| {
                // Color usage% by severity: normal → accent, high → warn, critical → critical
                let usage_color = if dev.usage_pct >= 90.0 {
                    palette.critical
                } else if dev.usage_pct >= 70.0 {
                    palette.warn
                } else {
                    palette.accent
                };
                let line = Line::from(vec![
                    Span::styled(
                        format!("{:<width$}", truncate(&dev.name, name_w), width = name_w),
                        Style::new().fg(palette.fg),
                    ),
                    Span::styled(
                        format!(
                            "{:>width$}",
                            fmt_rate_col(dev.read_bytes),
                            width = COL_W as usize
                        ),
                        Style::new().fg(palette.accent),
                    ),
                    Span::styled(
                        format!(
                            "{:>width$}",
                            fmt_rate_col(dev.write_bytes),
                            width = COL_W as usize
                        ),
                        Style::new().fg(palette.highlight),
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

    fn draw_detail(&mut self, frame: &mut Frame, area: Rect, name: &str) -> Result<()> {
        let rest = format!("DISK: {name}");
        let block = self.border_block(&rest);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Show stats header rows only when there is enough vertical room for a useful graph.
        let stats_rows: u16 = if inner.height >= 12 { 4 } else { 0 };
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
            && let Some(dev) = snap.devices.iter().find(|d| d.name == name)
        {
            let dim = Style::new().fg(self.palette.dim);
            let val = Style::new().fg(self.palette.fg);

            // Fixed column widths keep values aligned regardless of label length.
            // LW=13 ensures at least 1 space after the longest label "Write total:" (12 chars).
            const LW: usize = 13;
            const VW: usize = 14; // value column: "12884.9 MB" fits in 14

            let stat_lines = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(sections[0]);

            // Row 0: mount point and disk kind
            frame.render_widget(
                Line::from(vec![
                    Span::styled(format!("{:<LW$}", "Mount:"), dim),
                    Span::styled(format!("{:<VW$}", truncate(&dev.mount_point, VW)), val),
                    Span::styled(format!("{:<LW$}", "Type:"), dim),
                    Span::styled(dev.kind.clone(), val),
                ]),
                stat_lines[0],
            );

            // Row 1: filesystem, read-only, removable
            frame.render_widget(
                Line::from(vec![
                    Span::styled(format!("{:<LW$}", "FS:"), dim),
                    Span::styled(format!("{:<VW$}", dev.file_system.clone()), val),
                    Span::styled(format!("{:<LW$}", "RO:"), dim),
                    Span::styled(
                        format!("{:<VW$}", if dev.is_read_only { "yes" } else { "no" }),
                        val,
                    ),
                    Span::styled(format!("{:<LW$}", "Removable:"), dim),
                    Span::styled(if dev.is_removable { "yes" } else { "no" }, val),
                ]),
                stat_lines[1],
            );

            // Row 2: total, free, and used
            frame.render_widget(
                Line::from(vec![
                    Span::styled(format!("{:<LW$}", "Total:"), dim),
                    Span::styled(format!("{:<VW$}", fmt_bytes(dev.total_space)), val),
                    Span::styled(format!("{:<LW$}", "Free:"), dim),
                    Span::styled(format!("{:<VW$}", fmt_bytes(dev.available_space)), val),
                    Span::styled(format!("{:<LW$}", "Used:"), dim),
                    Span::styled(format!("{:.1}%", dev.usage_pct), val),
                ]),
                stat_lines[2],
            );

            // Row 3: cumulative I/O totals
            frame.render_widget(
                Line::from(vec![
                    Span::styled(format!("{:<LW$}", "Read total:"), dim),
                    Span::styled(format!("{:<VW$}", fmt_bytes(dev.total_read_bytes)), val),
                    Span::styled(format!("{:<LW$}", "Write total:"), dim),
                    Span::styled(fmt_bytes(dev.total_write_bytes), val),
                ]),
                stat_lines[3],
            );
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
        let (read_hist, write_hist) = match self.history.get(name) {
            Some(h) => h,
            None => return Ok(()),
        };

        let read_data: Vec<(f64, f64)> = read_hist
            .iter()
            .enumerate()
            .map(|(i, &v)| (i as f64, v as f64))
            .collect();
        let write_data: Vec<(f64, f64)> = write_hist
            .iter()
            .enumerate()
            .map(|(i, &v)| (i as f64, v as f64))
            .collect();

        let y_max = read_hist
            .iter()
            .chain(write_hist.iter())
            .copied()
            .max()
            .unwrap_or(0)
            .max(1024) as f64; // floor at 1 KB/s so the axis is never zero-height

        let datasets = vec![
            Dataset::default()
                .name("Read")
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::new().fg(self.palette.accent))
                .data(&read_data),
            Dataset::default()
                .name("Write")
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::new().fg(self.palette.highlight))
                .data(&write_data),
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
        frame.render_widget(chart, sections[2]);

        // --- Bottom summary line ---
        if let Some(snap) = &self.latest
            && let Some(dev) = snap.devices.iter().find(|d| d.name == name)
        {
            let summary = Line::from(vec![
                Span::styled("Read: ", Style::new().fg(self.palette.dim)),
                Span::styled(
                    fmt_rate(dev.read_bytes),
                    Style::new().fg(self.palette.accent),
                ),
                Span::styled("  Write: ", Style::new().fg(self.palette.dim)),
                Span::styled(
                    fmt_rate(dev.write_bytes),
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
        stats::snapshots::{DiskDeviceSnapshot, DiskSnapshot},
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn device(name: &str) -> DiskDeviceSnapshot {
        DiskDeviceSnapshot {
            name: name.into(),
            read_bytes: 0,
            write_bytes: 0,
            usage_pct: 0.0,
            ..Default::default()
        }
    }

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
        comp.update(&Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(70, 8)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("disk_with_data", terminal.backend());
    }

    #[test]
    fn enter_switches_to_detail_and_requests_fullscreen() {
        let mut comp = DiskComponent::default();
        comp.set_focused(true);
        comp.update(&Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        comp.list_state.select(Some(0));
        let action = comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        assert!(
            matches!(comp.view, ListView::Detail { .. }),
            "Enter must switch to detail view"
        );
        assert!(
            matches!(action, Some(Action::ToggleFullScreen)),
            "Enter must request fullscreen when not already fullscreen"
        );
    }

    #[test]
    fn esc_closes_detail_view() {
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        comp.list_state.select(Some(0));
        comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        assert!(matches!(comp.view, ListView::Detail { .. }));
        comp.handle_key_event(key(KeyCode::Esc)).unwrap();
        assert!(matches!(comp.view, ListView::List));
    }

    #[test]
    fn q_closes_detail_view() {
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        comp.list_state.select(Some(0));
        comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        assert!(matches!(comp.view, ListView::Detail { .. }));
        comp.handle_key_event(key(KeyCode::Char('q'))).unwrap();
        assert!(matches!(comp.view, ListView::List));
    }

    #[test]
    fn esc_in_detail_closes_fullscreen_when_fullscreen() {
        let mut comp = DiskComponent::default();
        comp.set_focused(true);
        comp.update(&Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        // Simulate fullscreen being active (as the app would set via ToggleFullScreen).
        comp.is_fullscreen = true;
        comp.view = ListView::Detail {
            name: "sda".to_string(),
        };
        let action = comp.handle_key_event(key(KeyCode::Esc)).unwrap();
        assert!(
            matches!(action, Some(Action::ToggleFullScreen)),
            "Esc from detail must close fullscreen"
        );
        assert!(matches!(comp.view, ListView::List));
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
        let mut comp = DiskComponent::default();
        // Two devices so Down can actually advance from row 0 to row 1.
        let two_devices = DiskSnapshot {
            devices: vec![device("sda"), device("sdb")],
        };
        comp.update(&Action::DiskUpdate(two_devices)).unwrap();
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
        // 5 devices — fewer than PAGE (10) so clamping is exercised.
        let snap = DiskSnapshot {
            devices: (0..5).map(|i| device(&format!("sd{i}"))).collect(),
        };
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(snap)).unwrap();
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

    #[test]
    fn first_update_auto_selects_row_zero() {
        let mut comp = DiskComponent::default();
        assert_eq!(
            comp.list_state.selected(),
            None,
            "no selection before first update"
        );
        comp.update(&Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        assert_eq!(
            comp.list_state.selected(),
            Some(0),
            "first update must select row 0"
        );
    }

    #[test]
    fn selection_preserved_across_updates() {
        let two = DiskSnapshot {
            devices: vec![device("sda"), device("sdb")],
        };
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(two.clone())).unwrap();
        comp.list_state.select(Some(1));
        comp.update(&Action::DiskUpdate(two)).unwrap();
        assert_eq!(
            comp.list_state.selected(),
            Some(1),
            "selection must survive re-update"
        );
    }

    #[test]
    fn selection_clamped_when_list_shrinks() {
        let three = DiskSnapshot {
            devices: vec![device("sda"), device("sdb"), device("sdc")],
        };
        let one = DiskSnapshot {
            devices: vec![device("sda")],
        };
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(three)).unwrap();
        comp.list_state.select(Some(2));
        comp.update(&Action::DiskUpdate(one)).unwrap();
        assert_eq!(
            comp.list_state.selected(),
            Some(0),
            "selection must clamp to last row"
        );
    }

    #[test]
    fn selection_cleared_when_list_becomes_empty() {
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        assert_eq!(comp.list_state.selected(), Some(0));
        comp.update(&Action::DiskUpdate(DiskSnapshot { devices: vec![] }))
            .unwrap();
        assert_eq!(
            comp.list_state.selected(),
            None,
            "empty list must clear selection"
        );
    }

    #[test]
    fn duplicate_device_names_are_not_shown() {
        // sysinfo iterates mount points, so the same physical device can appear
        // multiple times. build_disk deduplicates before sending the snapshot;
        // this test documents the expected contract at the component boundary.
        let snap = DiskSnapshot {
            devices: vec![
                DiskDeviceSnapshot {
                    name: "nvme0n1p3".into(),
                    usage_pct: 42.0,
                    ..Default::default()
                },
                DiskDeviceSnapshot {
                    name: "nvme0n1p3".into(), // duplicate — bind mount
                    usage_pct: 42.0,
                    ..Default::default()
                },
                DiskDeviceSnapshot {
                    name: "sda".into(),
                    usage_pct: 10.0,
                    ..Default::default()
                },
            ],
        };
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(snap)).unwrap();
        let names: Vec<&str> = comp
            .latest
            .as_ref()
            .unwrap()
            .devices
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(
            names.len(),
            unique.len(),
            "device list must not contain duplicate names; got {names:?}"
        );
    }

    #[test]
    fn sorts_devices_by_name_before_rendering() {
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(DiskSnapshot {
            devices: vec![device("zfs0"), device("nvme0n1"), device("sda")],
        }))
        .unwrap();

        let names: Vec<&str> = comp
            .latest
            .as_ref()
            .expect("disk snapshot should be stored")
            .devices
            .iter()
            .map(|device| device.name.as_str())
            .collect();

        assert_eq!(names, vec!["nvme0n1", "sda", "zfs0"]);
    }

    #[test]
    fn history_accumulates_per_device() {
        let mut comp = DiskComponent::default();
        for _ in 0..50 {
            comp.update(&Action::DiskUpdate(DiskSnapshot::stub()))
                .unwrap();
        }
        for (read, write) in comp.history.values() {
            assert_eq!(read.len(), 50);
            assert_eq!(write.len(), 50);
        }
    }

    #[test]
    fn history_ring_buffer_bounded() {
        let mut comp = DiskComponent::default();
        for _ in 0..200 {
            comp.update(&Action::DiskUpdate(DiskSnapshot::stub()))
                .unwrap();
        }
        for (read, write) in comp.history.values() {
            assert!(read.len() <= HISTORY_LEN);
            assert!(write.len() <= HISTORY_LEN);
        }
    }

    #[test]
    fn renders_graph_view() {
        let mut comp = DiskComponent::default();
        for _ in 0..50 {
            comp.update(&Action::DiskUpdate(DiskSnapshot::stub()))
                .unwrap();
        }
        comp.list_state.select(Some(0));
        comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(90, 20)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("disk_graph_view", terminal.backend());
    }

    #[test]
    fn detail_view_consumes_unhandled_keys() {
        // Keys not explicitly handled in detail mode (e.g. Tab, focus-switch
        // chars) must return Some so the global app handler never sees them and
        // cannot shift focus or close the modal.
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        comp.list_state.select(Some(0));
        comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        assert!(matches!(comp.view, ListView::Detail { .. }));

        for code in [
            KeyCode::Tab,
            KeyCode::BackTab,
            KeyCode::Char('p'),
            KeyCode::Char('n'),
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
    fn slash_enters_filter_mode() {
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        comp.handle_key_event(key(KeyCode::Char('/'))).unwrap();
        assert!(
            matches!(comp.view, ListView::Filter { .. }),
            "/ must enter filter mode"
        );
    }

    #[test]
    fn filter_mode_char_updates_filter_and_view() {
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        comp.handle_key_event(key(KeyCode::Char('/'))).unwrap();
        comp.handle_key_event(key(KeyCode::Char('s'))).unwrap();
        comp.handle_key_event(key(KeyCode::Char('d'))).unwrap();
        assert_eq!(comp.filter, "sd");
        assert!(matches!(comp.view, ListView::Filter { ref input } if input == "sd"));
    }

    #[test]
    fn filter_mode_backspace_removes_char() {
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        comp.handle_key_event(key(KeyCode::Char('/'))).unwrap();
        comp.handle_key_event(key(KeyCode::Char('s'))).unwrap();
        comp.handle_key_event(key(KeyCode::Char('d'))).unwrap();
        comp.handle_key_event(key(KeyCode::Backspace)).unwrap();
        assert_eq!(comp.filter, "s");
    }

    #[test]
    fn filter_mode_esc_clears_filter_and_returns_to_list() {
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        comp.handle_key_event(key(KeyCode::Char('/'))).unwrap();
        comp.handle_key_event(key(KeyCode::Char('s'))).unwrap();
        comp.handle_key_event(key(KeyCode::Esc)).unwrap();
        assert_eq!(comp.filter, "", "Esc must clear filter");
        assert!(
            matches!(comp.view, ListView::List),
            "Esc must return to list"
        );
    }

    #[test]
    fn filter_mode_enter_keeps_filter_and_returns_to_list() {
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        comp.handle_key_event(key(KeyCode::Char('/'))).unwrap();
        comp.handle_key_event(key(KeyCode::Char('s'))).unwrap();
        comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        assert_eq!(comp.filter, "s", "Enter must keep filter");
        assert!(
            matches!(comp.view, ListView::List),
            "Enter must return to list"
        );
    }

    #[test]
    fn filter_narrows_list_for_navigation() {
        // Three devices; filter to only those matching "sda" (one device).
        let snap = DiskSnapshot {
            devices: vec![device("nvme0n1"), device("sda"), device("sdb")],
        };
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(snap)).unwrap();
        // Filter to "sda" only — Down must not advance past index 0.
        comp.handle_key_event(key(KeyCode::Char('/'))).unwrap();
        comp.handle_key_event(key(KeyCode::Char('s'))).unwrap();
        comp.handle_key_event(key(KeyCode::Char('d'))).unwrap();
        comp.handle_key_event(key(KeyCode::Char('a'))).unwrap();
        comp.handle_key_event(key(KeyCode::Enter)).unwrap(); // exit filter mode
        assert_eq!(comp.filter, "sda");
        // Down should be a no-op since filtered list has only 1 item.
        let sel_before = comp.list_state.selected();
        comp.handle_key_event(key(KeyCode::Down)).unwrap();
        assert_eq!(
            comp.list_state.selected(),
            sel_before,
            "Down must not advance past the last filtered item"
        );
    }

    #[test]
    fn filter_enter_opens_filtered_device() {
        // Two devices: nvme0n1 and sda. Filter to "nvme", then Enter.
        let snap = DiskSnapshot {
            devices: vec![device("nvme0n1"), device("sda")],
        };
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(snap)).unwrap();
        comp.handle_key_event(key(KeyCode::Char('/'))).unwrap();
        for c in "nvme".chars() {
            comp.handle_key_event(key(KeyCode::Char(c))).unwrap();
        }
        comp.handle_key_event(key(KeyCode::Enter)).unwrap(); // exit filter mode, keeping "nvme"
        // Row 0 of the filtered list is "nvme0n1". Enter should open it.
        comp.list_state.select(Some(0));
        comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        assert!(
            matches!(&comp.view, ListView::Detail { name } if name == "nvme0n1"),
            "Enter must open the filtered device, got: {:?}",
            comp.view
        );
    }

    #[test]
    fn filter_mode_swallows_keys() {
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        comp.handle_key_event(key(KeyCode::Char('/'))).unwrap();
        // Even unrecognised keys must return Some(Render) so the global handler
        // cannot shift focus while the user is typing.
        for code in [KeyCode::Tab, KeyCode::BackTab, KeyCode::F(1)] {
            let action = comp.handle_key_event(key(code)).unwrap();
            assert!(action.is_some(), "{code:?} must be consumed in filter mode");
            assert!(
                matches!(comp.view, ListView::Filter { .. }),
                "{code:?} must not exit filter mode"
            );
        }
    }

    #[test]
    fn compact_state_restored_after_fullscreen_exit() {
        let two = DiskSnapshot {
            devices: vec![device("sda"), device("sdb")],
        };
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(two)).unwrap();
        comp.set_focused(true);
        // Record initial selection (idx 0)
        assert_eq!(comp.list_state.selected(), Some(0));
        // Enter fullscreen
        comp.update(&Action::ToggleFullScreen).unwrap();
        assert!(comp.is_fullscreen);
        // Navigate down (change selection)
        comp.handle_key_event(key(KeyCode::Down)).unwrap();
        assert_eq!(
            comp.list_state.selected(),
            Some(1),
            "selection must change in fullscreen"
        );
        // Apply a filter
        comp.filter = "sd".to_string();
        // Exit fullscreen
        comp.update(&Action::ToggleFullScreen).unwrap();
        assert_eq!(
            comp.list_state.selected(),
            Some(0),
            "selection must be restored"
        );
        assert_eq!(comp.filter, "", "filter must be restored");
        assert!(matches!(comp.view, ListView::List));
        assert!(!comp.is_fullscreen);
    }

    /// Compact background pass renders the frozen pre-fullscreen list title.
    ///
    /// After entering fullscreen and typing a filter (which changes the title),
    /// the compact background pass must show the title from before fullscreen
    /// was opened (no filter in title).
    #[test]
    fn compact_background_shows_frozen_state_during_fullscreen() {
        let snap = DiskSnapshot {
            devices: vec![device("sda"), device("sdb")],
        };
        let mut comp = DiskComponent::default();
        comp.update(&Action::DiskUpdate(snap)).unwrap();
        comp.set_focused(true);
        comp.update(&Action::ToggleFullScreen).unwrap();

        // In fullscreen: apply a filter (changes the rendered title).
        comp.filter = "sda".to_string();

        let mut terminal = Terminal::new(TestBackend::new(70, 8)).unwrap();

        // Compact background pass (no begin_overlay_render): must NOT show filter.
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        let compact_bg = format!("{:?}", terminal.backend());
        assert!(
            !compact_bg.contains("/sda"),
            "compact background must NOT show live filter '/sda'; got: {compact_bg}"
        );

        // Overlay pass (begin_overlay_render): MUST show the live filter.
        comp.begin_overlay_render();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        let overlay = format!("{:?}", terminal.backend());
        assert!(
            overlay.contains("/sda") || overlay.contains("sda"),
            "overlay pass must show live filter 'sda'; got: {overlay}"
        );
    }
}
