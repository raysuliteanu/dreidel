use anyhow::Result;
use crossterm::event::KeyEvent;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
};
use strum::{Display, EnumIter};

use crate::theme::ColorPalette;

pub mod cpu;
pub mod debug;
pub mod disk;
pub mod help;
pub mod net;
pub mod process;
pub mod status_bar;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumIter, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum ComponentId {
    StatusBar,
    Cpu,
    Net,
    Disk,
    Process,
    Debug,
}

/// Build a block title with the focus key highlighted: ` [K]rest `.
/// The bracket and uppercase key are accent+bold; the rest is fg.
pub(crate) fn keyed_title(key: char, rest: &str, palette: &ColorPalette) -> Line<'static> {
    let accent_bold = Style::new().fg(palette.accent).add_modifier(Modifier::BOLD);
    let fg = Style::new().fg(palette.fg);
    Line::from(vec![
        Span::styled(" [".to_string(), accent_bold),
        Span::styled(key.to_ascii_uppercase().to_string(), accent_bold),
        Span::styled(format!("]{rest} "), fg),
    ])
}

/// Core interface every TUI panel must implement.
///
/// Default no-op implementations are provided so panels only override what
/// they actually need.
pub trait Component: std::fmt::Debug {
    /// Called before every render pass so the component can style itself
    /// differently when it holds keyboard focus.
    fn set_focused(&mut self, _focused: bool) {}

    /// Preferred height in terminal rows, used by the layout engine to size
    /// the panel tightly to its content. Returns `None` when the component
    /// has no preference and should fill available space.
    fn preferred_height(&self) -> Option<u16> {
        None
    }

    fn handle_key_event(&mut self, _key: KeyEvent) -> Result<Option<crate::action::Action>> {
        Ok(None)
    }

    fn update(&mut self, _action: crate::action::Action) -> Result<Option<crate::action::Action>> {
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()>;
}
