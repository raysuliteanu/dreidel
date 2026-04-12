// SPDX-License-Identifier: GPL-3.0-only

//! CPU panel — per-core usage line chart with scrollable history.
//!
//! Renders braille-character sparklines for each logical core, a label column
//! showing current usage percentage (and per-core temperature on Linux), and
//! an optional stats header in fullscreen mode with CPU brand, core counts,
//! frequency, package temperature, and governor.

use std::collections::VecDeque;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders},
};

use crate::{
    action::Action,
    components::chart::{HistoryChart, LegendEntry},
    components::{Component, FilterEvent, FilterInput, HISTORY_LEN, SERIES_COLORS, keyed_title},
    stats::snapshots::CpuSnapshot,
    theme::ColorPalette,
};

/// Max cores shown in the compact sidebar slot; also the page-scroll step so
/// one PageDown always advances by exactly one visible window.
const COMPACT_MAX_CORES: usize = 8;

fn core_color(idx: usize) -> Color {
    SERIES_COLORS[idx % SERIES_COLORS.len()]
}

#[derive(Debug)]
struct CpuCompactSnapshot {
    scroll_offset: usize,
    state: CpuState,
    filter: String,
}

/// State for the CPU panel's filter input mode.
#[derive(Debug, Default, Clone, PartialEq)]
enum CpuState {
    #[default]
    Normal,
    FilterMode {
        input: String,
    },
}

#[derive(Debug)]
pub struct CpuComponent {
    palette: ColorPalette,
    focus_key: char,
    latest: Option<CpuSnapshot>,
    /// Per-core usage history (0.0–100.0). Oldest at front, newest at back.
    pub(crate) per_core_history: Vec<VecDeque<f64>>,
    scroll_offset: usize,
    state: CpuState,
    /// Active core-label filter. Empty = show all cores.
    filter: String,
    focused: bool,
    is_fullscreen: bool,
    compact_snapshot: Option<CpuCompactSnapshot>,
    /// One-shot flag set by `begin_overlay_render()`.  Consumed at the start of
    /// `draw()` to distinguish the compact background pass from the overlay pass.
    rendering_as_overlay: bool,
}

impl Default for CpuComponent {
    fn default() -> Self {
        Self {
            palette: ColorPalette::dark(),
            focus_key: 'c',
            latest: None,
            per_core_history: Vec::new(),
            scroll_offset: 0,
            state: CpuState::Normal,
            filter: String::new(),
            focused: false,
            is_fullscreen: false,
            compact_snapshot: None,
            rendering_as_overlay: false,
        }
    }
}

impl CpuComponent {
    pub fn new(palette: ColorPalette, focus_key: char) -> Self {
        Self {
            palette,
            focus_key,
            ..Default::default()
        }
    }

    fn num_cores(&self) -> usize {
        self.latest.as_ref().map(|s| s.per_core.len()).unwrap_or(0)
    }

    /// Returns the indices of cores whose label matches the active filter.
    /// When the filter is empty, all core indices are returned.
    fn filtered_cores(&self) -> Vec<usize> {
        let n = self.num_cores();
        if self.filter.is_empty() {
            return (0..n).collect();
        }
        // self.filter is stored lowercase; only format!() needs to be checked.
        (0..n)
            .filter(|&i| format!("cpu{i}").contains(self.filter.as_str()))
            .collect()
    }

    /// Returns the *count* of matching cores without allocating a Vec.
    /// Used by `preferred_height` to avoid a heap allocation on every layout pass.
    fn filtered_cores_len(&self) -> usize {
        let n = self.num_cores();
        if self.filter.is_empty() {
            return n;
        }
        (0..n)
            .filter(|&i| format!("cpu{i}").contains(self.filter.as_str()))
            .count()
    }

    /// Clamp scroll_offset so the last visible row never exceeds the last
    /// matching core in the filtered list.
    ///
    /// `n` is the pre-computed filtered core count; callers that already have
    /// it should pass it here to avoid recomputing.
    fn clamp_scroll(&mut self, visible: usize, n: usize) {
        if n == 0 || visible >= n {
            self.scroll_offset = 0;
        } else {
            self.scroll_offset = self.scroll_offset.min(n - visible);
        }
    }

    fn draw_header(&self, frame: &mut Frame, area: Rect, snap: &CpuSnapshot) {
        let dim = Style::new().fg(self.palette.dim);
        let val = Style::new().fg(self.palette.fg);
        let ac = Style::new().fg(self.palette.accent);

        let [row0, row1] =
            Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);

        // Row 0: aggregate %, brand, logical core count (+ physical on Linux)
        let logical = snap.per_core.len();
        let mut row0_spans = vec![
            Span::styled("CPU: ", dim),
            Span::styled(format!("{:.1}%", snap.aggregate), ac),
            Span::styled("  Brand: ", dim),
            Span::styled(snap.cpu_brand.clone(), val),
            Span::styled("  Cores: ", dim),
            Span::styled(format!("{logical}"), val),
        ];
        #[cfg(target_os = "linux")]
        if let Some(phys) = snap.physical_core_count {
            row0_spans.push(Span::styled("/", dim));
            row0_spans.push(Span::styled(format!("{phys} phys"), val));
        }
        frame.render_widget(Line::from(row0_spans), row0);

        // Row 1: avg frequency, then Linux-only temperature and governor
        let avg_freq = if snap.frequency.is_empty() {
            0
        } else {
            snap.frequency.iter().sum::<u64>() / snap.frequency.len() as u64
        };
        let mut row1_spans = vec![
            Span::styled("Freq: ", dim),
            Span::styled(format!("{avg_freq} MHz"), val),
        ];
        #[cfg(target_os = "linux")]
        {
            if let Some(temp) = snap.package_temp {
                row1_spans.push(Span::styled("  Temp: ", dim));
                row1_spans.push(Span::styled(format!("{temp:.0}°C"), val));
            }
            if let Some(ref gov) = snap.governor {
                row1_spans.push(Span::styled("  Governor: ", dim));
                row1_spans.push(Span::styled(gov.clone(), val));
            }
        }
        frame.render_widget(Line::from(row1_spans), row1);
    }

    fn draw_chart(
        &self,
        frame: &mut Frame,
        area: Rect,
        snap: &CpuSnapshot,
        filtered: &[usize],
        first: usize,
        last: usize,
    ) {
        #[cfg(target_os = "linux")]
        let has_temps = snap.per_core_temp.iter().any(|t| t.is_some());
        #[cfg(not(target_os = "linux"))]
        let has_temps = false;

        let label_inner_w: u16 = if has_temps { 18 } else { 11 };
        let label_total_w: u16 = label_inner_w + 1;

        if area.width <= label_total_w + 4 {
            return;
        }

        let actual_visible = (last - first).min(area.height as usize);
        if actual_visible == 0 {
            return;
        }

        let mut chart = HistoryChart::new(HISTORY_LEN)
            .y_bounds(0.0, 100.0)
            .legend_width(label_total_w)
            .border_style(Style::new().fg(self.palette.border))
            .axis_style(Style::new().fg(self.palette.dim));

        for &core_idx in &filtered[first..last] {
            chart = chart.series(
                self.per_core_history[core_idx].iter().copied(),
                Style::new().fg(core_color(core_idx)),
            );
        }

        for &core_idx in &filtered[first..first + actual_visible] {
            let pct = snap.per_core[core_idx];
            let mut text = format!("cpu{:<2}{:>5.1}%", core_idx, pct);

            #[cfg(target_os = "linux")]
            if has_temps {
                if let Some(Some(temp)) = snap.per_core_temp.get(core_idx) {
                    text.push_str(&format!(" {:>4.0}°C", temp));
                } else {
                    text.push_str("       ");
                }
            }

            chart = chart.legend(LegendEntry::top(Span::styled(
                text,
                Style::new().fg(core_color(core_idx)),
            )));
        }

        frame.render_widget(chart, area);
    }

    fn restore_compact_snapshot(&mut self) {
        if let Some(snap) = self.compact_snapshot.take() {
            self.scroll_offset = snap.scroll_offset;
            self.state = snap.state;
            self.filter = snap.filter;
        }
        self.is_fullscreen = false;
    }

    /// Render the compact sidebar appearance using the frozen snapshot state.
    ///
    /// Called during the compact background pass (when the fullscreen overlay is
    /// open).  Temporarily swaps live fields with snapshot values, calls `draw()`
    /// with `is_fullscreen = false` (which skips the overlay-only header and clears
    /// the `is_fullscreen` guard so we don't recurse), then restores live state.
    fn draw_compact_background(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let Some(snap) = self.compact_snapshot.take() else {
            return Ok(()); // no snapshot yet — render nothing
        };

        let live_scroll = std::mem::replace(&mut self.scroll_offset, snap.scroll_offset);
        let live_state = std::mem::replace(&mut self.state, snap.state.clone());
        let live_filter = std::mem::replace(&mut self.filter, snap.filter.clone());
        let live_fs = std::mem::replace(&mut self.is_fullscreen, false);
        // rendering_as_overlay is already false (consumed at top of draw()),
        // so the recursive draw() call proceeds as a normal (non-fullscreen) render.

        let result = self.draw(frame, area);

        self.scroll_offset = live_scroll;
        self.state = live_state;
        self.filter = live_filter;
        self.is_fullscreen = live_fs;
        self.compact_snapshot = Some(snap);

        result
    }
}

impl Component for CpuComponent {
    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        if !focused && self.is_fullscreen {
            self.restore_compact_snapshot();
        }
    }

    fn begin_overlay_render(&mut self) {
        self.rendering_as_overlay = true;
    }

    fn preferred_height(&self) -> Option<u16> {
        // 2 borders + one row per visible (filtered) core, capped at COMPACT_MAX_CORES.
        // Use filtered_cores_len() to avoid allocating a Vec on every layout pass.
        let cores = self.filtered_cores_len().min(COMPACT_MAX_CORES);
        Some(2 + cores as u16)
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        if matches!(self.state, CpuState::FilterMode { .. }) {
            // Take ownership of input without cloning the whole enum.
            let input = match std::mem::replace(&mut self.state, CpuState::Normal) {
                CpuState::FilterMode { input } => input,
                _ => unreachable!("variant confirmed above"),
            };
            match FilterInput::handle_key(input, key) {
                FilterEvent::Clear => {
                    self.filter = String::new();
                    self.scroll_offset = 0;
                    // state is already CpuState::Normal from replace above
                }
                FilterEvent::Commit => {
                    // filter stays as-is; state stays Normal
                }
                FilterEvent::Update(s) => {
                    self.filter = s.to_lowercase(); // keep stored filter lowercased
                    self.state = CpuState::FilterMode { input: s };
                    self.scroll_offset = 0;
                }
                FilterEvent::Ignored(input) => {
                    // key not consumed — restore state
                    self.state = CpuState::FilterMode { input };
                }
            }
            return Ok(Some(Action::Render));
        }
        let n = self.filtered_cores_len();
        match key.code {
            KeyCode::Up => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            KeyCode::Down => {
                if n > 0 {
                    self.scroll_offset = (self.scroll_offset + 1).min(n - 1);
                }
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(COMPACT_MAX_CORES);
            }
            KeyCode::PageDown => {
                if n > 0 {
                    self.scroll_offset = (self.scroll_offset + COMPACT_MAX_CORES).min(n - 1);
                }
            }
            KeyCode::Char('/') => {
                self.state = CpuState::FilterMode {
                    input: self.filter.clone(),
                };
                return Ok(Some(Action::Render));
            }
            _ => {}
        }
        Ok(None)
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        match action {
            Action::CpuUpdate(snap) => {
                let n = snap.per_core.len();
                while self.per_core_history.len() < n {
                    self.per_core_history.push(VecDeque::new());
                }
                for (i, &pct) in snap.per_core.iter().enumerate() {
                    let hist = &mut self.per_core_history[i];
                    if hist.len() >= HISTORY_LEN {
                        hist.pop_front();
                    }
                    hist.push_back(pct as f64);
                }
                self.latest = Some(snap.clone());
            }
            Action::ToggleFullScreen => {
                if !self.is_fullscreen {
                    self.compact_snapshot = Some(CpuCompactSnapshot {
                        scroll_offset: self.scroll_offset,
                        state: self.state.clone(),
                        filter: self.filter.clone(),
                    });
                    self.is_fullscreen = true;
                } else {
                    self.restore_compact_snapshot();
                }
            }
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        // One-shot overlay flag: consumed here so the compact background pass
        // (is_fullscreen=true, rendering_as_overlay=false) and the overlay pass
        // (is_fullscreen=true, rendering_as_overlay=true) can be distinguished.
        let is_overlay = std::mem::replace(&mut self.rendering_as_overlay, false);

        // Compact background pass: render from frozen snapshot so that changes
        // made in the fullscreen overlay don't bleed into the compact sidebar.
        if self.is_fullscreen && !is_overlay {
            return self.draw_compact_background(frame, area);
        }

        let border_color = if self.focused {
            self.palette.accent
        } else {
            self.palette.border
        };
        let title_rest = match &self.state {
            CpuState::FilterMode { input } => format!("PU [/{}▌]", input),
            CpuState::Normal if !self.filter.is_empty() => format!("PU [/{}]", self.filter),
            CpuState::Normal => "PU".to_string(),
        };
        let title: Line = keyed_title(self.focus_key, &title_rest, &self.palette);
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border_color));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.latest.is_none() {
            return Ok(());
        }

        // Show stats header only in fullscreen when there's enough room.
        let show_header = self.is_fullscreen && inner.height >= 6;
        let header_h: u16 = if show_header { 2 } else { 0 };
        let sep_h: u16 = if show_header { 1 } else { 0 };

        let [header_area, sep_area, chart_area] = Layout::vertical([
            Constraint::Length(header_h),
            Constraint::Length(sep_h),
            Constraint::Fill(1),
        ])
        .areas(inner);

        // Precompute mutable state before borrowing `self.latest`.
        let filtered = self.filtered_cores();
        let n = filtered.len();
        let visible = chart_area.height as usize;
        self.clamp_scroll(visible, n);
        let first = self.scroll_offset;
        let last = (first + visible).min(n);

        // Borrow the snapshot only after all mutation is complete.
        let snap = self.latest.as_ref().expect("checked is_none above");

        if show_header {
            self.draw_header(frame, header_area, snap);
            frame.render_widget(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::new().fg(self.palette.border)),
                sep_area,
            );
        }

        if chart_area.height == 0 || chart_area.width == 0 {
            return Ok(());
        }

        self.draw_chart(frame, chart_area, snap, &filtered, first, last);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{action::Action, stats::snapshots::CpuSnapshot};
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn renders_without_data() {
        let mut comp = CpuComponent::default();
        let mut terminal = Terminal::new(TestBackend::new(60, 10)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("cpu_no_data", terminal.backend());
    }

    #[test]
    fn renders_with_cpu_data() {
        let mut comp = CpuComponent::default();
        comp.update(&Action::CpuUpdate(CpuSnapshot::stub()))
            .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(60, 10)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("cpu_with_data", terminal.backend());
    }

    #[test]
    fn renders_fullscreen_header() {
        let mut comp = CpuComponent::default();
        comp.update(&Action::CpuUpdate(CpuSnapshot::stub()))
            .unwrap();
        comp.update(&Action::ToggleFullScreen).unwrap();
        // Simulate the App's overlay render pass: App calls begin_overlay_render()
        // immediately before the draw() that produces the fullscreen modal content.
        comp.begin_overlay_render();
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        assert_snapshot!("cpu_fullscreen", terminal.backend());
    }

    #[test]
    fn history_ring_buffer_bounded() {
        let mut comp = CpuComponent::default();
        for _ in 0..200 {
            comp.update(&Action::CpuUpdate(CpuSnapshot::stub()))
                .unwrap();
        }
        for hist in &comp.per_core_history {
            assert!(hist.len() <= HISTORY_LEN);
        }
    }

    #[test]
    fn scroll_clamps_to_valid_range() {
        let mut comp = CpuComponent::default();
        comp.update(&Action::CpuUpdate(CpuSnapshot::stub()))
            .unwrap(); // 4 cores
        // Scroll far past the end
        comp.update(&Action::ToggleFullScreen).unwrap();
        for _ in 0..100 {
            comp.handle_key_event(crossterm::event::KeyEvent::new(
                KeyCode::Down,
                crossterm::event::KeyModifiers::NONE,
            ))
            .unwrap();
        }
        assert!(comp.scroll_offset < comp.num_cores());
    }

    #[test]
    fn core_color_cycles() {
        // Colors cycle at SERIES_COLORS.len() (32).
        assert_eq!(core_color(0), core_color(SERIES_COLORS.len()));
        assert_ne!(core_color(0), core_color(1));
    }

    #[test]
    fn slash_enters_filter_mode() {
        let mut comp = CpuComponent::default();
        comp.update(&Action::CpuUpdate(CpuSnapshot::stub()))
            .unwrap();
        comp.handle_key_event(crossterm::event::KeyEvent::new(
            KeyCode::Char('/'),
            crossterm::event::KeyModifiers::NONE,
        ))
        .unwrap();
        assert!(
            matches!(comp.state, CpuState::FilterMode { .. }),
            "/ must enter filter mode"
        );
    }

    #[test]
    fn filter_mode_char_updates_filter_and_state() {
        let mut comp = CpuComponent::default();
        comp.update(&Action::CpuUpdate(CpuSnapshot::stub()))
            .unwrap();
        let key = |c| {
            crossterm::event::KeyEvent::new(KeyCode::Char(c), crossterm::event::KeyModifiers::NONE)
        };
        comp.handle_key_event(key('/')).unwrap();
        comp.handle_key_event(key('0')).unwrap();
        assert_eq!(comp.filter, "0");
        assert!(matches!(comp.state, CpuState::FilterMode { ref input } if input == "0"));
    }

    #[test]
    fn filter_mode_esc_clears_filter_and_returns_to_normal() {
        let mut comp = CpuComponent::default();
        comp.update(&Action::CpuUpdate(CpuSnapshot::stub()))
            .unwrap();
        let key = |c| {
            crossterm::event::KeyEvent::new(KeyCode::Char(c), crossterm::event::KeyModifiers::NONE)
        };
        comp.handle_key_event(key('/')).unwrap();
        comp.handle_key_event(key('1')).unwrap();
        comp.handle_key_event(crossterm::event::KeyEvent::new(
            KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ))
        .unwrap();
        assert_eq!(comp.filter, "", "Esc must clear filter");
        assert_eq!(comp.state, CpuState::Normal);
    }

    #[test]
    fn filter_mode_enter_keeps_filter_and_returns_to_normal() {
        let mut comp = CpuComponent::default();
        comp.update(&Action::CpuUpdate(CpuSnapshot::stub()))
            .unwrap();
        let key = |c| {
            crossterm::event::KeyEvent::new(KeyCode::Char(c), crossterm::event::KeyModifiers::NONE)
        };
        comp.handle_key_event(key('/')).unwrap();
        comp.handle_key_event(key('1')).unwrap();
        comp.handle_key_event(crossterm::event::KeyEvent::new(
            KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ))
        .unwrap();
        assert_eq!(comp.filter, "1", "Enter must keep filter");
        assert_eq!(comp.state, CpuState::Normal);
    }

    #[test]
    fn filtered_cores_matches_only_matching_cores() {
        // CpuSnapshot::stub() has 4 cores (cpu0..cpu3).
        let mut comp = CpuComponent::default();
        comp.update(&Action::CpuUpdate(CpuSnapshot::stub()))
            .unwrap();
        comp.filter = "1".to_string(); // matches cpu1
        let cores = comp.filtered_cores();
        assert!(
            cores.contains(&1),
            "filter '1' must include cpu1; got: {cores:?}"
        );
        assert!(
            !cores.contains(&0),
            "filter '1' must not include cpu0; got: {cores:?}"
        );
    }

    #[test]
    fn empty_filter_shows_all_cores() {
        let mut comp = CpuComponent::default();
        comp.update(&Action::CpuUpdate(CpuSnapshot::stub()))
            .unwrap();
        assert_eq!(
            comp.filtered_cores(),
            (0..comp.num_cores()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn compact_state_restored_after_fullscreen_exit() {
        let mut comp = CpuComponent::default();
        comp.update(&Action::CpuUpdate(CpuSnapshot::stub()))
            .unwrap();
        comp.update(&Action::ToggleFullScreen).unwrap();
        // In fullscreen: scroll down and filter
        comp.handle_key_event(crossterm::event::KeyEvent::new(
            KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        ))
        .unwrap();
        comp.filter = "1".to_string();
        // Exit fullscreen
        comp.update(&Action::ToggleFullScreen).unwrap();
        assert_eq!(
            comp.scroll_offset, 0,
            "scroll must be restored to pre-fullscreen value"
        );
        assert_eq!(
            comp.filter, "",
            "filter must be restored to pre-fullscreen value"
        );
        assert!(!comp.is_fullscreen);
    }

    /// Compact background pass renders without the fullscreen header, even when
    /// `is_fullscreen` is true.  The header is an overlay-only feature; it must
    /// not bleed into the compact sidebar that is rendered behind the modal.
    #[test]
    fn compact_background_hides_fullscreen_header() {
        let mut comp = CpuComponent::default();
        comp.update(&Action::CpuUpdate(CpuSnapshot::stub()))
            .unwrap();
        comp.update(&Action::ToggleFullScreen).unwrap();

        // Compact background pass (no begin_overlay_render): must not show header.
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        let compact_bg = format!("{:?}", terminal.backend());
        assert!(
            !compact_bg.contains("Brand:"),
            "compact background must NOT show CPU brand header; got: {compact_bg}"
        );
        assert!(
            !compact_bg.contains("Freq:"),
            "compact background must NOT show frequency header; got: {compact_bg}"
        );

        // Overlay pass (begin_overlay_render + tall area): must show the header.
        comp.begin_overlay_render();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        let overlay = format!("{:?}", terminal.backend());
        assert!(
            overlay.contains("Brand:"),
            "overlay pass must show CPU brand header; got: {overlay}"
        );
    }

    /// Compact background pass renders frozen filter state; the overlay pass
    /// renders the live filter being typed by the user.
    #[test]
    fn compact_background_shows_frozen_filter_during_fullscreen() {
        let key = |c| {
            crossterm::event::KeyEvent::new(KeyCode::Char(c), crossterm::event::KeyModifiers::NONE)
        };
        let mut comp = CpuComponent::default();
        comp.update(&Action::CpuUpdate(CpuSnapshot::stub()))
            .unwrap();
        comp.update(&Action::ToggleFullScreen).unwrap();

        // In fullscreen: enter filter mode and type "0".
        comp.handle_key_event(key('/')).unwrap();
        comp.handle_key_event(key('0')).unwrap();
        assert_eq!(comp.filter, "0");

        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();

        // Compact background pass: title must NOT show the live filter.
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        let compact_bg = format!("{:?}", terminal.backend());
        assert!(
            !compact_bg.contains("/0"),
            "compact background must not show live filter '/0'; got: {compact_bg}"
        );

        // Overlay pass: title MUST show the live filter.
        comp.begin_overlay_render();
        terminal.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
        let overlay = format!("{:?}", terminal.backend());
        assert!(
            overlay.contains("/0"),
            "overlay pass must show live filter '/0'; got: {overlay}"
        );
    }
}
