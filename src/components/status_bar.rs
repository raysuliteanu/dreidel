use anyhow::Result;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
};

use crate::{
    action::Action, components::Component, stats::snapshots::SysSnapshot, theme::ColorPalette,
};

#[derive(Debug)]
pub struct StatusBarComponent {
    palette: ColorPalette,
    sys: Option<SysSnapshot>,
}

impl StatusBarComponent {
    pub fn new(palette: ColorPalette) -> Self {
        Self { palette, sys: None }
    }
}

impl Component for StatusBarComponent {
    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        if let Action::SysUpdate(snap) = action {
            self.sys = Some(snap);
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let Some(sys) = &self.sys else {
            return Ok(());
        };

        let uptime = format_uptime(sys.uptime);
        let load = format!(
            "{:.2} {:.2} {:.2}",
            sys.load_avg[0], sys.load_avg[1], sys.load_avg[2]
        );
        let time = sys.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();

        let line = Line::from(vec![
            Span::styled(
                format!(" {} ", sys.hostname),
                Style::new().fg(self.palette.accent).bold(),
            ),
            Span::styled("| ", Style::new().fg(self.palette.border)),
            Span::styled(format!("up {} ", uptime), Style::new().fg(self.palette.fg)),
            Span::styled("| load: ", Style::new().fg(self.palette.dim)),
            Span::styled(format!("{} ", load), Style::new().fg(self.palette.fg)),
            Span::styled("| ", Style::new().fg(self.palette.border)),
            Span::styled(time, Style::new().fg(self.palette.dim)),
        ]);
        frame.render_widget(line, area);
        Ok(())
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
    use crate::{action::Action, stats::snapshots::SysSnapshot};
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};

    fn fixed_snapshot() -> SysSnapshot {
        use chrono::TimeZone;
        SysSnapshot {
            hostname: "dev-box".into(),
            uptime: 273_600,
            load_avg: [1.24, 0.98, 0.87],
            // Fixed timestamp so the snapshot is deterministic across runs
            timestamp: chrono::Local
                .with_ymd_and_hms(2026, 3, 25, 12, 0, 0)
                .unwrap(),
        }
    }

    #[test]
    fn renders_hostname_and_uptime() {
        let mut comp = StatusBarComponent::new(ColorPalette::dark());
        comp.update(Action::SysUpdate(fixed_snapshot())).unwrap();

        let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!(terminal.backend());
    }
}
