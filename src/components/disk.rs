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
    stats::snapshots::DiskSnapshot,
    theme::ColorPalette,
};

pub const HISTORY_LEN: usize = 100;

/// Which view the disk panel is currently showing.
#[derive(Debug, Clone)]
enum DiskView {
    /// Text list of all devices with live read/write rates.
    List,
    /// Detail view for a specific device: stats header + read/write graph.
    Detail { name: String },
}

#[derive(Debug)]
pub struct DiskComponent {
    palette: ColorPalette,
    focus_key: char,
    latest: Option<DiskSnapshot>,
    list_state: ListState,
    /// Per-device ring buffers: (read_bytes_per_sec, write_bytes_per_sec).
    history: HashMap<String, (VecDeque<u64>, VecDeque<u64>)>,
    view: DiskView,
    focused: bool,
    is_fullscreen: bool,
}

impl DiskComponent {
    pub fn new(palette: ColorPalette, focus_key: char) -> Self {
        Self {
            palette,
            focus_key,
            latest: None,
            list_state: ListState::default(),
            history: HashMap::new(),
            view: DiskView::List,
            focused: false,
            is_fullscreen: false,
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
        if !focused {
            self.is_fullscreen = false;
        }
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match &self.view.clone() {
            DiskView::Detail { .. } => {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                        self.view = DiskView::List;
                        // If we opened fullscreen on Enter, close it now.
                        let action = if self.is_fullscreen {
                            Action::ToggleFullScreen
                        } else {
                            Action::Render
                        };
                        return Ok(Some(action));
                    }
                    _ => {}
                }
            }
            DiskView::List => {
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
                        if let Some(dev) = snap.devices.get(idx) {
                            self.view = DiskView::Detail {
                                name: dev.name.clone(),
                            };
                            // Open the fullscreen modal unless already fullscreen.
                            let action = if !self.is_fullscreen {
                                Action::ToggleFullScreen
                            } else {
                                Action::Render
                            };
                            return Ok(Some(action));
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
            Action::DiskUpdate(snap) => {
                let mut snap = snap;
                snap.devices
                    .sort_by(|left, right| left.name.cmp(&right.name));
                // sysinfo can report the same device multiple times (one per mount
                // point). Sort puts duplicates adjacent; dedup removes them.
                snap.devices.dedup_by(|a, b| a.name == b.name);
                // Select first row on initial data; keep selection in bounds after refresh
                let len = snap.devices.len();
                if len == 0 {
                    self.list_state.select(None);
                } else {
                    let sel = self.list_state.selected().unwrap_or(0).min(len - 1);
                    self.list_state.select(Some(sel));
                }
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
            DiskView::List => self.draw_list(frame, area),
            DiskView::Detail { name } => self.draw_detail(frame, area, &name),
        }
    }
}

impl DiskComponent {
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
        let block = self.border_block("DISK");
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
        let items: Vec<ListItem> = snap
            .devices
            .iter()
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
        let stats_rows: u16 = if inner.height >= 12 { 5 } else { 0 };

        let sections = Layout::vertical([
            Constraint::Length(stats_rows),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .split(inner);

        // --- Stats header ---
        if stats_rows > 0
            && let Some(snap) = &self.latest.clone()
            && let Some(dev) = snap.devices.iter().find(|d| d.name == name)
        {
            let dim = Style::new().fg(self.palette.dim);
            let val = Style::new().fg(self.palette.fg);

            let stat_lines = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(sections[0]);

            // Row 0: mount point and disk kind
            frame.render_widget(
                Line::from(vec![
                    Span::styled("Mount: ", dim),
                    Span::styled(dev.mount_point.clone(), val),
                    Span::styled("   Type: ", dim),
                    Span::styled(dev.kind.clone(), val),
                ]),
                stat_lines[0],
            );

            // Row 1: filesystem, read-only, removable
            frame.render_widget(
                Line::from(vec![
                    Span::styled("FS: ", dim),
                    Span::styled(dev.file_system.clone(), val),
                    Span::styled("   RO: ", dim),
                    Span::styled(if dev.is_read_only { "yes" } else { "no" }, val),
                    Span::styled("   Removable: ", dim),
                    Span::styled(if dev.is_removable { "yes" } else { "no" }, val),
                ]),
                stat_lines[1],
            );

            // Row 2: total and free space
            frame.render_widget(
                Line::from(vec![
                    Span::styled("Total: ", dim),
                    Span::styled(fmt_bytes(dev.total_space), val),
                    Span::styled("   Free: ", dim),
                    Span::styled(fmt_bytes(dev.available_space), val),
                ]),
                stat_lines[2],
            );

            // Row 3: usage percentage
            frame.render_widget(
                Line::from(vec![
                    Span::styled("Used: ", dim),
                    Span::styled(format!("{:.1}%", dev.usage_pct), val),
                ]),
                stat_lines[3],
            );

            // Row 4: cumulative I/O totals
            frame.render_widget(
                Line::from(vec![
                    Span::styled("Read total: ", dim),
                    Span::styled(fmt_bytes(dev.total_read_bytes), val),
                    Span::styled("   Write total: ", dim),
                    Span::styled(fmt_bytes(dev.total_write_bytes), val),
                ]),
                stat_lines[4],
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
        frame.render_widget(chart, sections[1]);

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
        comp.update(Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(70, 8)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("disk_with_data", terminal.backend());
    }

    #[test]
    fn enter_switches_to_detail_and_requests_fullscreen() {
        let mut comp = DiskComponent::default();
        comp.set_focused(true);
        comp.update(Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        comp.list_state.select(Some(0));
        let action = comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        assert!(
            matches!(comp.view, DiskView::Detail { .. }),
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
        comp.update(Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        comp.list_state.select(Some(0));
        comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        assert!(matches!(comp.view, DiskView::Detail { .. }));
        comp.handle_key_event(key(KeyCode::Esc)).unwrap();
        assert!(matches!(comp.view, DiskView::List));
    }

    #[test]
    fn q_closes_detail_view() {
        let mut comp = DiskComponent::default();
        comp.update(Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        comp.list_state.select(Some(0));
        comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        assert!(matches!(comp.view, DiskView::Detail { .. }));
        comp.handle_key_event(key(KeyCode::Char('q'))).unwrap();
        assert!(matches!(comp.view, DiskView::List));
    }

    #[test]
    fn esc_in_detail_closes_fullscreen_when_fullscreen() {
        let mut comp = DiskComponent::default();
        comp.set_focused(true);
        comp.update(Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        // Simulate fullscreen being active (as the app would set via ToggleFullScreen).
        comp.is_fullscreen = true;
        comp.view = DiskView::Detail {
            name: "sda".to_string(),
        };
        let action = comp.handle_key_event(key(KeyCode::Esc)).unwrap();
        assert!(
            matches!(action, Some(Action::ToggleFullScreen)),
            "Esc from detail must close fullscreen"
        );
        assert!(matches!(comp.view, DiskView::List));
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
        // 5 devices — fewer than PAGE (10) so clamping is exercised.
        let snap = DiskSnapshot {
            devices: (0..5).map(|i| device(&format!("sd{i}"))).collect(),
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

    #[test]
    fn first_update_auto_selects_row_zero() {
        let mut comp = DiskComponent::default();
        assert_eq!(
            comp.list_state.selected(),
            None,
            "no selection before first update"
        );
        comp.update(Action::DiskUpdate(DiskSnapshot::stub()))
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
        comp.update(Action::DiskUpdate(two.clone())).unwrap();
        comp.list_state.select(Some(1));
        comp.update(Action::DiskUpdate(two)).unwrap();
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
        comp.update(Action::DiskUpdate(three)).unwrap();
        comp.list_state.select(Some(2));
        comp.update(Action::DiskUpdate(one)).unwrap();
        assert_eq!(
            comp.list_state.selected(),
            Some(0),
            "selection must clamp to last row"
        );
    }

    #[test]
    fn selection_cleared_when_list_becomes_empty() {
        let mut comp = DiskComponent::default();
        comp.update(Action::DiskUpdate(DiskSnapshot::stub()))
            .unwrap();
        assert_eq!(comp.list_state.selected(), Some(0));
        comp.update(Action::DiskUpdate(DiskSnapshot { devices: vec![] }))
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
        comp.update(Action::DiskUpdate(snap)).unwrap();
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
        comp.update(Action::DiskUpdate(DiskSnapshot {
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
            comp.update(Action::DiskUpdate(DiskSnapshot::stub()))
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
            comp.update(Action::DiskUpdate(DiskSnapshot::stub()))
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
            comp.update(Action::DiskUpdate(DiskSnapshot::stub()))
                .unwrap();
        }
        comp.list_state.select(Some(0));
        comp.handle_key_event(key(KeyCode::Enter)).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(70, 20)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("disk_graph_view", terminal.backend());
    }
}
