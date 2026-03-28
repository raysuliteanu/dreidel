// SPDX-License-Identifier: GPL-3.0-only

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

/// Truncate `s` to at most `max` Unicode scalar values, replacing the last 3
/// with `...` if truncated. Uses character-aware indexing to avoid panics on
/// multi-byte UTF-8 sequences (e.g. interface names with non-ASCII characters).
pub(crate) fn truncate(s: &str, max: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max {
        return s.to_string();
    }
    if max <= 3 {
        return s.chars().take(max).collect();
    }
    // Find the byte offset of the (max-3)th char so we can slice safely.
    let byte_end = s
        .char_indices()
        .nth(max - 3)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    format!("{}...", &s[..byte_end])
}

/// Format a byte rate without the "/s" suffix.
/// The column header carries the "(B/s)" unit context, so individual cells
/// can omit it to save horizontal space.
pub(crate) fn fmt_rate_col(bytes_per_sec: u64) -> String {
    const MB: u64 = 1_000_000;
    const KB: u64 = 1_000;
    if bytes_per_sec >= MB {
        format!("{:.1} MB", bytes_per_sec as f64 / MB as f64)
    } else if bytes_per_sec >= KB {
        format!("{:.1} KB", bytes_per_sec as f64 / KB as f64)
    } else {
        format!("{} B", bytes_per_sec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_ascii_long() {
        assert_eq!(truncate("wlp0s20f3u1u2", 10), "wlp0s20...");
    }

    #[test]
    fn truncate_ascii_short() {
        assert_eq!(truncate("lo", 10), "lo");
    }

    #[test]
    fn truncate_multibyte_utf8_does_not_panic() {
        // "café" is 5 bytes but 4 chars; max=3 (≤3) → take 3 chars = "caf".
        // The old byte-slicing implementation would panic here on some inputs;
        // this verifies we use char-aware indexing.
        let result = truncate("café", 3);
        assert_eq!(result, "caf");
    }

    #[test]
    fn truncate_multibyte_utf8_longer() {
        // "cáféö" = 5 chars; truncate to 4 → 1 char + "..." = "c...".
        // (max - 3 = 1 char before the ellipsis)
        let result = truncate("cáféö", 4);
        assert_eq!(result, "c...");
        // Verify the result is valid UTF-8 (i.e. no mid-codepoint slicing).
        assert!(std::str::from_utf8(result.as_bytes()).is_ok());
    }
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
