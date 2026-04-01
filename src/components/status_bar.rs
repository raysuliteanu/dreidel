// SPDX-License-Identifier: GPL-3.0-only

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
    stats::snapshots::{MemSnapshot, SysSnapshot},
    theme::ColorPalette,
};

// Width reserved for the right-aligned label on each mem gauge row.
const MEM_LABEL_WIDTH: u16 = 30;

#[derive(Debug)]
pub struct StatusBarComponent {
    palette: ColorPalette,
    sys: Option<SysSnapshot>,
    mem: Option<MemSnapshot>,
}

impl StatusBarComponent {
    pub fn new(palette: ColorPalette) -> Self {
        Self {
            palette,
            sys: None,
            mem: None,
        }
    }
}

impl Component for StatusBarComponent {
    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        match action {
            Action::SysUpdate(snap) => self.sys = Some(snap.clone()),
            Action::MemUpdate(snap) => self.mem = Some(snap.clone()),
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        // Use the hostname as the block title so it's visible without taking a content row.
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

        let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(inner);

        // Row 0: uptime / load / time (hostname moved to block title)
        if let Some(sys) = &self.sys {
            let uptime = format_uptime(sys.uptime);
            let load = format!(
                "{:.2} {:.2} {:.2}",
                sys.load_avg[0], sys.load_avg[1], sys.load_avg[2]
            );
            let time = sys.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
            let line = Line::from(vec![
                Span::styled("up ", Style::new().fg(self.palette.dim)),
                Span::styled(format!("{} ", uptime), Style::new().fg(self.palette.fg)),
                Span::styled("| load: ", Style::new().fg(self.palette.dim)),
                Span::styled(format!("{} ", load), Style::new().fg(self.palette.fg)),
                Span::styled("| ", Style::new().fg(self.palette.border)),
                Span::styled(time, Style::new().fg(self.palette.dim)),
            ]);
            frame.render_widget(line, rows[0]);
        }

        // Row 1: RAM gauge | SWAP gauge
        let Some(mem) = &self.mem else {
            return Ok(());
        };

        let ram_ratio = if mem.ram_total > 0 {
            (mem.ram_used as f64 / mem.ram_total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let swap_ratio = if mem.swap_total > 0 {
            (mem.swap_used as f64 / mem.swap_total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let swap_color = if mem.swap_used > 0 {
            self.palette.warn
        } else {
            self.palette.dim
        };

        // Split row 1: RAM on the left, separator, SWAP on the right.
        let halves = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .split(rows[1]);
        frame.render_widget(
            Span::styled("│", Style::new().fg(self.palette.border)),
            halves[1],
        );

        // RAM half: gauge | label
        let ram_cols =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(MEM_LABEL_WIDTH)])
                .split(halves[0]);
        frame.render_widget(
            Gauge::default()
                .ratio(ram_ratio)
                .label("")
                .gauge_style(Style::new().fg(self.palette.accent)),
            ram_cols[0],
        );
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!(
                    "RAM  {}/{} {:>5.1}%",
                    fmt_bytes(mem.ram_used),
                    fmt_bytes(mem.ram_total),
                    ram_ratio * 100.0,
                ),
                Style::new().fg(self.palette.fg),
            )),
            ram_cols[1],
        );

        // SWAP half: gauge | label (omit if no swap configured)
        if mem.swap_total > 0 {
            let swap_cols =
                Layout::horizontal([Constraint::Fill(1), Constraint::Length(MEM_LABEL_WIDTH)])
                    .split(halves[2]);
            frame.render_widget(
                Gauge::default()
                    .ratio(swap_ratio)
                    .label("")
                    .gauge_style(Style::new().fg(swap_color)),
                swap_cols[0],
            );
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!(
                        "SWAP {}/{} {:>5.1}%",
                        fmt_bytes(mem.swap_used),
                        fmt_bytes(mem.swap_total),
                        swap_ratio * 100.0,
                    ),
                    Style::new().fg(swap_color),
                )),
                swap_cols[1],
            );
        }

        Ok(())
    }
}

fn fmt_bytes(bytes: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    const KIB: u64 = 1024;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn format_uptime(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    if d > 0 {
        format!("{}d {}h {}m", d, h, m)
    } else if h > 0 {
        format!("{}h {}m", h, m)
    } else {
        format!("{}m", m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        action::Action,
        stats::snapshots::{MemSnapshot, SysSnapshot},
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
    fn renders_hostname_and_uptime() {
        let mut comp = StatusBarComponent::new(ColorPalette::dark());
        comp.update(&Action::SysUpdate(fixed_sys())).unwrap();
        comp.update(&Action::MemUpdate(MemSnapshot::stub()))
            .unwrap();

        let mut terminal = Terminal::new(TestBackend::new(80, 4)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!(terminal.backend());
    }
}
