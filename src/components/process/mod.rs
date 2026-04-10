// SPDX-License-Identifier: GPL-3.0-only

//! Process panel — sortable, filterable process table with detail inspector.
//!
//! Supports flat list and tree view modes, per-column sorting, incremental
//! name/PID/status filtering, a two-column detail inspector (Enter), and
//! `SIGTERM` kill with confirmation dialog (k).

pub mod filter;
pub mod sort;
pub mod tree;

use anyhow::{Context, Result};
use chrono::TimeZone;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Row, Table, TableState, Wrap},
};

use crate::{
    action::Action,
    components::{Component, fmt_rate_col, keyed_title},
    config::ProcessConfig,
    stats::snapshots::ProcessEntry,
    theme::ColorPalette,
};
use filter::ProcessFilter;
use sort::{SortColumn, SortDir, sort_processes};
use tree::TreeRow;

/// Flat list or tree hierarchy view mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessViewMode {
    Flat,
    Tree,
}

/// Returns a color from the palette based on CPU usage percentage.
/// >95% → critical (red), >80% → warn (orange), else → fg (normal).
fn cpu_color(pct: f32, palette: &ColorPalette) -> Color {
    if pct >= 95.0 {
        palette.critical
    } else if pct >= 80.0 {
        palette.warn
    } else {
        palette.fg
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessState {
    NormalList,
    FilterMode {
        input: String,
    },
    DetailView {
        pid: u32,
    },
    KillConfirm {
        pid: u32,
        name: String,
        /// Which button currently has keyboard focus. `false` = Cancel (default).
        ok_focused: bool,
    },
    /// Kill command was attempted but failed — show an error dialog to the user.
    KillError {
        message: String,
    },
}

struct ProcessCompactSnapshot {
    selected: Option<usize>,
    sort_col: SortColumn,
    sort_dir: SortDir,
    filter: ProcessFilter,
    state: ProcessState,
    view_mode: ProcessViewMode,
    expanded: std::collections::HashSet<u32>,
    tree_rows: Vec<TreeRow>,
    /// Cached displayed list at the moment fullscreen was entered.  Used by the
    /// compact background pass so it renders the same rows without calling
    /// `refresh_display()` with live filter/sort state.
    displayed: Vec<ProcessEntry>,
}

pub struct ProcessComponent {
    palette: ColorPalette,
    focus_key: char,
    raw: Vec<ProcessEntry>,
    displayed: Vec<ProcessEntry>,
    table_state: TableState,
    filter: ProcessFilter,
    sort_col: SortColumn,
    sort_dir: SortDir,
    pub state: ProcessState,
    focused: bool,
    is_fullscreen: bool,
    view_mode: ProcessViewMode,
    /// Set of PIDs whose children are visible in tree mode.  When a process
    /// first appears it is added here (expanded by default).
    expanded: std::collections::HashSet<u32>,
    /// Flattened tree rows — populated by `refresh_display()` when in Tree mode.
    tree_rows: Vec<TreeRow>,
    /// True when the last draw used the extended column layout (fullscreen overlay
    /// or area width ≥ 120).  Stored here so the sort-cycle key handler can use
    /// the column order that matches what is actually visible on screen.
    is_wide_layout: bool,
    compact_snapshot: Option<ProcessCompactSnapshot>,
    /// One-shot flag set by `begin_overlay_render()`.  Consumed at the start of
    /// `draw()` to distinguish the compact background pass from the overlay pass.
    rendering_as_overlay: bool,
}

impl std::fmt::Debug for ProcessComponent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessComponent")
            .field("state", &self.state)
            .field("sort_col", &self.sort_col)
            .field("sort_dir", &self.sort_dir)
            .field("filter", &self.filter)
            .field("displayed_count", &self.displayed.len())
            .finish()
    }
}

impl Default for ProcessComponent {
    fn default() -> Self {
        let mut table_state = TableState::default();
        table_state.select(Some(0));
        Self {
            palette: ColorPalette::dark(),
            focus_key: 'p',
            raw: Vec::new(),
            displayed: Vec::new(),
            table_state,
            filter: ProcessFilter::None,
            sort_col: SortColumn::default(),
            sort_dir: SortDir::default(),
            state: ProcessState::NormalList,
            focused: false,
            is_fullscreen: false,
            is_wide_layout: false,
            compact_snapshot: None,
            rendering_as_overlay: false,
            view_mode: ProcessViewMode::Flat,
            expanded: std::collections::HashSet::new(),
            tree_rows: Vec::new(),
        }
    }
}

impl ProcessComponent {
    pub fn new(palette: ColorPalette, focus_key: char, config: &ProcessConfig) -> Self {
        let sort_col = config.default_sort.parse().unwrap_or_default();
        let sort_dir = if config.default_sort_dir == "asc" {
            SortDir::Asc
        } else {
            SortDir::Desc
        };
        let view_mode = if config.show_tree {
            ProcessViewMode::Tree
        } else {
            ProcessViewMode::Flat
        };
        Self {
            palette,
            focus_key,
            sort_col,
            sort_dir,
            view_mode,
            ..Default::default()
        }
    }

    fn refresh_display(&mut self) {
        match self.view_mode {
            ProcessViewMode::Flat => {
                self.tree_rows.clear();
                let mut list: Vec<ProcessEntry> = self
                    .raw
                    .iter()
                    .filter(|p| !p.is_thread && self.filter.matches(p))
                    .cloned()
                    .collect();
                sort_processes(&mut list, self.sort_col, self.sort_dir);
                self.displayed = list;
            }
            ProcessViewMode::Tree => {
                // Expand newly-appeared PIDs by default.  PIDs that were
                // explicitly collapsed by the user are NOT in `expanded`
                // and must stay that way.
                let known: std::collections::HashSet<u32> =
                    self.tree_rows.iter().map(|r| r.entry.pid).collect();
                for p in &self.raw {
                    if !known.contains(&p.pid) {
                        self.expanded.insert(p.pid);
                    }
                }
                self.tree_rows = tree::build_tree(
                    &self.raw,
                    self.sort_col,
                    self.sort_dir,
                    &self.filter,
                    &self.expanded,
                );
                self.displayed = self.tree_rows.iter().map(|r| r.entry.clone()).collect();
            }
        }
        if self.displayed.is_empty() {
            self.table_state.select(None);
        } else {
            let max = self.displayed.len() - 1;
            let sel = self.table_state.selected().unwrap_or(0).min(max);
            self.table_state.select(Some(sel));
        }
    }

    fn restore_compact_snapshot(&mut self) {
        if let Some(snap) = self.compact_snapshot.take() {
            self.sort_col = snap.sort_col;
            self.sort_dir = snap.sort_dir;
            self.filter = snap.filter;
            self.state = snap.state;
            self.view_mode = snap.view_mode;
            self.expanded = snap.expanded;
            self.tree_rows = snap.tree_rows;
            self.refresh_display();
            let max = self.displayed.len().saturating_sub(1);
            self.table_state.select(snap.selected.map(|s| s.min(max)));
        }
        self.is_fullscreen = false;
    }

    /// Render the compact sidebar appearance using the frozen snapshot state.
    ///
    /// Temporarily swaps live fields (sort, filter, state, displayed list,
    /// table selection) with snapshot values, calls `draw()` with
    /// `is_fullscreen = false`, then restores live state.  Setting
    /// `is_fullscreen` to false prevents the recursive `draw()` call from
    /// re-entering this method.
    fn draw_compact_background(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let Some(snap) = self.compact_snapshot.take() else {
            return Ok(()); // no snapshot yet — render nothing
        };

        let live_sort_col = std::mem::replace(&mut self.sort_col, snap.sort_col);
        let live_sort_dir = std::mem::replace(&mut self.sort_dir, snap.sort_dir);
        let live_filter = std::mem::replace(&mut self.filter, snap.filter.clone());
        let live_state = std::mem::replace(&mut self.state, snap.state.clone());
        let live_view_mode = std::mem::replace(&mut self.view_mode, snap.view_mode);
        let live_expanded = std::mem::replace(&mut self.expanded, snap.expanded.clone());
        let live_tree_rows = std::mem::replace(&mut self.tree_rows, snap.tree_rows.clone());
        let live_displayed = std::mem::replace(&mut self.displayed, snap.displayed.clone());
        let live_fs = std::mem::replace(&mut self.is_fullscreen, false);
        let mut tmp_table = TableState::default();
        tmp_table.select(snap.selected);
        let live_table = std::mem::replace(&mut self.table_state, tmp_table);
        // rendering_as_overlay is already false (consumed at top of draw()).

        let result = self.draw(frame, area);

        self.sort_col = live_sort_col;
        self.sort_dir = live_sort_dir;
        self.filter = live_filter;
        self.state = live_state;
        self.view_mode = live_view_mode;
        self.expanded = live_expanded;
        self.tree_rows = live_tree_rows;
        self.displayed = live_displayed;
        self.is_fullscreen = live_fs;
        self.table_state = live_table;
        self.compact_snapshot = Some(snap);

        result
    }
}

impl Component for ProcessComponent {
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
        // Handle FilterMode first via mem::replace to take owned `input` without
        // cloning the entire enum — avoids the borrow-then-mutate conflict.
        if matches!(&self.state, ProcessState::FilterMode { .. }) {
            let input = match std::mem::replace(&mut self.state, ProcessState::NormalList) {
                ProcessState::FilterMode { input } => input,
                _ => unreachable!("checked above"),
            };
            match key.code {
                KeyCode::Esc => {
                    self.filter = ProcessFilter::None;
                    // self.state is already NormalList from the replace
                    self.refresh_display();
                }
                KeyCode::Enter => {
                    // self.state is already NormalList from the replace
                }
                KeyCode::Backspace => {
                    let mut s = input;
                    s.pop();
                    self.filter = ProcessFilter::parse(&s);
                    self.state = ProcessState::FilterMode { input: s };
                    self.refresh_display();
                }
                KeyCode::Char(c) => {
                    let mut s = input;
                    s.push(c);
                    self.filter = ProcessFilter::parse(&s);
                    self.state = ProcessState::FilterMode { input: s };
                    self.refresh_display();
                }
                _ => {
                    // Key not handled; restore state
                    self.state = ProcessState::FilterMode { input };
                }
            }
            return Ok(Some(Action::Render));
        }

        // For the remaining states extract only scalar/Copy data so the borrow
        // on self.state is released before we mutate it below.
        let (is_detail, is_kill_confirm, kill_pid, kill_ok_focused, is_kill_error) =
            match &self.state {
                ProcessState::DetailView { .. } => (true, false, 0u32, false, false),
                ProcessState::KillConfirm {
                    pid, ok_focused, ..
                } => (false, true, *pid, *ok_focused, false),
                ProcessState::KillError { .. } => (false, false, 0u32, false, true),
                _ => (false, false, 0u32, false, false),
            };

        if is_detail {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.state = ProcessState::NormalList;
                    return Ok(Some(Action::Render));
                }
                _ => {}
            }
            // Swallow all other keys so they don't reach the global handler.
            return Ok(Some(Action::Render));
        }

        if is_kill_confirm {
            let pid = kill_pid;
            match key.code {
                KeyCode::Tab | KeyCode::BackTab => {
                    if let ProcessState::KillConfirm { ok_focused, .. } = &mut self.state {
                        *ok_focused = !*ok_focused;
                    }
                }
                KeyCode::Enter => {
                    if kill_ok_focused {
                        if let Err(e) = kill_process(pid) {
                            self.state = ProcessState::KillError {
                                message: e.to_string(),
                            };
                        } else {
                            self.state = ProcessState::NormalList;
                        }
                    } else {
                        self.state = ProcessState::NormalList;
                    }
                }
                KeyCode::Esc => {
                    self.state = ProcessState::NormalList;
                }
                _ => {}
            }
            // Swallow all other keys so they don't reach the global handler.
            return Ok(Some(Action::Render));
        }

        if is_kill_error {
            match key.code {
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' ') => {
                    self.state = ProcessState::NormalList;
                    return Ok(Some(Action::Render));
                }
                _ => {}
            }
            // Swallow all other keys so they don't reach the global handler.
            return Ok(Some(Action::Render));
        }

        // NormalList
        {
            const PAGE: usize = 10;
            match key.code {
                KeyCode::Down => {
                    let next = self
                        .table_state
                        .selected()
                        .map(|i| i + 1)
                        .unwrap_or(0)
                        .min(self.displayed.len().saturating_sub(1));
                    self.table_state.select(Some(next));
                    return Ok(Some(Action::Render));
                }
                KeyCode::Up => {
                    let prev = self
                        .table_state
                        .selected()
                        .and_then(|i| i.checked_sub(1))
                        .unwrap_or(0);
                    self.table_state.select(Some(prev));
                    return Ok(Some(Action::Render));
                }
                KeyCode::PageDown => {
                    let next = self
                        .table_state
                        .selected()
                        .map(|i| i + PAGE)
                        .unwrap_or(0)
                        .min(self.displayed.len().saturating_sub(1));
                    self.table_state.select(Some(next));
                    return Ok(Some(Action::Render));
                }
                KeyCode::PageUp => {
                    let prev = self
                        .table_state
                        .selected()
                        .map(|i| i.saturating_sub(PAGE))
                        .unwrap_or(0);
                    self.table_state.select(Some(prev));
                    return Ok(Some(Action::Render));
                }
                KeyCode::Enter => {
                    if let Some(sel) = self.table_state.selected()
                        && let Some(p) = self.displayed.get(sel)
                    {
                        self.state = ProcessState::DetailView { pid: p.pid };
                        return Ok(Some(Action::Render));
                    }
                }
                KeyCode::Char('/') => {
                    self.state = ProcessState::FilterMode {
                        input: String::new(),
                    };
                    return Ok(Some(Action::Render));
                }
                KeyCode::Char('k') => {
                    if let Some(sel) = self.table_state.selected()
                        && let Some(p) = self.displayed.get(sel)
                    {
                        // Threads cannot be killed directly; target the owning
                        // process instead.  Threads only appear in tree view, so
                        // `parent_pid` is the process PID (flat view filters them out).
                        let (kill_pid, kill_name) = if p.is_thread {
                            let owner_pid = p.parent_pid.unwrap_or(p.pid);
                            self.raw
                                .iter()
                                .find(|e| e.pid == owner_pid)
                                .map(|o| (o.pid, o.name.clone()))
                                .unwrap_or((p.pid, p.name.clone()))
                        } else {
                            (p.pid, p.name.clone())
                        };
                        self.state = ProcessState::KillConfirm {
                            pid: kill_pid,
                            name: kill_name,
                            ok_focused: false,
                        };
                        return Ok(Some(Action::Render));
                    }
                }
                KeyCode::Char('s') => {
                    // Cycle through sortable columns in the order they appear
                    // on screen so the indicator always moves left-to-right.
                    // Normal view:   PID | User | Name | CPU% | MEM | Status
                    // Extended view: PID | User | PR | NI | VIRT | RES | SHR | S | %CPU | %MEM | TIME | Command
                    let cols: &[SortColumn] = if self.is_wide_layout {
                        &[
                            SortColumn::Pid,
                            SortColumn::User,
                            SortColumn::Priority,
                            SortColumn::Nice,
                            SortColumn::Virt,
                            SortColumn::Res,
                            SortColumn::Shr,
                            SortColumn::Status,
                            SortColumn::Cpu,
                            SortColumn::Mem,
                            SortColumn::Time,
                            SortColumn::Name,
                        ]
                    } else {
                        &[
                            SortColumn::Pid,
                            SortColumn::User,
                            SortColumn::Name,
                            SortColumn::Cpu,
                            SortColumn::Mem,
                            SortColumn::Status,
                        ]
                    };
                    let idx = cols.iter().position(|c| c == &self.sort_col).unwrap_or(0);
                    self.sort_col = cols[(idx + 1) % cols.len()];
                    self.refresh_display();
                    return Ok(Some(Action::Render));
                }
                KeyCode::Char('S') => {
                    self.sort_dir = if self.sort_dir == SortDir::Asc {
                        SortDir::Desc
                    } else {
                        SortDir::Asc
                    };
                    self.refresh_display();
                    return Ok(Some(Action::Render));
                }
                KeyCode::Char('t') => {
                    self.view_mode = match self.view_mode {
                        ProcessViewMode::Flat => ProcessViewMode::Tree,
                        ProcessViewMode::Tree => ProcessViewMode::Flat,
                    };
                    self.refresh_display();
                    return Ok(Some(Action::Render));
                }
                KeyCode::Char(' ') if self.view_mode == ProcessViewMode::Tree => {
                    // Toggle expand/collapse for the selected tree node.
                    if let Some(sel) = self.table_state.selected()
                        && let Some(row) = self.tree_rows.get(sel)
                    {
                        let pid = row.entry.pid;
                        if row.has_children {
                            if self.expanded.contains(&pid) {
                                self.expanded.remove(&pid);
                            } else {
                                self.expanded.insert(pid);
                            }
                            self.refresh_display();
                        }
                    }
                    return Ok(Some(Action::Render));
                }
                _ => {}
            }
            Ok(None)
        }
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        match action {
            Action::ProcUpdate(snap) => {
                self.raw = snap.processes.clone();
                self.refresh_display();
            }
            Action::ToggleFullScreen if self.focused => {
                if !self.is_fullscreen {
                    let safe_state = match &self.state {
                        ProcessState::NormalList | ProcessState::FilterMode { .. } => {
                            self.state.clone()
                        }
                        _ => ProcessState::NormalList,
                    };
                    self.compact_snapshot = Some(ProcessCompactSnapshot {
                        selected: self.table_state.selected(),
                        sort_col: self.sort_col,
                        sort_dir: self.sort_dir,
                        filter: self.filter.clone(),
                        state: safe_state,
                        view_mode: self.view_mode,
                        expanded: self.expanded.clone(),
                        tree_rows: self.tree_rows.clone(),
                        displayed: self.displayed.clone(),
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

        let tree_tag = if self.view_mode == ProcessViewMode::Tree {
            " [tree]"
        } else {
            ""
        };
        let title_rest = match &self.state {
            ProcessState::FilterMode { input } => {
                format!("rocesses{tree_tag} [filter: {}▌]", input)
            }
            _ => format!("rocesses{tree_tag}"),
        };
        let border_color = if self.focused {
            self.palette.accent
        } else {
            self.palette.border
        };
        let block = Block::default()
            .title(keyed_title(self.focus_key, &title_rest, &self.palette))
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border_color));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Kill error dialog — shown when kill -TERM fails
        if let ProcessState::KillError { message } = &self.state {
            let dialog_w = (inner.width * 3 / 4).max(30).min(inner.width);
            let dialog_h = 6_u16.min(inner.height);
            let dialog = Rect::new(
                inner.x + (inner.width.saturating_sub(dialog_w)) / 2,
                inner.y + (inner.height.saturating_sub(dialog_h)) / 2,
                dialog_w,
                dialog_h,
            );
            frame.render_widget(Clear, dialog);
            let err_block = Block::default()
                .title(Span::styled(
                    " Kill Failed ",
                    Style::new().fg(self.palette.critical).bold(),
                ))
                .borders(Borders::ALL)
                .border_style(Style::new().fg(self.palette.critical));
            let dialog_inner = err_block.inner(dialog);
            frame.render_widget(err_block, dialog);
            let body = Paragraph::new(vec![
                Line::from(Span::styled(
                    message.as_str(),
                    Style::new().fg(self.palette.fg),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "[ Enter / Esc ]  OK",
                    Style::new().fg(self.palette.dim),
                )),
            ])
            .wrap(Wrap { trim: false })
            .centered();
            frame.render_widget(body, dialog_inner);
            return Ok(());
        }

        // Kill confirm dialog
        if let ProcessState::KillConfirm {
            pid,
            name,
            ok_focused,
        } = &self.state
        {
            let pid = *pid;
            let name = name.clone();
            let ok_focused = *ok_focused;

            let msg = format!("Kill \"{}\" (pid {})?", name, pid);
            let dialog_w = (msg.len() as u16 + 6).max(32).min(inner.width);
            let dialog_h = 6_u16.min(inner.height);
            let dialog = Rect::new(
                inner.x + (inner.width.saturating_sub(dialog_w)) / 2,
                inner.y + (inner.height.saturating_sub(dialog_h)) / 2,
                dialog_w,
                dialog_h,
            );
            frame.render_widget(Clear, dialog);

            let confirm_block = Block::default()
                .title(Span::styled(
                    " Kill Process ",
                    Style::new().fg(self.palette.critical).bold(),
                ))
                .borders(Borders::ALL)
                .border_style(Style::new().fg(self.palette.critical));
            let dialog_inner = confirm_block.inner(dialog);
            frame.render_widget(confirm_block, dialog);

            let ok_style = if ok_focused {
                Style::new()
                    .fg(self.palette.bg)
                    .bg(self.palette.critical)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(self.palette.dim)
            };
            let cancel_style = if !ok_focused {
                Style::new()
                    .fg(self.palette.bg)
                    .bg(self.palette.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(self.palette.dim)
            };

            let body = Paragraph::new(vec![
                Line::from(Span::styled(msg, Style::new().fg(self.palette.fg))),
                Line::from(""),
                Line::from(vec![
                    Span::styled("[ OK ]", ok_style),
                    Span::raw("    "),
                    Span::styled("[ Cancel ]", cancel_style),
                ]),
            ])
            .centered();
            frame.render_widget(body, dialog_inner);
            return Ok(());
        }

        // Detail view overlay
        if let ProcessState::DetailView { pid } = &self.state {
            let pid = *pid;
            if let Some(p) = self.displayed.iter().find(|p| p.pid == pid).cloned() {
                let chunks = Layout::vertical([
                    Constraint::Length(4),
                    Constraint::Length(1),
                    Constraint::Min(0),
                    Constraint::Length(1),
                    Constraint::Length(3),
                ])
                .split(inner);
                let cols =
                    Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .split(chunks[2]);

                let command = if p.cmd.is_empty() {
                    p.name.clone()
                } else {
                    p.cmd.join(" ")
                };
                let header = vec![
                    detail_header_kv("Name:", p.name.clone(), &self.palette),
                    detail_header_kv("Command:", command, &self.palette),
                    detail_header_kv("Exe:", fmt_opt_str(p.exe.as_deref()), &self.palette),
                    detail_header_kv("CWD:", fmt_opt_str(p.cwd.as_deref()), &self.palette),
                ];
                let pairs = [
                    (
                        detail_kv("PID", p.pid.to_string(), &self.palette),
                        detail_kv("PPID", fmt_opt(p.parent_pid), &self.palette),
                    ),
                    (
                        detail_kv("User", p.user.clone(), &self.palette),
                        detail_kv("Status", p.status.to_string(), &self.palette),
                    ),
                    (
                        detail_kv(
                            "Type",
                            if p.is_thread { "thread" } else { "process" },
                            &self.palette,
                        ),
                        detail_kv("Session", fmt_opt(p.session_id), &self.palette),
                    ),
                    (
                        detail_kv("CPU", format!("{:.1}%", p.cpu_pct), &self.palette),
                        detail_kv(
                            "CPU time",
                            fmt_duration_long(p.cpu_time_secs as u64),
                            &self.palette,
                        ),
                    ),
                    (
                        detail_kv(
                            "User CPU",
                            fmt_duration_long(p.user_cpu_time_secs as u64),
                            &self.palette,
                        ),
                        detail_kv(
                            "Sys CPU",
                            fmt_duration_long(p.system_cpu_time_secs as u64),
                            &self.palette,
                        ),
                    ),
                    (
                        detail_kv(
                            "MEM",
                            format!("{:.1}% ({})", p.mem_pct, fmt_bytes(p.mem_bytes)),
                            &self.palette,
                        ),
                        detail_kv("VIRT", fmt_bytes(p.virt_bytes), &self.palette),
                    ),
                    (
                        detail_kv("SHR", fmt_bytes(p.shr_bytes), &self.palette),
                        detail_kv("Swap", fmt_opt_bytes(p.swap_bytes), &self.palette),
                    ),
                    (
                        detail_kv("Threads", p.threads.to_string(), &self.palette),
                        detail_kv("FDs", fmt_opt(p.fd_count), &self.palette),
                    ),
                    (
                        detail_kv("PR", p.priority.to_string(), &self.palette),
                        detail_kv("NI", p.nice.to_string(), &self.palette),
                    ),
                    (
                        detail_kv("Started", fmt_start_time(p.start_time), &self.palette),
                        detail_kv("Runtime", fmt_duration_long(p.run_time), &self.palette),
                    ),
                    (
                        detail_kv("Minflt", p.minor_faults.to_string(), &self.palette),
                        detail_kv("Majflt", p.major_faults.to_string(), &self.palette),
                    ),
                    (
                        detail_kv("Vol CS", fmt_opt(p.voluntary_ctxt_switches), &self.palette),
                        detail_kv(
                            "Invol CS",
                            fmt_opt(p.nonvoluntary_ctxt_switches),
                            &self.palette,
                        ),
                    ),
                    (
                        detail_kv("I/O read", fmt_bytes(p.read_bytes), &self.palette),
                        detail_kv("I/O write", fmt_bytes(p.write_bytes), &self.palette),
                    ),
                    (
                        detail_kv("Read calls", fmt_opt(p.io_read_calls), &self.palette),
                        detail_kv("Write calls", fmt_opt(p.io_write_calls), &self.palette),
                    ),
                    (
                        detail_kv("Read chars", fmt_opt_bytes(p.io_read_chars), &self.palette),
                        detail_kv(
                            "Write chars",
                            fmt_opt_bytes(p.io_write_chars),
                            &self.palette,
                        ),
                    ),
                    (
                        detail_kv("TTY", fmt_opt_str(p.tty.as_deref()), &self.palette),
                        detail_kv("Root", fmt_opt_str(p.root.as_deref()), &self.palette),
                    ),
                    (
                        detail_kv("GID", fmt_opt_str(p.group.as_deref()), &self.palette),
                        detail_kv(
                            "EGID",
                            fmt_opt_str(p.effective_group.as_deref()),
                            &self.palette,
                        ),
                    ),
                    (
                        detail_kv(
                            "EUID",
                            fmt_opt_str(p.effective_user.as_deref()),
                            &self.palette,
                        ),
                        detail_kv(
                            "Cancelled W",
                            fmt_opt_bytes(p.cancelled_write_bytes),
                            &self.palette,
                        ),
                    ),
                ];
                let left: Vec<_> = pairs.iter().map(|(left, _)| left.clone()).collect();
                let right: Vec<_> = pairs.iter().map(|(_, right)| right.clone()).collect();

                frame.render_widget(Paragraph::new(header).wrap(Wrap { trim: false }), chunks[0]);
                frame.render_widget(horizontal_rule(inner.width), chunks[1]);
                frame.render_widget(Paragraph::new(left).wrap(Wrap { trim: false }), cols[0]);
                frame.render_widget(Paragraph::new(right).wrap(Wrap { trim: false }), cols[1]);
                frame.render_widget(horizontal_rule(inner.width), chunks[3]);
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        "[Esc/q] back",
                        Style::new().fg(self.palette.dim),
                    )))
                    .centered(),
                    chunks[4],
                );
                return Ok(());
            }
        }

        // Use extended columns only when the area is wide enough.  The compact
        // sidebar slot is typically <120 cols, so draw_normal runs there.  The
        // fullscreen modal (95% of terminal) exceeds 120 cols on any ≥127-col
        // terminal, so draw_fullscreen runs there.  Basing this solely on
        // area.width (not is_fullscreen) prevents the extended layout from
        // bleeding into the compact sidebar pass that runs before the overlay.
        self.is_wide_layout = area.width >= 120;
        if self.is_wide_layout {
            self.draw_fullscreen(frame, inner)
        } else {
            self.draw_normal(frame, inner)
        }
    }
}

impl ProcessComponent {
    fn draw_normal(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let dir_sym = if self.sort_dir == SortDir::Desc {
            "▼"
        } else {
            "▲"
        };
        let header_cells = ["PID", "UID", "Name", "CPU%", "MEM", "Status"]
            .iter()
            .map(|h| {
                let label = match *h {
                    "CPU%" if self.sort_col == SortColumn::Cpu => format!("CPU%{}", dir_sym),
                    "MEM" if self.sort_col == SortColumn::Mem => format!("MEM{}", dir_sym),
                    "PID" if self.sort_col == SortColumn::Pid => format!("PID{}", dir_sym),
                    "UID" if self.sort_col == SortColumn::User => format!("UID{}", dir_sym),
                    "Name" if self.sort_col == SortColumn::Name => format!("Name{}", dir_sym),
                    "Status" if self.sort_col == SortColumn::Status => format!("Status{}", dir_sym),
                    _ => h.to_string(),
                };
                ratatui::widgets::Cell::from(label).style(
                    Style::new()
                        .fg(self.palette.accent)
                        .add_modifier(Modifier::BOLD),
                )
            });
        let header = Row::new(header_cells).height(1);

        let rows: Vec<Row> = self
            .displayed
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let name = if self.view_mode == ProcessViewMode::Tree {
                    let prefix = self
                        .tree_rows
                        .get(i)
                        .map(|r| r.tree_prefix())
                        .unwrap_or_default();
                    let collapse_marker = self
                        .tree_rows
                        .get(i)
                        .filter(|r| r.has_children && !r.is_expanded)
                        .map(|_| "[+] ")
                        .unwrap_or("");
                    format!("{prefix}{collapse_marker}{}", p.name)
                } else {
                    p.name.clone()
                };
                let dash = "—".to_string();
                Row::new(vec![
                    p.pid.to_string(),
                    p.user.clone(),
                    name,
                    if p.is_thread {
                        dash.clone()
                    } else {
                        format!("{:.1}", p.cpu_pct)
                    },
                    if p.is_thread {
                        dash.clone()
                    } else {
                        format!("{:.1}%", p.mem_pct)
                    },
                    p.status.to_string(),
                ])
                .style(Style::new().fg(cpu_color(p.cpu_pct, &self.palette)))
            })
            .collect();

        let widths = [
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Fill(1),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(10),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .row_highlight_style(Style::new().bg(self.palette.border).fg(self.palette.fg));

        frame.render_stateful_widget(table, area, &mut self.table_state);
        Ok(())
    }

    fn draw_fullscreen(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        // Column widths: PID(7) User(10) PR(4) NI(4) VIRT(10) RES(10) SHR(10) S(2) %CPU(6) %MEM(6) TIME(10) Command(Fill)
        let dir_sym = if self.sort_dir == SortDir::Desc {
            "▼"
        } else {
            "▲"
        };
        let accent_bold = Style::new()
            .fg(self.palette.accent)
            .add_modifier(Modifier::BOLD);
        // Pair each header label with its SortColumn so the active column
        // gets the direction indicator automatically.
        let header_cells: Vec<_> = [
            ("PID", SortColumn::Pid),
            ("UID", SortColumn::User),
            ("PR", SortColumn::Priority),
            ("NI", SortColumn::Nice),
            ("VIRT", SortColumn::Virt),
            ("RES", SortColumn::Res),
            ("SHR", SortColumn::Shr),
            ("S", SortColumn::Status),
            ("%CPU", SortColumn::Cpu),
            ("%MEM", SortColumn::Mem),
            ("TIME", SortColumn::Time),
            ("Command", SortColumn::Name),
        ]
        .iter()
        .map(|(h, col)| {
            let label = if *col == self.sort_col {
                format!("{h}{dir_sym}")
            } else {
                h.to_string()
            };
            ratatui::widgets::Cell::from(label).style(accent_bold)
        })
        .collect();
        let header = Row::new(header_cells).height(1);

        let widths = [
            Constraint::Length(7),
            Constraint::Length(10),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(2),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(10),
            Constraint::Fill(1),
        ];

        let rows: Vec<Row> = self
            .displayed
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let status_char = match p.status {
                    crate::stats::snapshots::ProcessStatus::Running => "R",
                    crate::stats::snapshots::ProcessStatus::Sleeping => "S",
                    crate::stats::snapshots::ProcessStatus::Idle => "I",
                    crate::stats::snapshots::ProcessStatus::Stopped => "T",
                    crate::stats::snapshots::ProcessStatus::Zombie => "Z",
                    crate::stats::snapshots::ProcessStatus::Dead => "X",
                    crate::stats::snapshots::ProcessStatus::Unknown => "?",
                };
                let raw_cmd = if p.cmd.is_empty() {
                    p.name.clone()
                } else {
                    p.cmd.join(" ")
                };
                let cmd = if self.view_mode == ProcessViewMode::Tree {
                    let prefix = self
                        .tree_rows
                        .get(i)
                        .map(|r| r.tree_prefix())
                        .unwrap_or_default();
                    let collapse_marker = self
                        .tree_rows
                        .get(i)
                        .filter(|r| r.has_children && !r.is_expanded)
                        .map(|_| "[+] ")
                        .unwrap_or("");
                    format!("{prefix}{collapse_marker}{raw_cmd}")
                } else {
                    raw_cmd
                };
                let dash = "\u{2014}".to_string();
                Row::new(vec![
                    p.pid.to_string(),
                    p.user.clone(),
                    if p.is_thread {
                        dash.clone()
                    } else {
                        p.priority.to_string()
                    },
                    if p.is_thread {
                        dash.clone()
                    } else {
                        p.nice.to_string()
                    },
                    if p.is_thread {
                        dash.clone()
                    } else {
                        fmt_rate_col(p.virt_bytes)
                    },
                    if p.is_thread {
                        dash.clone()
                    } else {
                        fmt_rate_col(p.mem_bytes)
                    },
                    if p.is_thread {
                        dash.clone()
                    } else {
                        fmt_rate_col(p.shr_bytes)
                    },
                    status_char.to_string(),
                    if p.is_thread {
                        dash.clone()
                    } else {
                        format!("{:.1}", p.cpu_pct)
                    },
                    if p.is_thread {
                        dash.clone()
                    } else {
                        format!("{:.1}", p.mem_pct)
                    },
                    fmt_cpu_time(p.cpu_time_secs),
                    cmd,
                ])
                .style(Style::new().fg(cpu_color(p.cpu_pct, &self.palette)))
            })
            .collect();

        let table = Table::new(rows, widths)
            .header(header)
            .row_highlight_style(Style::new().bg(self.palette.border).fg(self.palette.fg));

        frame.render_stateful_widget(table, area, &mut self.table_state);
        Ok(())
    }
}

/// Format total CPU time as `MM:SS`.
fn fmt_cpu_time(secs: f64) -> String {
    let total = secs as u64;
    let m = total / 60;
    let s = total % 60;
    format!("{:02}:{:02}", m, s)
}

/// Format a duration in seconds as a human-readable string like "1h 02m 03s".
fn fmt_duration_long(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h {m:02}m {s:02}s")
    } else {
        format!("{m}m {s:02}s")
    }
}

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

fn fmt_start_time(unix_secs: u64) -> String {
    if unix_secs == 0 {
        return "-".into();
    }
    chrono::Local
        .timestamp_opt(unix_secs as i64, 0)
        .single()
        .map(|ts| ts.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "-".into())
}

fn fmt_opt_str(value: Option<&str>) -> String {
    value.filter(|s| !s.is_empty()).unwrap_or("-").to_string()
}

fn fmt_opt<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map(|v| v.to_string()).unwrap_or_else(|| "-".into())
}

fn fmt_opt_bytes(value: Option<u64>) -> String {
    value.map(fmt_bytes).unwrap_or_else(|| "-".into())
}

fn detail_kv(label: &str, value: impl Into<String>, palette: &ColorPalette) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {label:<15} "), Style::new().fg(palette.dim)),
        Span::styled(value.into(), Style::new().fg(palette.fg)),
    ])
}

fn detail_header_kv(
    label: &str,
    value: impl Into<String>,
    palette: &ColorPalette,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {label:<9} "), Style::new().fg(palette.dim)),
        Span::styled(value.into(), Style::new().fg(palette.fg)),
    ])
}

fn horizontal_rule(width: u16) -> Paragraph<'static> {
    Paragraph::new("─".repeat(width.saturating_sub(1) as usize)).centered()
}

fn kill_process(pid: u32) -> Result<()> {
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .with_context(|| format!("sending SIGTERM to pid {pid}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use crossterm::event::{KeyCode, KeyModifiers};
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};

    use crate::{action::Action, stats::snapshots::ProcSnapshot};

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn key_code(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn renders_process_list() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn slash_key_enters_filter_mode() {
        let mut comp = ProcessComponent::default();
        comp.handle_key_event(key('/')).unwrap();
        assert_eq!(
            comp.state,
            ProcessState::FilterMode {
                input: String::new()
            }
        );
    }

    #[test]
    fn esc_in_filter_mode_returns_to_list() {
        let mut comp = ProcessComponent::default();
        comp.handle_key_event(key('/')).unwrap();
        comp.handle_key_event(key_code(KeyCode::Esc)).unwrap();
        assert_eq!(comp.state, ProcessState::NormalList);
    }

    #[test]
    fn cpu_color_tiers() {
        let palette = ColorPalette::dark();
        assert_eq!(cpu_color(96.0, &palette), palette.critical);
        assert_eq!(cpu_color(95.0, &palette), palette.critical);
        assert_eq!(cpu_color(81.0, &palette), palette.warn);
        assert_eq!(cpu_color(80.0, &palette), palette.warn);
        assert_eq!(cpu_color(79.9, &palette), palette.fg);
        assert_eq!(cpu_color(0.0, &palette), palette.fg);
    }

    #[test]
    fn first_row_selected_on_proc_update() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        assert_eq!(comp.table_state.selected(), Some(0));
    }

    #[test]
    fn page_up_down_clamp_to_list_bounds() {
        // Build a snapshot with 5 processes — fewer than PAGE (10).
        let mut snap = ProcSnapshot::stub();
        let base = snap.processes[0].clone();
        snap.processes = (0..5)
            .map(|i| ProcessEntry {
                pid: i,
                name: format!("proc{i}"),
                ..base.clone()
            })
            .collect();

        let mut comp = ProcessComponent::default();
        comp.set_focused(true);
        comp.update(&Action::ProcUpdate(snap)).unwrap();
        comp.table_state.select(Some(2));

        // PageDown must clamp to last row (index 4, not 12).
        let action = comp.handle_key_event(key_code(KeyCode::PageDown)).unwrap();
        assert!(matches!(action, Some(Action::Render)));
        assert_eq!(
            comp.table_state.selected(),
            Some(4),
            "PageDown must clamp at last row"
        );

        // PageUp must clamp to first row (index 0).
        let action = comp.handle_key_event(key_code(KeyCode::PageUp)).unwrap();
        assert!(matches!(action, Some(Action::Render)));
        assert_eq!(
            comp.table_state.selected(),
            Some(0),
            "PageUp must clamp at first row"
        );
    }

    #[test]
    fn enter_opens_detail_view() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        comp.table_state.select(Some(0));
        comp.handle_key_event(key_code(KeyCode::Enter)).unwrap();
        assert!(matches!(comp.state, ProcessState::DetailView { .. }));
    }

    #[test]
    fn detail_view_renders_two_column_process_fields() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        comp.table_state.select(Some(0));
        comp.handle_key_event(key_code(KeyCode::Enter)).unwrap();

        let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn kill_error_state_dismisses_on_enter_or_esc() {
        // Directly place the component in KillError state (simulates a failed kill)
        let mut comp = ProcessComponent {
            state: ProcessState::KillError {
                message: "kill -TERM pid 99 failed (exit code 1)".to_string(),
            },
            ..Default::default()
        };
        // Enter dismisses and returns to NormalList
        comp.handle_key_event(key_code(KeyCode::Enter)).unwrap();
        assert_eq!(comp.state, ProcessState::NormalList);

        // Same with Esc
        comp.state = ProcessState::KillError {
            message: "some error".to_string(),
        };
        comp.handle_key_event(key_code(KeyCode::Esc)).unwrap();
        assert_eq!(comp.state, ProcessState::NormalList);
    }

    #[test]
    fn kill_error_renders_without_panic() {
        let mut comp = ProcessComponent {
            state: ProcessState::KillError {
                message: "kill -TERM pid 1 failed (exit code 1) — permission denied".to_string(),
            },
            ..Default::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        // Verify the error text appears somewhere in the rendered output
        let rendered = format!("{:?}", terminal.backend());
        assert!(rendered.contains("Kill Failed") || rendered.contains("permission"));
    }

    #[test]
    fn toggle_fullscreen_via_action_when_focused() {
        let mut comp = ProcessComponent::default();
        comp.set_focused(true);
        assert!(!comp.is_fullscreen);

        comp.update(&Action::ToggleFullScreen).unwrap();
        assert!(comp.is_fullscreen);

        comp.update(&Action::ToggleFullScreen).unwrap();
        assert!(!comp.is_fullscreen);
    }

    #[test]
    fn toggle_fullscreen_ignored_when_not_focused() {
        let mut comp = ProcessComponent::default();
        // focused defaults to false
        comp.update(&Action::ToggleFullScreen).unwrap();
        assert!(
            !comp.is_fullscreen,
            "unfocused component must not enter fullscreen"
        );
    }

    #[test]
    fn set_focused_false_clears_fullscreen() {
        let mut comp = ProcessComponent::default();
        comp.set_focused(true);
        comp.is_fullscreen = true;
        comp.set_focused(false);
        assert!(!comp.is_fullscreen);
    }

    #[test]
    fn fullscreen_renders_without_panic() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        comp.set_focused(true);
        comp.update(&Action::ToggleFullScreen).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!(terminal.backend());
    }

    /// Full-width layout slots (Dashboard/Classic Bottom) trigger extended columns
    /// automatically — no explicit fullscreen toggle required.
    #[test]
    fn wide_area_uses_extended_columns_without_fullscreen() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        // 120-col area meets the threshold; is_fullscreen stays false.
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        let rendered = format!("{:?}", terminal.backend());
        // Extended-only columns that do not appear in the normal view.
        assert!(
            rendered.contains("UID") && rendered.contains("VIRT") && rendered.contains("Command"),
            "wide area must render extended columns; got: {rendered}"
        );
        assert!(
            !comp.is_fullscreen,
            "is_fullscreen flag must remain false — extended view triggered by width"
        );
    }

    /// Narrow area (sidebar right-panel width) keeps the compact 5-column view.
    #[test]
    fn narrow_area_uses_normal_columns() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        // 80-col area is well below the threshold.
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        let rendered = format!("{:?}", terminal.backend());
        assert!(
            rendered.contains("CPU%") && rendered.contains("Status") && rendered.contains("UID"),
            "narrow area must render normal columns; got: {rendered}"
        );
        // Extended-only columns must not appear.
        assert!(
            !rendered.contains("VIRT") && !rendered.contains("Command"),
            "narrow area must not render extended columns; got: {rendered}"
        );
    }

    /// Sort cycle in the extended view follows column left-to-right order:
    /// PID → UID → PR → NI → VIRT → RES → SHR → S → %CPU → %MEM → TIME → Command → PID …
    #[test]
    fn extended_view_sort_cycle_follows_column_order() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        comp.set_focused(true);

        // 140-col terminal triggers extended layout.
        let mut terminal = Terminal::new(TestBackend::new(140, 10)).unwrap();
        let render = |comp: &mut ProcessComponent, term: &mut Terminal<TestBackend>| {
            term.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
            format!("{:?}", term.backend())
        };

        // Start at default (Cpu, index 8 in the extended cycle).  Cycling
        // forwards visits every column exactly once before wrapping back.
        // Cycle order: Pid(0) UID(1) PR(2) NI(3) VIRT(4) RES(5) SHR(6)
        //              S(7) %CPU(8) %MEM(9) TIME(10) Command(11)
        let steps: &[(&str, &str)] = &[
            ("%CPU▼", "initial default"),
            ("%MEM▼", "Cpu→Mem"),
            ("TIME▼", "Mem→Time"),
            ("Command▼", "Time→Name"),
            ("PID▼", "Name→Pid"),
            ("UID▼", "Pid→UID"),
            ("PR▼", "UID→Priority"),
            ("NI▼", "Priority→Nice"),
            ("VIRT▼", "Nice→Virt"),
            ("RES▼", "Virt→Res"),
            ("SHR▼", "Res→Shr"),
            ("S▼", "Shr→Status"),
            ("%CPU▼", "Status→Cpu (wrap)"),
        ];

        // Check initial state without pressing 's'.
        let rendered = render(&mut comp, &mut terminal);
        assert!(
            rendered.contains(steps[0].0),
            "step '{}': expected '{}'; got: {rendered}",
            steps[0].1,
            steps[0].0
        );

        // Walk remaining steps, pressing 's' before each check.
        // steps[12] is the wrap-around back to %CPU▼.
        for (expected, label) in &steps[1..] {
            comp.handle_key_event(key_code(KeyCode::Char('s'))).unwrap();
            let rendered = render(&mut comp, &mut terminal);
            assert!(
                rendered.contains(expected),
                "step '{label}': expected '{expected}'; got: {rendered}"
            );
        }
    }

    /// Sort cycle in the normal (narrow) view follows column left-to-right order:
    /// PID → User → Name → CPU% → MEM → Status → PID …
    #[test]
    fn normal_view_sort_cycle_follows_column_order() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        comp.set_focused(true);

        // 80-col terminal keeps the narrow layout.
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
        let render = |comp: &mut ProcessComponent, term: &mut Terminal<TestBackend>| {
            term.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
            format!("{:?}", term.backend())
        };

        // Default: CPU%▼
        let rendered = render(&mut comp, &mut terminal);
        assert!(
            rendered.contains("CPU%▼"),
            "expected CPU%▼; got: {rendered}"
        );

        // Cpu → Mem
        comp.handle_key_event(key_code(KeyCode::Char('s'))).unwrap();
        let rendered = render(&mut comp, &mut terminal);
        assert!(
            rendered.contains("MEM▼"),
            "expected MEM▼ after Cpu→Mem; got: {rendered}"
        );

        // Mem → Status
        comp.handle_key_event(key_code(KeyCode::Char('s'))).unwrap();
        let rendered = render(&mut comp, &mut terminal);
        assert!(
            rendered.contains("Status▼"),
            "expected Status▼ after Mem→Status; got: {rendered}"
        );

        // Status → Pid
        comp.handle_key_event(key_code(KeyCode::Char('s'))).unwrap();
        let rendered = render(&mut comp, &mut terminal);
        assert!(
            rendered.contains("PID▼"),
            "expected PID▼ after Status→Pid; got: {rendered}"
        );

        // Pid → User
        comp.handle_key_event(key_code(KeyCode::Char('s'))).unwrap();
        let rendered = render(&mut comp, &mut terminal);
        assert!(
            rendered.contains("UID▼"),
            "expected UID▼ after Pid→User; got: {rendered}"
        );

        // User → Name
        comp.handle_key_event(key_code(KeyCode::Char('s'))).unwrap();
        let rendered = render(&mut comp, &mut terminal);
        assert!(
            rendered.contains("Name▼"),
            "expected Name▼ after User→Name; got: {rendered}"
        );

        // Name → Cpu (wraps back)
        comp.handle_key_event(key_code(KeyCode::Char('s'))).unwrap();
        let rendered = render(&mut comp, &mut terminal);
        assert!(
            rendered.contains("CPU%▼"),
            "expected CPU%▼ after wrap-around; got: {rendered}"
        );
    }

    #[test]
    fn fmt_cpu_time_formats_correctly() {
        assert_eq!(fmt_cpu_time(0.0), "00:00");
        assert_eq!(fmt_cpu_time(59.9), "00:59");
        assert_eq!(fmt_cpu_time(60.0), "01:00");
        assert_eq!(fmt_cpu_time(123.4), "02:03");
        assert_eq!(fmt_cpu_time(3661.0), "61:01");
    }

    #[test]
    fn fmt_duration_long_formats_correctly() {
        assert_eq!(fmt_duration_long(0), "0m 00s");
        assert_eq!(fmt_duration_long(65), "1m 05s");
        assert_eq!(fmt_duration_long(3661), "1h 01m 01s");
    }

    #[test]
    fn fmt_start_time_formats_local_timestamp() {
        let ts = chrono::Local
            .with_ymd_and_hms(2026, 4, 4, 12, 34, 56)
            .single()
            .expect("local timestamp must exist");
        assert_eq!(fmt_start_time(ts.timestamp() as u64), "2026-04-04 12:34:56");
        assert_eq!(fmt_start_time(0), "-");
    }

    #[test]
    fn detail_view_consumes_unhandled_keys() {
        // Keys not explicitly handled in detail mode must return Some so the
        // global app handler never sees them and cannot shift focus or close
        // the modal.
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        // Enter detail view for the first process.
        comp.table_state.select(Some(0));
        comp.handle_key_event(key_code(KeyCode::Enter)).unwrap();
        assert!(
            matches!(comp.state, ProcessState::DetailView { .. }),
            "Enter must open detail view"
        );

        for code in [
            KeyCode::Tab,
            KeyCode::BackTab,
            KeyCode::Char('n'),
            KeyCode::Char('i'),
            KeyCode::Char('f'),
            KeyCode::Char('d'),
        ] {
            let action = comp.handle_key_event(key_code(code)).unwrap();
            assert!(
                action.is_some(),
                "{code:?} must be consumed in detail view, got None"
            );
            assert!(
                matches!(comp.state, ProcessState::DetailView { .. }),
                "{code:?} must not exit detail view"
            );
        }
    }

    fn kill_confirm_state(pid: u32, name: &str, ok_focused: bool) -> ProcessState {
        ProcessState::KillConfirm {
            pid,
            name: name.to_string(),
            ok_focused,
        }
    }

    #[test]
    fn kill_confirm_default_focuses_cancel() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        comp.table_state.select(Some(0));
        comp.handle_key_event(key('k')).unwrap();
        assert!(
            matches!(
                comp.state,
                ProcessState::KillConfirm {
                    ok_focused: false,
                    ..
                }
            ),
            "Cancel must be the default focused button"
        );
    }

    #[test]
    fn kill_confirm_tab_cycles_focus() {
        let mut comp = ProcessComponent {
            state: kill_confirm_state(42, "test", false),
            ..Default::default()
        };
        // Tab: Cancel → OK
        comp.handle_key_event(key_code(KeyCode::Tab)).unwrap();
        assert!(
            matches!(
                comp.state,
                ProcessState::KillConfirm {
                    ok_focused: true,
                    ..
                }
            ),
            "Tab must move focus to OK"
        );
        // Tab again: OK → Cancel
        comp.handle_key_event(key_code(KeyCode::Tab)).unwrap();
        assert!(
            matches!(
                comp.state,
                ProcessState::KillConfirm {
                    ok_focused: false,
                    ..
                }
            ),
            "Second Tab must move focus back to Cancel"
        );
        // BackTab also cycles
        comp.handle_key_event(key_code(KeyCode::BackTab)).unwrap();
        assert!(matches!(
            comp.state,
            ProcessState::KillConfirm {
                ok_focused: true,
                ..
            }
        ));
    }

    #[test]
    fn kill_confirm_enter_on_cancel_returns_to_list() {
        let mut comp = ProcessComponent {
            state: kill_confirm_state(42, "test", false), // Cancel focused
            ..Default::default()
        };
        comp.handle_key_event(key_code(KeyCode::Enter)).unwrap();
        assert_eq!(
            comp.state,
            ProcessState::NormalList,
            "Enter on Cancel must cancel"
        );
    }

    #[test]
    fn kill_confirm_esc_returns_to_list() {
        let mut comp = ProcessComponent {
            state: kill_confirm_state(42, "test", true), // OK focused
            ..Default::default()
        };
        comp.handle_key_event(key_code(KeyCode::Esc)).unwrap();
        assert_eq!(
            comp.state,
            ProcessState::NormalList,
            "Esc must always cancel"
        );
    }

    #[test]
    fn kill_confirm_swallows_unhandled_keys() {
        let mut comp = ProcessComponent {
            state: kill_confirm_state(42, "test", false),
            ..Default::default()
        };
        // Keys that used to work (y/n) must now be swallowed without side effects.
        let action = comp.handle_key_event(key('y')).unwrap();
        assert!(action.is_some(), "'y' must be swallowed in kill confirm");
        assert!(
            matches!(comp.state, ProcessState::KillConfirm { .. }),
            "'y' must not confirm kill"
        );
        let action = comp.handle_key_event(key('n')).unwrap();
        assert!(action.is_some(), "'n' must be swallowed in kill confirm");
        assert!(
            matches!(comp.state, ProcessState::KillConfirm { .. }),
            "'n' must not cancel kill"
        );
    }

    #[test]
    fn kill_confirm_renders_dialog_with_buttons() {
        let mut comp = ProcessComponent {
            state: kill_confirm_state(1234, "my-process", false),
            ..Default::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        let rendered = format!("{:?}", terminal.backend());
        assert!(
            rendered.contains("Kill Process"),
            "dialog must show 'Kill Process' title"
        );
        assert!(
            rendered.contains("my-process"),
            "dialog must show process name"
        );
        assert!(rendered.contains("OK"), "dialog must have OK button");
        assert!(
            rendered.contains("Cancel"),
            "dialog must have Cancel button"
        );
        assert_snapshot!("kill_confirm_dialog", terminal.backend());
    }

    #[test]
    fn compact_state_restored_after_fullscreen_exit() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        comp.set_focused(true);
        // Record initial state
        let initial_sort = comp.sort_col;
        let initial_dir = comp.sort_dir;
        // Enter fullscreen
        comp.update(&Action::ToggleFullScreen).unwrap();
        assert!(comp.is_fullscreen);
        // Change sort column
        comp.handle_key_event(key_code(KeyCode::Char('s'))).unwrap();
        assert_ne!(
            comp.sort_col, initial_sort,
            "sort must change in fullscreen"
        );
        // Exit fullscreen
        comp.update(&Action::ToggleFullScreen).unwrap();
        assert!(!comp.is_fullscreen);
        assert_eq!(comp.sort_col, initial_sort, "sort_col must be restored");
        assert_eq!(comp.sort_dir, initial_dir, "sort_dir must be restored");
    }

    #[test]
    fn kill_confirm_state_not_saved_in_compact_snapshot() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        comp.set_focused(true);
        // Put component in KillConfirm state
        comp.state = ProcessState::KillConfirm {
            pid: 12345,
            name: "test".to_string(),
            ok_focused: false,
        };
        // Enter fullscreen — KillConfirm should be coerced to NormalList in snapshot
        comp.update(&Action::ToggleFullScreen).unwrap();
        // Exit fullscreen
        comp.update(&Action::ToggleFullScreen).unwrap();
        assert_eq!(
            comp.state,
            ProcessState::NormalList,
            "KillConfirm must be coerced to NormalList in snapshot"
        );
    }

    /// Compact background pass renders the frozen pre-fullscreen sort order.
    ///
    /// After entering fullscreen and pressing 's' to change the sort column, a
    /// `draw()` call WITHOUT `begin_overlay_render()` (i.e. the compact background
    /// pass) must show the ORIGINAL sort indicator, not the changed one.
    #[test]
    fn compact_background_shows_frozen_sort_during_fullscreen() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        comp.set_focused(true);

        // Render compact baseline — must show default sort (CPU%▼ on 80-col terminal).
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        let baseline = format!("{:?}", terminal.backend());
        assert!(
            baseline.contains("CPU%▼"),
            "baseline must show CPU%▼; got: {baseline}"
        );

        // Enter fullscreen and change the sort column to MEM.
        comp.update(&Action::ToggleFullScreen).unwrap();
        comp.handle_key_event(key_code(KeyCode::Char('s'))).unwrap();
        assert_eq!(
            comp.sort_col,
            sort::SortColumn::Mem,
            "sort must advance to Mem after 's'"
        );

        // Compact background pass (no begin_overlay_render): must show original sort.
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        let compact_bg = format!("{:?}", terminal.backend());
        assert!(
            compact_bg.contains("CPU%▼"),
            "compact background pass must still show CPU%▼ (frozen); got: {compact_bg}"
        );
        assert!(
            !compact_bg.contains("MEM▼"),
            "compact background pass must NOT show MEM▼ (live state); got: {compact_bg}"
        );

        // Overlay pass (begin_overlay_render): must show new sort.
        comp.begin_overlay_render();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        let overlay = format!("{:?}", terminal.backend());
        assert!(
            overlay.contains("MEM▼"),
            "overlay pass must show MEM▼ (live state); got: {overlay}"
        );
    }

    /// Compact background pass renders the frozen filter string.
    ///
    /// After entering fullscreen and typing a filter, the compact background pass
    /// must show no filter in the title (filter was empty when fullscreen opened).
    #[test]
    fn compact_background_shows_frozen_filter_during_fullscreen() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        comp.set_focused(true);
        comp.update(&Action::ToggleFullScreen).unwrap();

        // In fullscreen: enter filter mode and type a filter string.
        comp.handle_key_event(key('/')).unwrap();
        comp.handle_key_event(key('b')).unwrap();
        assert!(
            matches!(comp.state, ProcessState::FilterMode { ref input } if input == "b"),
            "must be in FilterMode with input 'b'"
        );

        // Compact background pass: title must NOT show the filter.
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        let compact_bg = format!("{:?}", terminal.backend());
        assert!(
            !compact_bg.contains("filter: b"),
            "compact background must not show live filter; got: {compact_bg}"
        );
        // keyed_title renders "[P]rocesses"; check the non-key portion is present.
        assert!(
            compact_bg.contains("rocesses"),
            "compact background must show plain 'rocesses' in title; got: {compact_bg}"
        );
    }

    // ── Tree-view tests ──────────────────────────────────────────────

    #[test]
    fn t_key_toggles_tree_mode() {
        let mut comp = ProcessComponent::default();
        assert_eq!(comp.view_mode, ProcessViewMode::Flat);
        comp.handle_key_event(key('t')).unwrap();
        assert_eq!(comp.view_mode, ProcessViewMode::Tree);
        comp.handle_key_event(key('t')).unwrap();
        assert_eq!(comp.view_mode, ProcessViewMode::Flat);
    }

    #[test]
    fn tree_mode_title_shows_tag() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        comp.handle_key_event(key('t')).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        let rendered = format!("{:?}", terminal.backend());
        assert!(
            rendered.contains("[tree]"),
            "title must contain [tree]; got: {rendered}"
        );
    }

    #[test]
    fn tree_mode_renders_hierarchy() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        comp.handle_key_event(key('t')).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("tree_view_normal", terminal.backend());
    }

    #[test]
    fn tree_mode_fullscreen_renders_hierarchy() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        comp.set_focused(true);
        comp.handle_key_event(key('t')).unwrap();
        comp.update(&Action::ToggleFullScreen).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(140, 20)).unwrap();
        comp.begin_overlay_render();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("tree_view_fullscreen", terminal.backend());
    }

    #[test]
    fn space_collapses_tree_node() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        comp.handle_key_event(key('t')).unwrap();

        // Select root (systemd, pid 1) which is at index 0 in PID-asc sorted tree
        // Default sort is CPU desc, so let's switch to PID asc first.
        comp.sort_col = super::sort::SortColumn::Pid;
        comp.sort_dir = super::sort::SortDir::Asc;
        comp.refresh_display();
        comp.table_state.select(Some(0));

        let initial_len = comp.displayed.len();
        assert!(
            initial_len > 1,
            "tree must have more than 1 row; got: {initial_len}"
        );

        // Collapse systemd.
        comp.handle_key_event(key_code(KeyCode::Char(' '))).unwrap();
        assert!(
            comp.displayed.len() < initial_len,
            "collapsing root must hide children; before={initial_len} after={}",
            comp.displayed.len()
        );

        // Expand again.
        comp.table_state.select(Some(0));
        comp.handle_key_event(key_code(KeyCode::Char(' '))).unwrap();
        assert_eq!(
            comp.displayed.len(),
            initial_len,
            "expanding root must restore children"
        );
    }

    #[test]
    fn space_in_flat_mode_is_ignored() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        assert_eq!(comp.view_mode, ProcessViewMode::Flat);
        // Space should return Ok(None) and not panic.
        let action = comp.handle_key_event(key_code(KeyCode::Char(' '))).unwrap();
        assert!(
            action.is_none(),
            "space in flat mode must be unhandled (None)"
        );
    }

    #[test]
    fn tree_mode_preserved_across_fullscreen_toggle() {
        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        comp.set_focused(true);
        // Switch to tree mode
        comp.handle_key_event(key('t')).unwrap();
        assert_eq!(comp.view_mode, ProcessViewMode::Tree);
        // Enter fullscreen — compact snapshot stores tree mode.
        comp.update(&Action::ToggleFullScreen).unwrap();
        // Switch back to flat in fullscreen.
        comp.handle_key_event(key('t')).unwrap();
        assert_eq!(comp.view_mode, ProcessViewMode::Flat);
        // Exit fullscreen — should restore tree mode.
        comp.update(&Action::ToggleFullScreen).unwrap();
        assert_eq!(
            comp.view_mode,
            ProcessViewMode::Tree,
            "tree mode must be restored from compact snapshot"
        );
    }

    #[test]
    fn config_show_tree_sets_initial_mode() {
        let config = ProcessConfig {
            show_tree: true,
            ..Default::default()
        };
        let comp = ProcessComponent::new(ColorPalette::dark(), 'p', &config);
        assert_eq!(comp.view_mode, ProcessViewMode::Tree);
    }

    /// Threads must not appear in the flat list view — only processes.
    #[test]
    fn flat_view_excludes_threads() {
        let mut snap = ProcSnapshot::stub();
        let base = snap.processes[0].clone();
        // Add a thread entry whose parent is one of the stub processes.
        snap.processes.push(ProcessEntry {
            pid: 9999,
            name: "worker-thread".into(),
            parent_pid: Some(base.pid),
            is_thread: true,
            ..base
        });

        let mut comp = ProcessComponent::default();
        comp.update(&Action::ProcUpdate(snap)).unwrap();

        assert!(
            comp.displayed.iter().all(|p| !p.is_thread),
            "flat view must not contain any thread entries; got: {:?}",
            comp.displayed
                .iter()
                .filter(|p| p.is_thread)
                .map(|p| &p.name)
                .collect::<Vec<_>>()
        );
    }

    /// Pressing 'k' on a thread row in tree view must target the owning
    /// process, not the thread TID.
    #[test]
    fn kill_on_thread_targets_parent_process() {
        let mut snap = ProcSnapshot::stub();
        let base = snap.processes[0].clone();
        let parent_pid = base.pid;
        let parent_name = base.name.clone();
        snap.processes.push(ProcessEntry {
            pid: 8888,
            name: "worker-thread".into(),
            parent_pid: Some(parent_pid),
            is_thread: true,
            ..base
        });

        let config = ProcessConfig {
            show_tree: true,
            ..Default::default()
        };
        let mut comp = ProcessComponent::new(ColorPalette::dark(), 'p', &config);
        comp.update(&Action::ProcUpdate(snap)).unwrap();

        // Navigate to the thread row (it will be a child of the parent process).
        let thread_idx = comp
            .displayed
            .iter()
            .position(|p| p.is_thread)
            .expect("thread must appear as a child in tree view");
        comp.table_state.select(Some(thread_idx));

        comp.handle_key_event(key('k')).unwrap();

        assert_eq!(
            comp.state,
            ProcessState::KillConfirm {
                pid: parent_pid,
                name: parent_name,
                ok_focused: false,
            },
            "kill confirm must reference the parent process, not the thread TID"
        );
    }
}
