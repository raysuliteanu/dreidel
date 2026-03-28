// SPDX-License-Identifier: GPL-3.0-only

use anyhow::Result;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{components::Component, config::KeyBindings, theme::ColorPalette};

// Popup dimensions
const POPUP_WIDTH: u16 = 46;
const POPUP_HEIGHT: u16 = 19;

#[derive(Debug)]
pub struct HelpComponent {
    palette: ColorPalette,
    kb: KeyBindings,
}

impl HelpComponent {
    pub fn new(palette: ColorPalette, kb: KeyBindings) -> Self {
        Self { palette, kb }
    }

    /// Compute a centered rect of the given size within `area`.
    fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
        let w = width.min(area.width);
        let h = height.min(area.height);
        let cols = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(w),
            Constraint::Fill(1),
        ])
        .split(area);
        let rows = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(h),
            Constraint::Fill(1),
        ])
        .split(cols[1]);
        rows[1]
    }
}

impl Component for HelpComponent {
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let popup = Self::centered_rect(POPUP_WIDTH, POPUP_HEIGHT, area);

        // Clear background so the popup isn't transparent
        frame.render_widget(Clear, popup);

        let version = env!("CARGO_PKG_VERSION");
        let repository = env!("CARGO_PKG_REPOSITORY");
        let change_id = option_env!("JJ_CHANGE_ID").unwrap_or("");

        let title = format!(" toppers v{version} ");
        let block = Block::default()
            .title(Span::styled(
                title,
                Style::new()
                    .fg(self.palette.fg)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::new().fg(self.palette.accent));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        let kb = &self.kb;
        let dim = Style::new().fg(self.palette.dim);
        let key = Style::new()
            .fg(self.palette.accent)
            .add_modifier(Modifier::BOLD);
        let fg = Style::new().fg(self.palette.fg);

        let mut lines: Vec<Line> = vec![
            Line::from(Span::styled(
                "Global keys",
                Style::new()
                    .fg(self.palette.fg)
                    .add_modifier(Modifier::UNDERLINED),
            )),
            key_line(kb.focus_proc, "focus process", &key, &dim),
            key_line(kb.focus_cpu, "focus cpu", &key, &dim),
            key_line(kb.focus_net, "focus net", &key, &dim),
            key_line(kb.focus_disk, "focus disk", &key, &dim),
            key_line(kb.fullscreen, "fullscreen toggle", &key, &dim),
            key_line(kb.debug, "debug sidebar toggle", &key, &dim),
            key_line('q', "quit", &key, &dim),
            Line::from(vec![
                Span::styled(format!(" {:<4}", "Tab"), key),
                Span::styled(" cycle focus", dim),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Process keys",
                Style::new()
                    .fg(self.palette.fg)
                    .add_modifier(Modifier::UNDERLINED),
            )),
            key_line('/', "filter", &key, &dim),
            key_line('s', "cycle sort column", &key, &dim),
            key_line('k', "kill process", &key, &dim),
            Line::from(""),
        ];

        if !repository.is_empty() {
            lines.push(Line::from(Span::styled(repository, fg)));
        }
        if !change_id.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("change: ", dim),
                Span::styled(change_id, Style::new().fg(self.palette.dim)),
            ]));
        }
        lines.push(Line::from(Span::styled("Press ?, h or Esc to close", dim)));

        frame.render_widget(Paragraph::new(lines), inner);
        Ok(())
    }
}

fn key_line<'a>(c: char, desc: &'a str, key: &Style, dim: &Style) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!(" {:<4}", c), *key),
        Span::styled(format!(" {desc}"), *dim),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn renders_help_overlay() {
        let mut settings = insta::Settings::clone_current();
        // JJ change IDs are baked in at compile time and change every commit;
        // redact them so the snapshot stays stable across commits.
        settings.add_filter(r"change: [a-z0-9]+", "change: [CHANGE_ID]");
        settings.bind(|| {
            let mut comp = HelpComponent::new(ColorPalette::dark(), KeyBindings::default());
            let mut terminal = Terminal::new(TestBackend::new(60, 24)).unwrap();
            terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
            assert_snapshot!("help_overlay", terminal.backend());
        });
    }
}
