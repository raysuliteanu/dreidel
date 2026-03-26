use anyhow::Result;
use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};
use strum::{Display, EnumIter};

pub mod cpu;
pub mod debug;
pub mod disk;
pub mod mem;
pub mod net;
pub mod process;
pub mod status_bar;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumIter, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum ComponentId {
    StatusBar,
    Cpu,
    Mem,
    Net,
    Disk,
    Process,
    Debug,
}

/// Core interface every TUI panel must implement.
///
/// Default no-op implementations are provided so panels only override what
/// they actually need.
pub trait Component: std::fmt::Debug {
    fn handle_key_event(&mut self, _key: KeyEvent) -> Result<Option<crate::action::Action>> {
        Ok(None)
    }

    fn update(&mut self, _action: crate::action::Action) -> Result<Option<crate::action::Action>> {
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()>;
}
