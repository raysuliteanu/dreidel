// SPDX-License-Identifier: GPL-3.0-only

//! The [`Component`] trait and shared utilities for all UI panels.
//!
//! Each panel (CPU, Network, Disk, Process, Status Bar, Help) implements
//! [`Component`]. This module also exports shared types ([`ComponentId`],
//! `ListView`, `FilterEvent`, `FilterInput`) and helper functions used by
//! the Net and Disk panels.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders},
};
use strum::{Display, EnumIter};

use crate::{action::Action, theme::ColorPalette};

pub(crate) mod chart;
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

/// Shared view-state for list panels (Net, Disk) that support filtering and detail drill-down.
#[derive(Debug, Clone, Default)]
pub(crate) enum ListView {
    #[default]
    List,
    Filter {
        input: String,
    },
    Detail {
        name: String,
    },
}

/// Result of processing a key event while in filter mode.
pub(crate) enum FilterEvent {
    /// Esc: discard filter, return to list.
    Clear,
    /// Enter: accept current input (already reflected in `self.filter`), return to list.
    Commit,
    /// Backspace/Char: input was updated.
    Update(String),
    /// Key not consumed; original input returned unchanged.
    Ignored(String),
}

/// Stateless helper for filter-mode key handling shared by Net and Disk panels.
pub(crate) struct FilterInput;

impl FilterInput {
    pub(crate) fn handle_key(input: String, key: KeyEvent) -> FilterEvent {
        match key.code {
            KeyCode::Esc => FilterEvent::Clear,
            KeyCode::Enter => FilterEvent::Commit,
            KeyCode::Backspace => {
                let mut s = input;
                s.pop();
                FilterEvent::Update(s)
            }
            KeyCode::Char(c) => {
                let mut s = input;
                s.push(c);
                FilterEvent::Update(s)
            }
            _ => FilterEvent::Ignored(input),
        }
    }
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

/// History ring-buffer length shared by all sparkline/chart components.
pub(crate) const HISTORY_LEN: usize = 100;

/// SI byte-unit thresholds shared across all formatting helpers.
const MB: u64 = 1_000_000;
const KB: u64 = 1_000;
const GB: u64 = 1_000_000_000;
const TB: u64 = 1_000_000_000_000;

/// Floor applied to chart y-axes so they are never zero-height when idle.
pub(crate) const MIN_CHART_FLOOR: u64 = 1_024;

/// Rows scrolled per PageUp / PageDown keypress in list and table components.
pub(crate) const PAGE_SCROLL: usize = 10;

/// Format a byte rate with the "/s" suffix, for axis labels and sparkline annotations.
pub(crate) fn fmt_rate(bytes_per_sec: u64) -> String {
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
    if bytes_per_sec >= MB {
        format!("{:.1} MB", bytes_per_sec as f64 / MB as f64)
    } else if bytes_per_sec >= KB {
        format!("{:.1} KB", bytes_per_sec as f64 / KB as f64)
    } else {
        format!("{} B", bytes_per_sec)
    }
}

/// Format an absolute byte count with decimal SI suffixes (TB/GB/MB/KB/B).
/// Falls back to `fmt_rate_col` for sub-MB values to reuse its KB/B logic.
pub(crate) fn fmt_bytes(bytes: u64) -> String {
    if bytes >= TB {
        format!("{:.1} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        fmt_rate_col(bytes)
    }
}

/// Handle a key event while the panel is in `ListView::Detail`.
///
/// Returns `Some(action)` in all cases (detail mode swallows every key so the
/// global handler cannot shift focus or close the modal), and resets `view` to
/// `ListView::List` on Esc / q / Q.  Used by both `NetComponent` and
/// `DiskComponent` to avoid duplicating this arm.
pub(crate) fn handle_detail_key(key: KeyEvent, is_fullscreen: bool, view: &mut ListView) -> Action {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            *view = ListView::List;
            if is_fullscreen {
                Action::ToggleFullScreen
            } else {
                Action::Render
            }
        }
        // Swallow all other keys so they don't reach the global handler
        // (which would shift focus or trigger other app-level shortcuts).
        _ => Action::Render,
    }
}

/// Build the focused/unfocused border block used by list panels (Net, Disk).
///
/// Shared between `NetComponent` and `DiskComponent` — identical implementation.
pub(crate) fn list_border_block(
    focus_key: char,
    rest: &str,
    palette: &ColorPalette,
    focused: bool,
) -> Block<'static> {
    let border_color = if focused {
        palette.accent
    } else {
        palette.border
    };
    Block::default()
        .title(keyed_title(focus_key, rest, palette))
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_color))
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

    /// Called by `App` immediately before the fullscreen overlay render pass.
    ///
    /// The default no-op is correct for components that do not support fullscreen.
    /// Components that support fullscreen override this to set an internal
    /// `rendering_as_overlay` flag, which `draw()` consumes to distinguish the
    /// compact background pass from the overlay pass.
    fn begin_overlay_render(&mut self) {}

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
