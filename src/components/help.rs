// SPDX-License-Identifier: GPL-3.0-only

use anyhow::Result;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use std::path::PathBuf;

use crate::{components::Component, config::KeyBindings, theme::ColorPalette};

// Popup dimensions
const POPUP_WIDTH: u16 = 55;
const POPUP_HEIGHT: u16 = 30;

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

        let title = format!(" dreidel v{version} ");
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

        let section = Style::new()
            .fg(self.palette.fg)
            .add_modifier(Modifier::UNDERLINED);

        let config_path = tilde_path(
            dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("dreidel/config.toml"),
        );
        let log_path = tilde_path(
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("dreidel/dreidel.log"),
        );

        let mut lines: Vec<Line> = vec![
            Line::from(Span::styled("Global keys", section)),
            key_line(kb.focus_proc, "focus process", &key, &dim),
            key_line(kb.focus_cpu, "focus cpu", &key, &dim),
            key_line(kb.focus_net, "focus net", &key, &dim),
            key_line(kb.focus_disk, "focus disk", &key, &dim),
            key_line(kb.fullscreen, "fullscreen toggle", &key, &dim),
            key_line('q', "quit", &key, &dim),
            Line::from(vec![
                Span::styled(format!(" {:<4}", "Tab"), key),
                Span::styled(" cycle focus", dim),
            ]),
            Line::from(""),
            Line::from(Span::styled("Process keys", section)),
            Line::from(vec![
                Span::styled(format!(" {:<4}", "Ent"), key),
                Span::styled(" open detail", dim),
            ]),
            key_line('/', "filter", &key, &dim),
            key_line('s', "cycle sort column", &key, &dim),
            key_line('k', "kill process", &key, &dim),
            Line::from(""),
            Line::from(Span::styled("Net keys", section)),
            Line::from(vec![
                Span::styled(format!(" {:<4}", "Ent"), key),
                Span::styled(" open detail (fullscreen)", dim),
            ]),
            Line::from(""),
            Line::from(Span::styled("Disk keys", section)),
            Line::from(vec![
                Span::styled(format!(" {:<4}", "Ent"), key),
                Span::styled(" open detail (fullscreen)", dim),
            ]),
            Line::from(""),
        ];

        if !repository.is_empty() {
            lines.push(Line::from(Span::styled(repository, fg)));
        }
        if !change_id.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("change id: ", dim),
                Span::styled(change_id, Style::new().fg(self.palette.dim)),
            ]));
        }
        lines.push(Line::from(vec![
            Span::styled("   config: ", dim),
            Span::styled(config_path, fg),
        ]));
        lines.push(Line::from(vec![
            Span::styled("      log: ", dim),
            Span::styled(log_path, fg),
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Press ?, h, q or Esc to close",
            dim,
        )));

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

fn tilde_path(path: PathBuf) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(rel) = path.strip_prefix(&home)
    {
        return format!("~/{}", rel.display());
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn renders_help_overlay() {
        let mut settings = insta::Settings::clone_current();
        // The "change id:" line is only rendered when jj is available at build time (not in CI).
        // Strip the entire line so the snapshot is stable in both environments.
        settings.add_filter(r"[^\n]*change id:[^\n]*\n", "");
        // Config and log paths vary per system; redact them for snapshot stability.
        settings.add_filter(r"config: \S+", "config: [CONFIG_PATH]");
        settings.add_filter(r"log: \S+", "log: [LOG_PATH]");
        settings.bind(|| {
            let mut comp = HelpComponent::new(ColorPalette::dark(), KeyBindings::default());
            // Tall enough to show the full popup (POPUP_HEIGHT=28) with margin.
            let mut terminal = Terminal::new(TestBackend::new(60, 32)).unwrap();
            terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
            assert_snapshot!("help_overlay", terminal.backend());
        });
    }
}
