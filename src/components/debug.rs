// SPDX-License-Identifier: GPL-3.0-only

use crate::{action::Action, components::Component, theme::ColorPalette};
use anyhow::Result;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Paragraph, Wrap},
};

#[derive(Debug)]
pub struct DebugComponent {
    palette: ColorPalette,
    snapshot: String,
}

impl Default for DebugComponent {
    fn default() -> Self {
        Self {
            palette: ColorPalette::dark(),
            snapshot: String::new(),
        }
    }
}

impl DebugComponent {
    pub fn new(palette: ColorPalette) -> Self {
        Self {
            palette,
            snapshot: String::new(),
        }
    }
}

impl Component for DebugComponent {
    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        if let Action::DebugSnapshot(s) = action {
            self.snapshot = s;
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let block = Block::default()
            .title(" DEBUG ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(self.palette.warn));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let para = Paragraph::new(self.snapshot.as_str())
            .style(Style::new().fg(self.palette.dim))
            .wrap(Wrap { trim: true });
        frame.render_widget(para, inner);
        Ok(())
    }
}
