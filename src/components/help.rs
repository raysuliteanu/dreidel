// SPDX-License-Identifier: GPL-3.0-only

//! Help overlay — fullscreen keybinding reference.
//!
//! Activated by pressing `?`. Displays all keyboard shortcuts, the config
//! file path, and the log file path. The dashboard continues updating behind
//! the overlay.

use anyhow::Result;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use std::path::PathBuf;

use crate::{
    components::Component,
    config::KeyBindings,
    theme::{ColorPalette, Theme},
};

// Popup dimensions; inner area is (width-2) × (height-2) after the border.
const POPUP_WIDTH: u16 = 62;
const POPUP_HEIGHT: u16 = 35;

#[derive(Debug)]
pub struct HelpComponent {
    palette: ColorPalette,
    kb: KeyBindings,
    detected_theme: Option<Theme>,
    active_theme: Theme,
}

impl HelpComponent {
    pub fn new(
        palette: ColorPalette,
        kb: KeyBindings,
        detected_theme: Option<Theme>,
        active_theme: Theme,
    ) -> Self {
        Self {
            palette,
            kb,
            detected_theme,
            active_theme,
        }
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
        let commit_sha = option_env!("GIT_SHA").unwrap_or("unknown");
        let commit_id = if commit_sha.len() >= 7 && commit_sha != "unknown" {
            &commit_sha[..7]
        } else {
            commit_sha
        };

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

        // --- Global keys (full width) ---
        let global_lines: Vec<Line> = vec![
            Line::from(Span::styled("Global", section)),
            kl(kb.help, "help / close", &key, &dim),
            kl(kb.focus_proc, "focus process", &key, &dim),
            kl(kb.focus_cpu, "focus cpu", &key, &dim),
            kl(kb.focus_net, "focus net", &key, &dim),
            kl(kb.focus_disk, "focus disk", &key, &dim),
            kl(kb.fullscreen, "fullscreen", &key, &dim),
            kl('q', "quit", &key, &dim),
            kl("Tab", "cycle focus", &key, &dim),
        ];

        // --- Component grid: top row CPU | Disk ---
        let cpu_lines: Vec<Line> = vec![
            Line::from(Span::styled("CPU", section)),
            kl('/', "filter", &key, &dim),
            kl("↑↓", "navigate", &key, &dim),
            kl("PgUD", "page", &key, &dim),
        ];
        let disk_lines: Vec<Line> = vec![
            Line::from(Span::styled("Disk", section)),
            kl("Ent", "open detail", &key, &dim),
            kl('/', "filter", &key, &dim),
            kl("↑↓", "navigate", &key, &dim),
            kl("PgUD", "page", &key, &dim),
        ];

        // --- Component grid: bottom row Process | Net ---
        let proc_lines: Vec<Line> = vec![
            Line::from(Span::styled("Process", section)),
            kl("Ent", "open detail", &key, &dim),
            kl('/', "filter", &key, &dim),
            kl("↑↓", "navigate", &key, &dim),
            kl("PgUD", "page", &key, &dim),
            kl('s', "cycle sort", &key, &dim),
            kl('S', "reverse sort", &key, &dim),
            kl('k', "kill", &key, &dim),
            kl('t', "tree view", &key, &dim),
            kl("Spc", "expand/collapse", &key, &dim),
        ];
        let net_lines: Vec<Line> = vec![
            Line::from(Span::styled("Net", section)),
            kl("Ent", "open detail", &key, &dim),
            kl('/', "filter", &key, &dim),
            kl("↑↓", "navigate", &key, &dim),
            kl("PgUD", "page", &key, &dim),
        ];

        // --- Footer ---
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

        let footer_sep = Span::styled("─".repeat(inner.width as usize), dim);
        let detected_str = match self.detected_theme {
            Some(t) => t.to_string(),
            None => "n/a".to_string(),
        };
        let active_str = self.active_theme.to_string();

        let mut footer_lines: Vec<Line> = vec![
            Line::from(vec![
                Span::styled("   config: ", dim),
                Span::styled(config_path, fg),
            ]),
            Line::from(vec![
                Span::styled("      log: ", dim),
                Span::styled(log_path, fg),
            ]),
            Line::from(vec![
                Span::styled(" detected: ", dim),
                Span::styled(detected_str, fg),
            ]),
            Line::from(vec![
                Span::styled("    theme: ", dim),
                Span::styled(active_str, fg),
            ]),
        ];
        if !repository.is_empty() || !commit_id.is_empty() {
            footer_lines.push(Line::from(footer_sep));
            if !repository.is_empty() {
                footer_lines.push(Line::from(vec![
                    Span::styled("     Repo: ", dim),
                    Span::styled(repository, fg),
                ]));
            }
            if !commit_id.is_empty() {
                footer_lines.push(Line::from(vec![
                    Span::styled("Commit id: ", dim),
                    Span::styled(commit_id, dim),
                ]));
            }
        }

        // Heights for the grid rows
        let global_h = global_lines.len() as u16;
        // Each grid row is tall enough for the taller of its two columns
        let row1_h = cpu_lines.len().max(disk_lines.len()) as u16;
        let row2_h = proc_lines.len().max(net_lines.len()) as u16;
        let footer_h = footer_lines.len() as u16;

        // Vertical split:
        //   global  |  blank  |  ─rule─  |  row1  |  blank  |  row2  |  ─rule─  |  footer
        let [
            global_area,
            _,
            rule1_area,
            row1_area,
            _,
            row2_area,
            rule2_area,
            footer_area,
        ] = Layout::vertical([
            Constraint::Length(global_h),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(row1_h),
            Constraint::Length(1),
            Constraint::Length(row2_h),
            Constraint::Length(1),
            Constraint::Length(footer_h),
        ])
        .areas(inner);

        let make_rule = || Line::from(Span::styled("─".repeat(inner.width as usize), dim));
        let [cpu_area, disk_area] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(row1_area);
        let [proc_area, net_area] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(row2_area);

        frame.render_widget(Paragraph::new(global_lines), global_area);
        frame.render_widget(Paragraph::new(make_rule()), rule1_area);
        frame.render_widget(Paragraph::new(cpu_lines), cpu_area);
        frame.render_widget(Paragraph::new(disk_lines), disk_area);
        frame.render_widget(Paragraph::new(proc_lines), proc_area);
        frame.render_widget(Paragraph::new(net_lines), net_area);
        frame.render_widget(Paragraph::new(make_rule()), rule2_area);
        frame.render_widget(Paragraph::new(footer_lines), footer_area);

        Ok(())
    }
}

/// Render a key-binding row: key label (left-padded to 5 chars) + description.
/// Accepts any `Display` value so both `char` ('q') and `&str` ("Tab", "↑↓") work.
fn kl<'a>(label: impl std::fmt::Display, desc: &'a str, key: &Style, dim: &Style) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!(" {label:<5}"), *key),
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
        // Redact the version (bumped on each release).
        settings.add_filter(r"dreidel v[\d.]+", "dreidel v[VERSION]");
        // Redact the commit id (set at build time, varies per commit).
        settings.add_filter(r"Commit id: \w+", "Commit id: [COMMIT_ID]");
        // Config and log paths vary per system; redact them for snapshot stability.
        settings.add_filter(r"config: \S+", "config: [CONFIG_PATH]");
        settings.add_filter(r"log: \S+", "log: [LOG_PATH]");
        settings.add_filter(r"detected: \S+", "detected: [DETECTED]");
        settings.add_filter(r"theme: \S+", "theme: [THEME]");
        settings.bind(|| {
            let mut comp = HelpComponent::new(
                ColorPalette::dark(),
                KeyBindings::default(),
                Some(Theme::Dark),
                Theme::Dark,
            );
            // Wide and tall enough to show the full popup (62×33) with margin.
            let mut terminal = Terminal::new(TestBackend::new(68, 40)).unwrap();
            terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
            assert_snapshot!("help_overlay", terminal.backend());
        });
    }
}
