// SPDX-License-Identifier: GPL-3.0-only

//! Status bar — summary of system, CPU, tasks, and memory.
//!
//! Positioned at the top (default), bottom, or hidden via config. Consumes
//! [`SysUpdate`](crate::action::Action::SysUpdate),
//! [`CpuUpdate`](crate::action::Action::CpuUpdate),
//! [`MemUpdate`](crate::action::Action::MemUpdate), and
//! [`ProcUpdate`](crate::action::Action::ProcUpdate) actions.
//!
//! Layout (`top`-style):
//! - Row 0: uptime, load averages, right-aligned timestamp
//! - Row 1: CPU mode breakdown (us/sy/ni/id/wa/hi/si/st), Linux-only
//! - Row 2: task counts by process status
//! - Row 3: RAM gauge + free / buf/cache / avail labels
//! - Row 4: SWAP gauge + free label (only when swap is configured)

use anyhow::Result;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph},
};

use crate::{
    action::Action,
    components::Component,
    stats::snapshots::{CpuSnapshot, MemSnapshot, ProcSnapshot, ProcessStatus, SysSnapshot},
    theme::ColorPalette,
};

// Width reserved for the right-aligned label on each mem gauge row. Sized to
// fit the RAM row's "used/total  free  buffer/cache  available" labels.
const MEM_LABEL_WIDTH: u16 = 64;

#[derive(Debug)]
pub struct StatusBarComponent {
    palette: ColorPalette,
    sys: Option<SysSnapshot>,
    cpu: Option<CpuSnapshot>,
    mem: Option<MemSnapshot>,
    proc: Option<ProcSnapshot>,
}

impl StatusBarComponent {
    pub fn new(palette: ColorPalette) -> Self {
        Self {
            palette,
            sys: None,
            cpu: None,
            mem: None,
            proc: None,
        }
    }

    /// Whether the host has swap configured; controls whether the SWAP row is drawn.
    fn has_swap(&self) -> bool {
        self.mem.as_ref().is_some_and(|m| m.swap_total > 0)
    }
}

impl Component for StatusBarComponent {
    fn preferred_height(&self) -> Option<u16> {
        // 2 border rows + 4 content rows (sys/cpu/tasks/ram), +1 when swap present.
        Some(if self.has_swap() { 7 } else { 6 })
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        match action {
            Action::SysUpdate(snap) => self.sys = Some(snap.clone()),
            Action::CpuUpdate(snap) => self.cpu = Some(snap.clone()),
            Action::MemUpdate(snap) => self.mem = Some(snap.clone()),
            Action::ProcUpdate(snap) => self.proc = Some(snap.clone()),
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        // Hostname is the block title so it doesn't consume a content row.
        let hostname = self
            .sys
            .as_ref()
            .map(|s| format!(" {} ", s.hostname))
            .unwrap_or_default();
        let block = Block::default()
            .title(Span::styled(
                hostname,
                Style::new()
                    .fg(self.palette.accent)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::new().fg(self.palette.border));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Content rows: sys, cpu, tasks, ram, [swap?].
        let show_swap = self.has_swap();
        let constraints: Vec<Constraint> = if show_swap {
            vec![Constraint::Length(1); 5]
        } else {
            vec![Constraint::Length(1); 4]
        };
        let rows = Layout::vertical(constraints).split(inner);

        self.draw_sys_row(frame, rows[0]);
        self.draw_cpu_row(frame, rows[1]);
        self.draw_tasks_row(frame, rows[2]);
        self.draw_ram_row(frame, rows[3]);
        if show_swap {
            self.draw_swap_row(frame, rows[4]);
        }

        Ok(())
    }
}

impl StatusBarComponent {
    fn draw_sys_row(&self, frame: &mut Frame, area: Rect) {
        let Some(sys) = &self.sys else { return };
        let uptime = format_uptime(sys.uptime);
        let load = format!(
            "{:.2} {:.2} {:.2}",
            sys.load_avg[0], sys.load_avg[1], sys.load_avg[2]
        );
        let time = sys.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();

        // Left span (uptime + load) fills, right span (timestamp) is fixed width.
        let halves =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(time.len() as u16)])
                .split(area);

        let left = Line::from(vec![
            Span::styled("up ", Style::new().fg(self.palette.dim)),
            Span::styled(format!("{uptime}  "), Style::new().fg(self.palette.fg)),
            Span::styled("load: ", Style::new().fg(self.palette.dim)),
            Span::styled(load, Style::new().fg(self.palette.fg)),
        ]);
        frame.render_widget(left, halves[0]);
        frame.render_widget(
            Paragraph::new(Span::styled(time, Style::new().fg(self.palette.dim))),
            halves[1],
        );
    }

    fn draw_cpu_row(&self, frame: &mut Frame, area: Rect) {
        let dim = Style::new().fg(self.palette.dim);
        let fg = Style::new().fg(self.palette.fg);
        let label = Span::styled("cpu  ", dim);

        // Full-word labels (vs. top's us/sy/ni/id/wa/hi/si/st) so readers don't
        // need to know the top glossary. Percentages are right-aligned to keep
        // columns lined up as values fluctuate.
        let spans = match self.cpu.as_ref().and_then(|c| c.cpu_modes) {
            Some(m) => vec![
                label,
                Span::styled(format!("{:>5.1}%", m.user), fg),
                Span::styled(" user  ", dim),
                Span::styled(format!("{:>5.1}%", m.system), fg),
                Span::styled(" sys  ", dim),
                Span::styled(format!("{:>5.1}%", m.nice), fg),
                Span::styled(" nice  ", dim),
                Span::styled(format!("{:>5.1}%", m.idle), fg),
                Span::styled(" idle  ", dim),
                Span::styled(format!("{:>5.1}%", m.iowait), fg),
                Span::styled(" iowait  ", dim),
                Span::styled(format!("{:>4.1}%", m.irq), fg),
                Span::styled(" irq  ", dim),
                Span::styled(format!("{:>4.1}%", m.softirq), fg),
                Span::styled(" softirq  ", dim),
                Span::styled(format!("{:>4.1}%", m.steal), fg),
                Span::styled(" steal", dim),
            ],
            None => vec![label, Span::styled("—", dim)],
        };
        frame.render_widget(Line::from(spans), area);
    }

    fn draw_tasks_row(&self, frame: &mut Frame, area: Rect) {
        let dim = Style::new().fg(self.palette.dim);
        let fg = Style::new().fg(self.palette.fg);

        let Some(proc) = &self.proc else {
            frame.render_widget(
                Line::from(vec![Span::styled("tasks  ", dim), Span::styled("—", dim)]),
                area,
            );
            return;
        };

        // Count only top-level processes, not threads.
        let mut total = 0usize;
        let mut running = 0usize;
        let mut sleeping = 0usize;
        let mut stopped = 0usize;
        let mut zombie = 0usize;
        for p in &proc.processes {
            if p.is_thread {
                continue;
            }
            total += 1;
            match p.status {
                ProcessStatus::Running => running += 1,
                ProcessStatus::Sleeping | ProcessStatus::Idle => sleeping += 1,
                ProcessStatus::Stopped => stopped += 1,
                ProcessStatus::Zombie => zombie += 1,
                _ => {}
            }
        }

        let spans = vec![
            Span::styled("tasks  ", dim),
            Span::styled(format!("{total}"), fg),
            Span::styled(" total  ", dim),
            Span::styled(format!("{running}"), fg),
            Span::styled(" running  ", dim),
            Span::styled(format!("{sleeping}"), fg),
            Span::styled(" sleeping  ", dim),
            Span::styled(format!("{stopped}"), fg),
            Span::styled(" stopped  ", dim),
            Span::styled(format!("{zombie}"), fg),
            Span::styled(" zombie", dim),
        ];
        frame.render_widget(Line::from(spans), area);
    }

    fn draw_ram_row(&self, frame: &mut Frame, area: Rect) {
        let Some(mem) = &self.mem else { return };

        let ratio = if mem.ram_total > 0 {
            (mem.ram_used as f64 / mem.ram_total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let cols = Layout::horizontal([Constraint::Fill(1), Constraint::Length(MEM_LABEL_WIDTH)])
            .split(area);

        frame.render_widget(
            Gauge::default()
                .ratio(ratio)
                .label("")
                .gauge_style(Style::new().fg(self.palette.accent)),
            cols[0],
        );

        // `ram_free`/`ram_buffers`/`ram_cached`/`ram_available` are only populated
        // on Linux; on other platforms they are zero and we render just the
        // used/total totals to avoid showing misleading "free 0" labels.
        let label = if mem.ram_available > 0 || mem.ram_free > 0 {
            format!(
                "RAM {}/{}  free {}  buffer/cache {}  available {}",
                fmt_bytes(mem.ram_used),
                fmt_bytes(mem.ram_total),
                fmt_bytes(mem.ram_free),
                fmt_bytes(mem.ram_buffers + mem.ram_cached),
                fmt_bytes(mem.ram_available),
            )
        } else {
            format!(
                "RAM {}/{}  {:>5.1}%",
                fmt_bytes(mem.ram_used),
                fmt_bytes(mem.ram_total),
                ratio * 100.0,
            )
        };

        frame.render_widget(
            Paragraph::new(Span::styled(label, Style::new().fg(self.palette.fg))),
            cols[1],
        );
    }

    fn draw_swap_row(&self, frame: &mut Frame, area: Rect) {
        let Some(mem) = &self.mem else { return };
        let ratio = if mem.swap_total > 0 {
            (mem.swap_used as f64 / mem.swap_total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let color = if mem.swap_used > 0 {
            self.palette.warn
        } else {
            self.palette.dim
        };

        let cols = Layout::horizontal([Constraint::Fill(1), Constraint::Length(MEM_LABEL_WIDTH)])
            .split(area);

        frame.render_widget(
            Gauge::default()
                .ratio(ratio)
                .label("")
                .gauge_style(Style::new().fg(color)),
            cols[0],
        );

        let free = mem.swap_total.saturating_sub(mem.swap_used);
        let label = format!(
            "SWAP {}/{}  free {}",
            fmt_bytes(mem.swap_used),
            fmt_bytes(mem.swap_total),
            fmt_bytes(free),
        );
        frame.render_widget(
            Paragraph::new(Span::styled(label, Style::new().fg(color))),
            cols[1],
        );
    }
}

fn fmt_bytes(bytes: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    const KIB: u64 = 1024;
    if bytes >= GIB {
        format!("{:.1}G", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1}M", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1}K", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes}B")
    }
}

fn format_uptime(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        action::Action,
        stats::snapshots::{CpuSnapshot, MemSnapshot, ProcSnapshot, SysSnapshot},
    };
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};

    fn fixed_sys() -> SysSnapshot {
        use chrono::TimeZone;
        SysSnapshot {
            hostname: "dev-box".into(),
            uptime: 273_600,
            load_avg: [1.24, 0.98, 0.87],
            timestamp: chrono::Local
                .with_ymd_and_hms(2026, 3, 25, 12, 0, 0)
                .unwrap(),
        }
    }

    #[test]
    fn renders_all_rows_with_swap() {
        let mut comp = StatusBarComponent::new(ColorPalette::dark());
        comp.update(&Action::SysUpdate(fixed_sys())).unwrap();
        comp.update(&Action::CpuUpdate(CpuSnapshot::stub()))
            .unwrap();
        comp.update(&Action::MemUpdate(MemSnapshot::stub()))
            .unwrap();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();

        assert_eq!(comp.preferred_height(), Some(7));
        let mut terminal = Terminal::new(TestBackend::new(120, 7)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn hides_swap_row_when_no_swap() {
        let mut comp = StatusBarComponent::new(ColorPalette::dark());
        let mut mem = MemSnapshot::stub();
        mem.swap_total = 0;
        mem.swap_used = 0;

        comp.update(&Action::SysUpdate(fixed_sys())).unwrap();
        comp.update(&Action::CpuUpdate(CpuSnapshot::stub()))
            .unwrap();
        comp.update(&Action::MemUpdate(mem)).unwrap();
        comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
            .unwrap();

        assert_eq!(comp.preferred_height(), Some(6));
    }
}
