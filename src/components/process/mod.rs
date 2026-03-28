pub mod filter;
pub mod sort;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
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
    },
    /// Kill command was attempted but failed — show an error dialog to the user.
    KillError {
        message: String,
    },
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
        Self {
            palette,
            focus_key,
            sort_col,
            sort_dir,
            ..Default::default()
        }
    }

    fn refresh_display(&mut self) {
        let mut list: Vec<ProcessEntry> = self
            .raw
            .iter()
            .filter(|p| self.filter.matches(p))
            .cloned()
            .collect();
        sort_processes(&mut list, self.sort_col, self.sort_dir);
        if list.is_empty() {
            self.table_state.select(None);
        } else {
            let max = list.len() - 1;
            let sel = self.table_state.selected().unwrap_or(0).min(max);
            self.table_state.select(Some(sel));
        }
        self.displayed = list;
    }
}

impl Component for ProcessComponent {
    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        if !focused {
            self.is_fullscreen = false;
        }
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match &self.state.clone() {
            ProcessState::FilterMode { input } => {
                match key.code {
                    KeyCode::Esc => {
                        self.filter = ProcessFilter::None;
                        self.state = ProcessState::NormalList;
                        self.refresh_display();
                    }
                    KeyCode::Enter => {
                        self.state = ProcessState::NormalList;
                    }
                    KeyCode::Backspace => {
                        let mut s = input.clone();
                        s.pop();
                        self.filter = ProcessFilter::parse(&s);
                        self.state = ProcessState::FilterMode { input: s };
                        self.refresh_display();
                    }
                    KeyCode::Char(c) => {
                        let mut s = input.clone();
                        s.push(c);
                        self.filter = ProcessFilter::parse(&s);
                        self.state = ProcessState::FilterMode { input: s };
                        self.refresh_display();
                    }
                    _ => {}
                }
                Ok(Some(Action::Render))
            }
            ProcessState::DetailView { .. } => {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {
                        self.state = ProcessState::NormalList;
                        return Ok(Some(Action::Render));
                    }
                    _ => {}
                }
                Ok(None)
            }
            ProcessState::KillConfirm { pid, name } => {
                let pid = *pid;
                let _name = name.clone();
                match key.code {
                    KeyCode::Char('y') | KeyCode::Enter => {
                        if let Err(e) = kill_process(pid) {
                            self.state = ProcessState::KillError {
                                message: e.to_string(),
                            };
                        } else {
                            self.state = ProcessState::NormalList;
                        }
                        return Ok(Some(Action::Render));
                    }
                    KeyCode::Char('n') | KeyCode::Esc => {
                        self.state = ProcessState::NormalList;
                        return Ok(Some(Action::Render));
                    }
                    _ => {}
                }
                Ok(None)
            }
            ProcessState::KillError { .. } => {
                match key.code {
                    KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' ') => {
                        self.state = ProcessState::NormalList;
                        return Ok(Some(Action::Render));
                    }
                    _ => {}
                }
                Ok(None)
            }
            ProcessState::NormalList => {
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
                            self.state = ProcessState::KillConfirm {
                                pid: p.pid,
                                name: p.name.clone(),
                            };
                            return Ok(Some(Action::Render));
                        }
                    }
                    KeyCode::Char('s') => {
                        // Cycle through sort columns
                        use strum::IntoEnumIterator;
                        let cols: Vec<SortColumn> = SortColumn::iter().collect();
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
                    _ => {}
                }
                Ok(None)
            }
        }
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::ProcUpdate(snap) => {
                self.raw = snap.processes;
                self.refresh_display();
            }
            Action::ToggleFullScreen if self.focused => {
                self.is_fullscreen = !self.is_fullscreen;
            }
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let title_rest = match &self.state {
            ProcessState::FilterMode { input } => format!("rocesses [filter: {}▌]", input),
            _ => "rocesses".to_string(),
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
        if let ProcessState::KillError { message } = &self.state.clone() {
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

        // Kill confirm overlay — shown in place of the table
        if let ProcessState::KillConfirm { pid, name } = &self.state.clone() {
            let msg = format!(" Kill {} (pid {})? [y/n] ", name, pid);
            let line = Line::from(Span::styled(
                msg,
                Style::new().fg(self.palette.critical).bold(),
            ));
            frame.render_widget(line, inner);
            return Ok(());
        }

        // Detail view overlay
        if let ProcessState::DetailView { pid } = &self.state {
            let pid = *pid;
            if let Some(p) = self.displayed.iter().find(|p| p.pid == pid).cloned() {
                let lines: Vec<Line> = vec![
                    Line::from(format!(" PID:     {}", p.pid)),
                    Line::from(format!(" Name:    {}", p.name)),
                    Line::from(format!(" Cmd:     {}", p.cmd.join(" "))),
                    Line::from(format!(" User:    {}", p.user)),
                    Line::from(format!(" Status:  {}", p.status)),
                    Line::from(format!(" CPU:     {:.1}%", p.cpu_pct)),
                    Line::from(format!(
                        " MEM:     {:.1}% ({} bytes)",
                        p.mem_pct, p.mem_bytes
                    )),
                    Line::from(format!(" Virt:    {} bytes", p.virt_bytes)),
                    Line::from(format!(" Nice:    {}", p.nice)),
                    Line::from(format!(" Threads: {}", p.threads)),
                    Line::from(format!(" I/O R:   {} bytes", p.read_bytes)),
                    Line::from(format!(" I/O W:   {} bytes", p.write_bytes)),
                    Line::from(" "),
                    Line::from(Span::styled(
                        " [Esc/q] back",
                        Style::new().fg(self.palette.dim),
                    )),
                ];
                let para = ratatui::widgets::Paragraph::new(lines);
                frame.render_widget(para, inner);
                return Ok(());
            }
        }

        if self.is_fullscreen {
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
        let header_cells = ["PID", "Name", "CPU%", "MEM", "Status"].iter().map(|h| {
            let label = match *h {
                "CPU%" if self.sort_col == SortColumn::Cpu => format!("CPU%{}", dir_sym),
                "MEM" if self.sort_col == SortColumn::Mem => format!("MEM{}", dir_sym),
                "PID" if self.sort_col == SortColumn::Pid => format!("PID{}", dir_sym),
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
            .map(|p| {
                Row::new(vec![
                    p.pid.to_string(),
                    p.name.clone(),
                    format!("{:.1}", p.cpu_pct),
                    format!("{:.1}%", p.mem_pct),
                    p.status.to_string(),
                ])
                .style(Style::new().fg(cpu_color(p.cpu_pct, &self.palette)))
            })
            .collect();

        let widths = [
            Constraint::Length(7),
            Constraint::Fill(1),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(10),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .row_highlight_style(
                Style::new()
                    .fg(self.palette.highlight)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(table, area, &mut self.table_state);
        Ok(())
    }

    fn draw_fullscreen(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        // Column widths: PID(7) User(10) PR(4) NI(4) VIRT(10) RES(10) SHR(10) S(2) %CPU(6) %MEM(6) TIME(10) Command(Fill)
        let accent_bold = Style::new()
            .fg(self.palette.accent)
            .add_modifier(Modifier::BOLD);
        let header_cells = [
            ("PID", Constraint::Length(7)),
            ("User", Constraint::Length(10)),
            ("PR", Constraint::Length(4)),
            ("NI", Constraint::Length(4)),
            ("VIRT", Constraint::Length(10)),
            ("RES", Constraint::Length(10)),
            ("SHR", Constraint::Length(10)),
            ("S", Constraint::Length(2)),
            ("%CPU", Constraint::Length(6)),
            ("%MEM", Constraint::Length(6)),
            ("TIME", Constraint::Length(10)),
            ("Command", Constraint::Fill(1)),
        ]
        .iter()
        .map(|(h, _)| ratatui::widgets::Cell::from(*h).style(accent_bold))
        .collect::<Vec<_>>();
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
            .map(|p| {
                let status_char = match p.status {
                    crate::stats::snapshots::ProcessStatus::Running => "R",
                    crate::stats::snapshots::ProcessStatus::Sleeping => "S",
                    crate::stats::snapshots::ProcessStatus::Idle => "I",
                    crate::stats::snapshots::ProcessStatus::Stopped => "T",
                    crate::stats::snapshots::ProcessStatus::Zombie => "Z",
                    crate::stats::snapshots::ProcessStatus::Dead => "X",
                    crate::stats::snapshots::ProcessStatus::Unknown => "?",
                };
                let cmd = if p.cmd.is_empty() {
                    p.name.clone()
                } else {
                    p.cmd.join(" ")
                };
                Row::new(vec![
                    p.pid.to_string(),
                    p.user.clone(),
                    p.priority.to_string(),
                    p.nice.to_string(),
                    fmt_rate_col(p.virt_bytes),
                    fmt_rate_col(p.mem_bytes),
                    fmt_rate_col(p.shr_bytes),
                    status_char.to_string(),
                    format!("{:.1}", p.cpu_pct),
                    format!("{:.1}", p.mem_pct),
                    fmt_cpu_time(p.cpu_time_secs),
                    cmd,
                ])
                .style(Style::new().fg(cpu_color(p.cpu_pct, &self.palette)))
            })
            .collect();

        let table = Table::new(rows, widths)
            .header(header)
            .row_highlight_style(
                Style::new()
                    .fg(self.palette.highlight)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

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

fn kill_process(pid: u32) -> Result<()> {
    use anyhow::{Context, bail};
    let status = std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .context("sending SIGTERM")?;
    if !status.success() {
        let code = status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        tracing::warn!(pid, exit_code = %code, "kill -TERM returned non-zero exit status");
        bail!(
            "kill -TERM pid {pid} failed (exit code {code}) — \
             the process may not exist or you may lack permission to signal it"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
        comp.update(Action::ProcUpdate(ProcSnapshot::stub()))
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
        comp.update(Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        assert_eq!(comp.table_state.selected(), Some(0));
    }

    #[test]
    fn enter_opens_detail_view() {
        let mut comp = ProcessComponent::default();
        comp.update(Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        comp.table_state.select(Some(0));
        comp.handle_key_event(key_code(KeyCode::Enter)).unwrap();
        assert!(matches!(comp.state, ProcessState::DetailView { .. }));
    }

    #[test]
    fn kill_error_state_dismisses_on_enter_or_esc() {
        let mut comp = ProcessComponent::default();
        // Directly place the component in KillError state (simulates a failed kill)
        comp.state = ProcessState::KillError {
            message: "kill -TERM pid 99 failed (exit code 1)".to_string(),
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
        let mut comp = ProcessComponent::default();
        comp.state = ProcessState::KillError {
            message: "kill -TERM pid 1 failed (exit code 1) — permission denied".to_string(),
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

        comp.update(Action::ToggleFullScreen).unwrap();
        assert!(comp.is_fullscreen);

        comp.update(Action::ToggleFullScreen).unwrap();
        assert!(!comp.is_fullscreen);
    }

    #[test]
    fn toggle_fullscreen_ignored_when_not_focused() {
        let mut comp = ProcessComponent::default();
        // focused defaults to false
        comp.update(Action::ToggleFullScreen).unwrap();
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
        comp.update(Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        comp.set_focused(true);
        comp.update(Action::ToggleFullScreen).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn fmt_cpu_time_formats_correctly() {
        assert_eq!(fmt_cpu_time(0.0), "00:00");
        assert_eq!(fmt_cpu_time(59.9), "00:59");
        assert_eq!(fmt_cpu_time(60.0), "01:00");
        assert_eq!(fmt_cpu_time(123.4), "02:03");
        assert_eq!(fmt_cpu_time(3661.0), "61:01");
    }
}
