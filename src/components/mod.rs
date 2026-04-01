// SPDX-License-Identifier: GPL-3.0-only

use anyhow::Result;
use crossterm::event::KeyEvent;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use strum::{Display, EnumIter};

use crate::theme::ColorPalette;

pub mod cpu;
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
}

/// 32 visually distinct colors for multi-series graphs (per-core CPU, per-interface net, etc.).
///
/// The first 16 are vivid, spread across the hue wheel. The next 16 are lighter
/// tinted variants of the same hues. Cycles for systems with more than 32 series.
pub const SERIES_COLORS: [Color; 32] = [
    // Row 1: vivid primaries, hues spaced ~22° apart
    Color::Rgb(0, 220, 220),  // cyan
    Color::Rgb(220, 210, 0),  // yellow
    Color::Rgb(50, 210, 80),  // green
    Color::Rgb(210, 60, 210), // magenta
    Color::Rgb(80, 140, 255), // cornflower blue
    Color::Rgb(255, 85, 55),  // red-orange
    Color::Rgb(0, 210, 150),  // teal
    Color::Rgb(255, 155, 0),  // orange
    Color::Rgb(160, 75, 255), // violet
    Color::Rgb(145, 225, 0),  // lime
    Color::Rgb(255, 60, 155), // hot pink
    Color::Rgb(0, 165, 255),  // sky blue
    Color::Rgb(235, 185, 30), // golden
    Color::Rgb(0, 200, 180),  // aquamarine
    Color::Rgb(255, 135, 75), // peach-orange
    Color::Rgb(135, 55, 220), // purple
    // Row 2: lighter tints of the same 16 hues
    Color::Rgb(100, 240, 240), // light cyan
    Color::Rgb(245, 245, 110), // light yellow
    Color::Rgb(110, 240, 145), // light green
    Color::Rgb(240, 130, 240), // light magenta
    Color::Rgb(145, 185, 255), // light cornflower
    Color::Rgb(255, 145, 130), // light red-orange
    Color::Rgb(100, 230, 195), // light teal
    Color::Rgb(255, 205, 95),  // light orange
    Color::Rgb(200, 135, 255), // light violet
    Color::Rgb(190, 250, 95),  // light lime
    Color::Rgb(255, 130, 195), // light hot pink
    Color::Rgb(100, 205, 255), // light sky blue
    Color::Rgb(255, 225, 130), // light golden
    Color::Rgb(100, 235, 210), // light aquamarine
    Color::Rgb(255, 190, 145), // light peach
    Color::Rgb(185, 130, 255), // light purple
];

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

/// History ring-buffer length shared by all sparkline/chart components.
pub(crate) const HISTORY_LEN: usize = 100;

/// Format a byte rate with the "/s" suffix, for axis labels and sparkline annotations.
pub(crate) fn fmt_rate(bytes_per_sec: u64) -> String {
    const MB: u64 = 1_000_000;
    const KB: u64 = 1_000;
    if bytes_per_sec >= MB {
        format!("{:.1} MB/s", bytes_per_sec as f64 / MB as f64)
    } else if bytes_per_sec >= KB {
        format!("{:.1} KB/s", bytes_per_sec as f64 / KB as f64)
    } else {
        format!("{} B/s", bytes_per_sec)
    }
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

    fn update(&mut self, _action: &crate::action::Action) -> Result<Option<crate::action::Action>> {
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()>;
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
