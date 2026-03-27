pub mod filter;
pub mod sort;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Row, Table, TableState},
};

use crate::{
    action::Action, components::Component, config::ProcessConfig, stats::snapshots::ProcessEntry,
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
    FilterMode { input: String },
    DetailView { pid: u32 },
    KillConfirm { pid: u32, name: String },
}

pub struct ProcessComponent {
    palette: ColorPalette,
    raw: Vec<ProcessEntry>,
    displayed: Vec<ProcessEntry>,
    table_state: TableState,
    filter: ProcessFilter,
    sort_col: SortColumn,
    sort_dir: SortDir,
    pub state: ProcessState,
    focused: bool,
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
        Self {
            palette: ColorPalette::dark(),
            raw: Vec::new(),
            displayed: Vec::new(),
            table_state: TableState::default(),
            filter: ProcessFilter::None,
            sort_col: SortColumn::default(),
            sort_dir: SortDir::default(),
            state: ProcessState::NormalList,
            focused: false,
        }
    }
}

impl ProcessComponent {
    pub fn new(palette: ColorPalette, config: &ProcessConfig) -> Self {
        let sort_col = config.default_sort.parse().unwrap_or_default();
        let sort_dir = if config.default_sort_dir == "asc" {
            SortDir::Asc
        } else {
            SortDir::Desc
        };
        Self {
            palette,
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
        // Clamp selection to the new list length
        let max = list.len().saturating_sub(1);
        if let Some(sel) = self.table_state.selected()
            && sel > max
        {
            self.table_state.select(Some(max));
        }
        self.displayed = list;
    }
}

impl Component for ProcessComponent {
    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
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
                        kill_process(pid)?;
                        self.state = ProcessState::NormalList;
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
        if let Action::ProcUpdate(snap) = action {
            self.raw = snap.processes;
            self.refresh_display();
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let title = match &self.state {
            ProcessState::FilterMode { input } => format!(" Processes [filter: {}▌] ", input),
            _ => " Processes ".to_string(),
        };
        let border_color = if self.focused {
            self.palette.accent
        } else {
            self.palette.border
        };
        let title_style = if self.focused {
            Style::new()
                .fg(self.palette.fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(self.palette.fg)
        };
        let block = Block::default()
            .title(Span::styled(title, title_style))
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border_color));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Kill confirm overlay — shown in place of the table
        if let ProcessState::KillConfirm { pid, name } = &self.state {
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
            if let Some(p) = self.displayed.iter().find(|p| p.pid == pid) {
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

        // Normal list / table
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

        frame.render_stateful_widget(table, inner, &mut self.table_state);
        Ok(())
    }
}

fn kill_process(pid: u32) -> Result<()> {
    use anyhow::Context;
    std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .context("sending SIGTERM")?;
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
    fn enter_opens_detail_view() {
        let mut comp = ProcessComponent::default();
        comp.update(Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();
        comp.table_state.select(Some(0));
        comp.handle_key_event(key_code(KeyCode::Enter)).unwrap();
        assert!(matches!(comp.state, ProcessState::DetailView { .. }));
    }
}
