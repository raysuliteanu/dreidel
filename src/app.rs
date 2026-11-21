// SPDX-License-Identifier: GPL-3.0-only

//! Top-level application struct and event loop.
//!
//! [`App`] owns the action channel, all component instances, focus/fullscreen
//! state, and the layout engine. Its [`run`](App::run) method is the main
//! async loop: read terminal events → dispatch actions → render.

use anyhow::{Context, Result};
use crossterm::event::KeyEvent;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    widgets::{Clear, Paragraph},
};
use std::str::FromStr;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::{
    action::Action,
    components::help::HelpComponent,
    components::status_bar::StatusBarComponent,
    components::{Component, ComponentId},
    config::Config,
    layout::{
        LayoutHints, LayoutPreset, SlotOverrides, StatusBarPosition, fit_layout, split_status_bar,
    },
    stats::spawn_collector,
    theme::ColorPalette,
    tui::{Event, Tui},
};

/// Smallest terminal (width, height) dreidel renders into; below this only a
/// "terminal too small" message is drawn.
const MIN_TERMINAL: (u16, u16) = (40, 10);

#[derive(Debug, Clone, PartialEq)]
pub enum FocusState {
    Normal { focused: ComponentId },
    FullScreen(ComponentId),
}

/// Returns a rect centered in `area` at `pct_w`% width and `pct_h`% height.
fn centered_pct(area: Rect, pct_w: u16, pct_h: u16) -> Rect {
    let w = (area.width * pct_w / 100).max(1);
    let h = (area.height * pct_h / 100).max(1);
    let cols = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(w),
        Constraint::Fill(1),
    ])
    .split(area);
    Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(h),
        Constraint::Fill(1),
    ])
    .split(cols[1])[1]
}

pub struct App {
    config: Config,
    action_tx: mpsc::Sender<Action>,
    action_rx: mpsc::Receiver<Action>,
    components: Vec<(ComponentId, Box<dyn Component>)>,
    status_bar: StatusBarComponent,
    help_comp: Box<dyn Component>,
    focus: FocusState,
    show_help: bool,
    loading: bool,
    should_quit: bool,
    should_suspend: bool,
    preset: LayoutPreset,
    slot_overrides: SlotOverrides,
    status_pos: StatusBarPosition,
    visible: Vec<ComponentId>,
    /// Ordered list of component IDs placed by [`fit_layout`] in the last
    /// render (in `components` order). Used to restrict Tab cycling to
    /// components the user can actually see.
    rendered_ids: Vec<ComponentId>,
    /// Visible components dropped by [`fit_layout`] because the terminal is too
    /// small for them. Their focus keys still work: focusing one brings it back.
    dropped_ids: Vec<ComponentId>,
    palette: ColorPalette,
}

impl App {
    pub fn new(config: Config, detected_theme: Option<crate::theme::Theme>) -> Result<Self> {
        let (action_tx, action_rx) = mpsc::channel(config.general.channel_capacity);
        let palette = config.general.theme.palette();

        let kb = &config.keybindings;
        let components: Vec<(ComponentId, Box<dyn Component>)> = vec![
            (
                ComponentId::Cpu,
                Box::new(crate::components::cpu::CpuComponent::new(
                    palette.clone(),
                    kb.focus_cpu,
                )),
            ),
            (
                ComponentId::Net,
                Box::new(crate::components::net::NetComponent::new(
                    palette.clone(),
                    kb.focus_net,
                )),
            ),
            (
                ComponentId::Disk,
                Box::new(crate::components::disk::DiskComponent::new(
                    palette.clone(),
                    kb.focus_disk,
                )),
            ),
            (
                ComponentId::Process,
                Box::new(crate::components::process::ProcessComponent::new(
                    palette.clone(),
                    kb.focus_proc,
                    &config.process,
                )),
            ),
        ];

        let mut visible: Vec<ComponentId> = Vec::new();
        for s in &config.layout.show {
            let id = ComponentId::from_str(s).map_err(|_| {
                anyhow::anyhow!(
                    "unknown component {s:?} in show list; valid values are: cpu, net, disk, process"
                )
            })?;
            // `fit_layout` expects distinct panel ids; the status bar is not a panel.
            if id != ComponentId::StatusBar && !visible.contains(&id) {
                visible.push(id);
            }
        }

        let preset = LayoutPreset::from_str(&config.layout.preset).unwrap_or_else(|_| {
            warn!(value = %config.layout.preset, "unknown layout preset; using default (sidebar)");
            LayoutPreset::default()
        });
        let status_pos =
            StatusBarPosition::from_str(&config.layout.status_bar).unwrap_or_else(|_| {
                warn!(
                    value = %config.layout.status_bar,
                    "unknown status_bar position; using top"
                );
                StatusBarPosition::default()
            });

        Ok(Self {
            action_tx,
            action_rx,
            status_bar: StatusBarComponent::new(palette.clone()),
            help_comp: Box::new(HelpComponent::new(
                palette.clone(),
                config.keybindings.clone(),
                detected_theme,
                config.general.theme,
            )),
            // Default focus: process panel when all components are present,
            // otherwise the first listed component so the user always starts
            // with something focused (e.g. `--show net` focuses net).
            focus: FocusState::Normal {
                focused: if visible.len() == 4 {
                    ComponentId::Process
                } else {
                    visible.first().copied().unwrap_or(ComponentId::Process)
                },
            },
            show_help: false,
            loading: true,
            should_quit: false,
            should_suspend: false,
            preset,
            slot_overrides: SlotOverrides::default(),
            status_pos,
            visible,
            rendered_ids: Vec::new(),
            dropped_ids: Vec::new(),
            palette,
            config,
            components,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        // There is no render timer: the UI redraws when a collector batch
        // arrives (see `ProcUpdate` in handle_actions), on key presses, and on
        // resize, so frames are never redundant and new data is drawn at once.
        let mut tui = Tui::new()?.mouse(true);
        tui.enter().context("entering TUI")?;

        let collector_token = tokio_util::sync::CancellationToken::new();
        spawn_collector(
            self.action_tx.clone(),
            collector_token.child_token(),
            self.config.general.refresh_rate_ms,
            self.config.general.thread_refresh_ms,
        );

        loop {
            // Wake on terminal input or collector data, whichever comes first;
            // both receivers are cancel-safe.
            let (event, first_action) = tokio::select! {
                event = tui.next_event() => (event, None),
                action = self.action_rx.recv() => (None, action),
            };
            if let Some(event) = event {
                self.handle_event(event).context("handling event")?;
            }
            self.handle_actions(&mut tui, first_action)
                .context("handling actions")?;

            if self.should_suspend {
                tui.exit().context("suspending TUI")?;
                // Resume on next iteration — cleared by Resume action
                self.should_suspend = false;
            } else if self.should_quit {
                collector_token.cancel();
                tui.exit().context("exiting TUI")?;
                break;
            }
        }
        Ok(())
    }

    fn handle_event(&mut self, event: Event) -> Result<()> {
        // When the help overlay is visible, only Esc/? /h close it; all other
        // keys are swallowed so nothing behind the overlay reacts. Data-driven
        // renders still happen, so the dashboard keeps refreshing behind it.
        if self.show_help {
            if let Event::Key(key) = &event {
                use crossterm::event::KeyCode;
                let is_close = key.code == KeyCode::Esc
                    || key.code == KeyCode::Char(self.config.keybindings.help)
                    || key.code == KeyCode::Char('h')
                    || key.code == KeyCode::Char('H')
                    || key.code == KeyCode::Char('q')
                    || key.code == KeyCode::Char('Q');
                if is_close {
                    let _ = self.action_tx.try_send(Action::ToggleHelp);
                }
            }
            return Ok(());
        }

        let tx = &self.action_tx;
        match &event {
            Event::Quit => {
                let _ = tx.try_send(Action::Quit);
            }
            // First frame (loading overlay) before any data arrives.
            Event::Init => {
                let _ = tx.try_send(Action::Render);
            }
            Event::Resize(x, y) => {
                let _ = tx.try_send(Action::Resize(*x, *y));
            }
            Event::Key(key) => {
                // Focused component gets first right of refusal on key events.
                // Critical for Esc/q which mean different things inside vs outside
                // component sub-states (e.g., q in DetailView ≠ global quit).
                let focused_id = match &self.focus {
                    FocusState::Normal { focused } | FocusState::FullScreen(focused) => *focused,
                };
                let consumed = self
                    .components
                    .iter_mut()
                    .find(|(id, _)| *id == focused_id)
                    .and_then(|(_, comp)| match comp.handle_key_event(*key) {
                        Ok(action) => action,
                        Err(e) => {
                            warn!(component = ?focused_id, error = %e, "key handler error");
                            None
                        }
                    });

                if let Some(action) = consumed {
                    let _ = self.action_tx.try_send(action);
                } else {
                    self.handle_key_event(*key)?;
                }
                // Render immediately on any key press so focus changes and
                // other state updates are visible without waiting for the
                // next periodic Render tick.
                let _ = self.action_tx.try_send(Action::Render);
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<()> {
        use crossterm::event::KeyCode;
        let kb = &self.config.keybindings;
        if let KeyCode::Char(c) = key.code {
            let c = c.to_ascii_lowercase();
            // Only focus a component if it is in the layout or was dropped for
            // lack of space (focusing a dropped panel brings it back).  Use
            // rendered_ids/dropped_ids (populated after the first render) when
            // available; fall back to visible so the very first key-press also works.
            let focusable: Vec<ComponentId> = if self.rendered_ids.is_empty() {
                self.visible.clone()
            } else {
                self.rendered_ids
                    .iter()
                    .chain(&self.dropped_ids)
                    .copied()
                    .collect()
            };
            if c == kb.focus_proc && focusable.contains(&ComponentId::Process) {
                let _ = self
                    .action_tx
                    .try_send(Action::FocusComponent(ComponentId::Process));
            } else if c == kb.focus_cpu && focusable.contains(&ComponentId::Cpu) {
                let _ = self
                    .action_tx
                    .try_send(Action::FocusComponent(ComponentId::Cpu));
            } else if c == kb.focus_net && focusable.contains(&ComponentId::Net) {
                let _ = self
                    .action_tx
                    .try_send(Action::FocusComponent(ComponentId::Net));
            } else if c == kb.focus_disk && focusable.contains(&ComponentId::Disk) {
                let _ = self
                    .action_tx
                    .try_send(Action::FocusComponent(ComponentId::Disk));
            } else if c == kb.fullscreen {
                let _ = self.action_tx.try_send(Action::ToggleFullScreen);
            } else if c == 'q' {
                match &self.focus {
                    FocusState::FullScreen(_) => {
                        let _ = self.action_tx.try_send(Action::ToggleFullScreen);
                    }
                    FocusState::Normal { .. } => {
                        let _ = self.action_tx.try_send(Action::Quit);
                    }
                }
            }
            // help key: '?' (requires Shift) or 'h'/'H'
            if key.code == KeyCode::Char(kb.help) || c == 'h' {
                let _ = self.action_tx.try_send(Action::ToggleHelp);
            }
        }
        if key.code == KeyCode::Tab || key.code == KeyCode::BackTab {
            // Only cycle focus in normal mode; fullscreen/modal state owns all keys.
            if matches!(self.focus, FocusState::Normal { .. }) {
                // Only cycle through components that have a layout slot AND are visible;
                // cycling through a component with no slot would appear to do nothing.
                let visible_ids = &self.rendered_ids;
                if !visible_ids.is_empty() {
                    let focused_id = match &self.focus {
                        FocusState::Normal { focused } => *focused,
                        FocusState::FullScreen(_) => unreachable!("guarded above"),
                    };
                    let cur = visible_ids
                        .iter()
                        .position(|id| *id == focused_id)
                        .unwrap_or(0);
                    let next = if key.code == KeyCode::Tab {
                        (cur + 1) % visible_ids.len()
                    } else {
                        (cur + visible_ids.len() - 1) % visible_ids.len()
                    };
                    let _ = self
                        .action_tx
                        .try_send(Action::FocusComponent(visible_ids[next]));
                }
            }
        }
        if key.code == KeyCode::Esc {
            match &self.focus {
                FocusState::FullScreen(_) => {
                    let _ = self.action_tx.try_send(Action::ToggleFullScreen);
                }
                FocusState::Normal { .. } => {
                    let _ = self.action_tx.try_send(Action::Quit);
                }
            }
        }
        Ok(())
    }

    /// Process `first` (an action already received by the run loop), then
    /// drain everything else queued.
    fn handle_actions(&mut self, tui: &mut Tui, first: Option<Action>) -> Result<()> {
        let mut first = first;
        while let Some(action) = first.take().or_else(|| self.action_rx.try_recv().ok()) {
            if !matches!(action, Action::Render) {
                debug!("action: {action}");
            }
            match &action {
                Action::Quit => self.should_quit = true,
                Action::Suspend => self.should_suspend = true,
                Action::Resume => self.should_suspend = false,
                Action::ClearScreen => {
                    tui.clear().context("clearing screen")?;
                }
                Action::ToggleHelp => {
                    self.show_help = !self.show_help;
                    self.render(tui).context("rendering after help toggle")?;
                }
                Action::ToggleFullScreen => {
                    self.focus = match &self.focus {
                        FocusState::Normal { focused } => FocusState::FullScreen(*focused),
                        FocusState::FullScreen(id) => FocusState::Normal { focused: *id },
                    };
                }
                Action::FocusComponent(id) => {
                    self.focus = FocusState::Normal { focused: *id };
                }
                Action::Resize(w, h) => {
                    tui.resize(Rect::new(0, 0, *w, *h)).context("resizing")?;
                    self.render(tui).context("re-rendering after resize")?;
                }
                Action::Render => {
                    let start = std::time::Instant::now();
                    self.render(tui).context("rendering")?;
                    debug!(
                        target: "dreidel::perf",
                        us = start.elapsed().as_micros() as u64,
                        "render"
                    );
                }
                Action::SysUpdate(_) => self.loading = false,
                _ => {}
            }
            // Fan out to all components
            let start = std::time::Instant::now();
            for (_, comp) in &mut self.components {
                if let Some(new_action) = comp.update(&action)? {
                    let _ = self.action_tx.try_send(new_action);
                }
            }
            self.status_bar.update(&action)?;
            debug!(
                target: "dreidel::perf",
                action = %action,
                us = start.elapsed().as_micros() as u64,
                "component update"
            );
            // ProcUpdate is the last action of each collector batch, so one
            // render here draws the complete tick.
            if matches!(action, Action::ProcUpdate(_)) {
                let _ = self.action_tx.try_send(Action::Render);
            }
        }
        Ok(())
    }

    fn render(&mut self, tui: &mut Tui) -> Result<()> {
        self.render_to(&mut *tui)
    }

    /// Inner render that accepts any ratatui backend; used directly in tests via TestBackend.
    fn render_to<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut ratatui::Terminal<B>,
    ) -> Result<()>
    where
        B::Error: Send + Sync + 'static,
    {
        let focused_id = match &self.focus {
            FocusState::Normal { focused } | FocusState::FullScreen(focused) => *focused,
        };
        for (id, comp) in &mut self.components {
            comp.set_focused(*id == focused_id);
        }

        let size = terminal.size().context("reading terminal size")?;
        let total_area = Rect::new(0, 0, size.width, size.height);
        if total_area.width < MIN_TERMINAL.0 || total_area.height < MIN_TERMINAL.1 {
            terminal
                .draw(|frame| {
                    let row = Layout::vertical([
                        Constraint::Fill(1),
                        Constraint::Length(1),
                        Constraint::Fill(1),
                    ])
                    .split(frame.area())[1];
                    frame.render_widget(
                        Paragraph::new("terminal too small (need 40×10)").centered(),
                        row,
                    );
                })
                .context("drawing too-small message")?;
            return Ok(());
        }

        let focus = self.focus.clone();
        let show_help = self.show_help;
        let loading = self.loading;
        let palette = self.palette.clone();
        let cpu_height = self
            .components
            .iter()
            .find(|(id, _)| *id == ComponentId::Cpu)
            .and_then(|(_, c)| c.preferred_height());
        let hints = LayoutHints {
            left_top: cpu_height,
            right_top: cpu_height,
        };

        // Hide the status bar when it would leave fewer than MIN_TERMINAL rows
        // for the panels.
        let status_height = self.status_bar.preferred_height().unwrap_or(6);
        let status_pos = if total_area.height.saturating_sub(status_height) >= MIN_TERMINAL.1 {
            self.status_pos
        } else {
            StatusBarPosition::Hidden
        };
        let (status_rect, main_area) = split_status_bar(total_area, status_pos, status_height);

        let fit = fit_layout(
            self.preset,
            main_area,
            &self.slot_overrides,
            &hints,
            &self.visible,
            focused_id,
        );
        // Tab cycling visits only components the user can see.
        self.rendered_ids = self
            .components
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| fit.placed.iter().any(|(pid, _)| pid == id))
            .collect();
        self.dropped_ids = fit.dropped;
        let kb = &self.config.keybindings;
        let dropped_hints = self
            .dropped_ids
            .iter()
            .filter_map(|id| {
                let key = match id {
                    ComponentId::Process => kb.focus_proc,
                    ComponentId::Cpu => kb.focus_cpu,
                    ComponentId::Net => kb.focus_net,
                    ComponentId::Disk => kb.focus_disk,
                    ComponentId::StatusBar => return None,
                };
                Some((key, *id))
            })
            .collect();
        self.status_bar.set_dropped(dropped_hints);

        terminal.draw(|frame| {
            if status_pos != StatusBarPosition::Hidden
                && let Err(e) = self.status_bar.draw(frame, status_rect)
            {
                warn!(error = %e, "status bar draw failed");
            }

            // Always draw the normal layout first so it remains visible behind
            // any overlay (fullscreen modal, help, loading).
            for (component_id, rect) in &fit.placed {
                if let Some((_, comp)) = self
                    .components
                    .iter_mut()
                    .find(|(cid, _)| cid == component_id)
                    && let Err(e) = comp.draw(frame, *rect)
                {
                    warn!(component = ?component_id, error = %e, "component draw failed");
                }
            }

            // Fullscreen overlay: drawn on top of the normal layout like a modal.
            if let FocusState::FullScreen(id) = &focus
                && let Some((component_id, comp)) =
                    self.components.iter_mut().find(|(cid, _)| cid == id)
            {
                let modal = centered_pct(main_area, 95, 95);
                // Signal to the component that the next draw() call is the overlay
                // pass, not the compact background pass.  The component uses this
                // one-shot flag to render live state rather than frozen snapshot.
                comp.begin_overlay_render();
                frame.render_widget(Clear, modal);
                if let Err(e) = comp.draw(frame, modal) {
                    warn!(component = ?component_id, error = %e, "component draw failed");
                }
            }

            // Help overlay is drawn last so it appears on top of everything else.
            if show_help && let Err(e) = self.help_comp.draw(frame, total_area) {
                warn!(error = %e, "help overlay draw failed");
            }

            // Loading overlay: shown until the first SysUpdate arrives.
            // Drawn after help so it covers both (help won't be open on startup).
            if loading {
                use ratatui::{
                    layout::{Constraint, Layout},
                    style::{Modifier, Style},
                    text::{Line, Span},
                    widgets::{Block, BorderType, Borders, Clear, Paragraph},
                };
                const W: u16 = 26;
                const H: u16 = 3;
                let cols = Layout::horizontal([
                    Constraint::Fill(1),
                    Constraint::Length(W.min(total_area.width)),
                    Constraint::Fill(1),
                ])
                .split(total_area);
                let popup = Layout::vertical([
                    Constraint::Fill(1),
                    Constraint::Length(H.min(total_area.height)),
                    Constraint::Fill(1),
                ])
                .split(cols[1])[1];
                frame.render_widget(Clear, popup);
                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(palette.accent));
                let inner = block.inner(popup);
                frame.render_widget(block, popup);
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        "Loading stats...",
                        Style::new().fg(palette.fg).add_modifier(Modifier::BOLD),
                    )))
                    .centered(),
                    inner,
                );
            }
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};

    use crate::stats::snapshots::{
        CpuSnapshot, DiskSnapshot, MemSnapshot, NetSnapshot, ProcSnapshot, SysSnapshot,
    };

    fn make_app() -> App {
        App::new(Config::default(), None).expect("app construction should not fail")
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    /// Drain all pending actions from the channel and return them.
    fn drain(app: &mut App) -> Vec<Action> {
        let mut actions = Vec::new();
        while let Ok(a) = app.action_rx.try_recv() {
            actions.push(a);
        }
        actions
    }

    /// Feed stub data to all components and clear the loading overlay so that
    /// render tests see real component output rather than the loading spinner.
    fn feed_stubs(app: &mut App) {
        for (_, comp) in &mut app.components {
            let _ = comp.update(&Action::CpuUpdate(CpuSnapshot::stub()));
            let _ = comp.update(&Action::MemUpdate(MemSnapshot::stub()));
            let _ = comp.update(&Action::NetUpdate(NetSnapshot::stub()));
            let _ = comp.update(&Action::DiskUpdate(DiskSnapshot::stub()));
            let _ = comp.update(&Action::ProcUpdate(ProcSnapshot::stub().into()));
            let _ = comp.update(&Action::SysUpdate(SysSnapshot::stub()));
        }
        app.loading = false;
    }

    /// Returns true if any cell in `buf` within the given rect has a non-blank symbol.
    fn has_content(buf: &ratatui::buffer::Buffer, rect: Rect) -> bool {
        for y in rect.top()..rect.bottom() {
            for x in rect.left()..rect.right() {
                if buf[(x, y)].symbol() != " " {
                    return true;
                }
            }
        }
        false
    }

    /// Returns a rect that is strictly outside the modal region — the thin strip
    /// on the left side of the screen.  With the sidebar preset, CPU occupies
    /// the left column, so this area is always rendered in every mode.
    fn outside_modal_strip(terminal_area: Rect) -> Rect {
        // centered_pct at 95% leaves 2.5% on each side; on a 200-col terminal
        // that is a 5-col margin, so the leftmost 3 cols are always outside.
        Rect::new(
            terminal_area.x,
            terminal_area.y + 1,
            3,
            terminal_area.height - 1,
        )
    }

    #[test]
    fn q_in_normal_mode_sends_quit() {
        let mut app = make_app();
        app.focus = FocusState::Normal {
            focused: ComponentId::Process,
        };
        app.handle_key_event(key('q')).unwrap();
        let actions = drain(&mut app);
        assert!(
            actions.iter().any(|a| matches!(a, Action::Quit)),
            "expected Quit, got {actions:?}"
        );
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, Action::ToggleFullScreen)),
            "should not toggle fullscreen in normal mode"
        );
    }

    #[test]
    fn q_in_fullscreen_mode_sends_toggle_fullscreen() {
        let mut app = make_app();
        app.focus = FocusState::FullScreen(ComponentId::Cpu);
        app.handle_key_event(key('q')).unwrap();
        let actions = drain(&mut app);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::ToggleFullScreen)),
            "expected ToggleFullScreen, got {actions:?}"
        );
        assert!(
            !actions.iter().any(|a| matches!(a, Action::Quit)),
            "should not quit when in fullscreen mode"
        );
    }

    #[test]
    fn uppercase_q_in_fullscreen_mode_sends_toggle_fullscreen() {
        let mut app = make_app();
        app.focus = FocusState::FullScreen(ComponentId::Cpu);
        // handle_key_event lowercases the char, so 'Q' == 'q'
        app.handle_key_event(key('Q')).unwrap();
        let actions = drain(&mut app);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::ToggleFullScreen)),
            "expected ToggleFullScreen for 'Q', got {actions:?}"
        );
        assert!(!actions.iter().any(|a| matches!(a, Action::Quit)));
    }

    #[test]
    fn esc_in_fullscreen_mode_sends_toggle_fullscreen() {
        let mut app = make_app();
        app.focus = FocusState::FullScreen(ComponentId::Cpu);
        app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        let actions = drain(&mut app);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::ToggleFullScreen)),
            "expected ToggleFullScreen on Esc in fullscreen, got {actions:?}"
        );
        assert!(
            !actions.iter().any(|a| matches!(a, Action::Quit)),
            "should not quit when Esc pressed in fullscreen mode"
        );
    }

    // --- Render tests -------------------------------------------------------

    /// In fullscreen mode the modal overlay is drawn on top of the normal
    /// layout.  The background components must still be visible — cells outside
    /// the 95% modal rect must contain content from the normal layout.
    #[test]
    fn fullscreen_renders_normal_layout_behind_modal() {
        let mut app = make_app();
        feed_stubs(&mut app);
        app.focus = FocusState::FullScreen(ComponentId::Process);

        let mut terminal = Terminal::new(TestBackend::new(200, 50)).unwrap();
        app.render_to(&mut terminal).unwrap();

        let buf = terminal.backend().buffer().clone();
        let terminal_area = Rect::new(0, 0, 200, 50);
        let strip = outside_modal_strip(terminal_area);

        assert!(
            has_content(&buf, strip),
            "cells outside the fullscreen modal must contain background layout content"
        );
    }

    /// In normal mode every component slot is drawn and no modal overlay exists,
    /// so the full content area should have rendered content.
    #[test]
    fn normal_mode_renders_full_layout() {
        let mut app = make_app();
        feed_stubs(&mut app);
        app.focus = FocusState::Normal {
            focused: ComponentId::Process,
        };

        let mut terminal = Terminal::new(TestBackend::new(200, 50)).unwrap();
        app.render_to(&mut terminal).unwrap();

        let buf = terminal.backend().buffer().clone();
        // The left column of the sidebar layout should have content.
        let left_col = Rect::new(0, 1, 3, 49);
        assert!(
            has_content(&buf, left_col),
            "left column must have content in normal mode"
        );
        // The right column (process panel) should also have content.
        let right_col = Rect::new(197, 1, 3, 49);
        assert!(
            has_content(&buf, right_col),
            "right column must have content in normal mode"
        );
    }

    /// Switching from fullscreen back to normal mode must restore full-layout
    /// rendering (no stale modal geometry in subsequent renders).
    #[test]
    fn app_new_rejects_invalid_component_in_show_list() {
        let mut cfg = Config::default();
        cfg.layout.show = vec!["cpu".into(), "foo".into()];
        assert!(App::new(cfg, None).is_err());
    }

    /// With a single visible component the adaptive layout should fill the
    /// entire content area — not just the narrow sidebar slot it would occupy
    /// under the default Sidebar preset.
    #[test]
    fn adaptive_single_component_fills_content_area() {
        let mut cfg = Config::default();
        cfg.layout.show = vec!["net".into()];
        let mut app = App::new(cfg, None).unwrap();
        feed_stubs(&mut app);

        let width = 160u16;
        let height = 40u16;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        app.render_to(&mut terminal).unwrap();

        // The status bar occupies the top 4 rows; the remainder is the content
        // area.  The net component should render content across the full width
        // of the content area, not just a narrow column on the left.
        let buf = terminal.backend().buffer().clone();
        let right_strip = Rect::new(width / 2, 4, width / 2 - 1, height - 4);
        assert!(
            has_content(&buf, right_strip),
            "right half of content area should be rendered in single-component adaptive mode"
        );
    }

    /// When fewer than four components are shown the first listed component
    /// should receive initial focus so the user never starts unfocused.
    #[test]
    fn initial_focus_is_first_visible_component_when_not_all_shown() {
        let mut cfg = Config::default();
        cfg.layout.show = vec!["net".into(), "disk".into()];
        let app = App::new(cfg, None).expect("app");
        assert!(
            matches!(
                app.focus,
                FocusState::Normal {
                    focused: ComponentId::Net
                }
            ),
            "expected Net to have initial focus, got {:?}",
            app.focus,
        );
    }

    /// With all four components the default initial focus must remain Process.
    #[test]
    fn initial_focus_is_process_when_all_components_shown() {
        let app = make_app();
        assert!(
            matches!(
                app.focus,
                FocusState::Normal {
                    focused: ComponentId::Process
                }
            ),
            "expected Process to have initial focus, got {:?}",
            app.focus,
        );
    }

    /// Focus shortcut keys for hidden components must be no-ops — pressing 'p'
    /// when only the disk panel is shown should not steal focus from disk.
    #[test]
    fn focus_key_ignored_for_hidden_component() {
        let mut cfg = Config::default();
        cfg.layout.show = vec!["disk".into()];
        let mut app = App::new(cfg, None).expect("app");

        // Trigger a render so rendered_ids is populated.
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        app.render_to(&mut terminal).unwrap();
        drain(&mut app); // clear any queued actions

        // Press the process focus key — should produce no FocusComponent action.
        app.handle_key_event(KeyEvent::new(
            KeyCode::Char(app.config.keybindings.focus_proc),
            KeyModifiers::NONE,
        ))
        .unwrap();
        let actions = drain(&mut app);
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, Action::FocusComponent(_))),
            "pressing a focus key for a hidden component must not change focus; got {actions:?}"
        );
    }

    /// Collect a buffer row as a string.
    fn row_text(buf: &ratatui::buffer::Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
    }

    fn buffer_text(buf: &ratatui::buffer::Buffer) -> String {
        (0..buf.area.height)
            .map(|y| row_text(buf, y))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn small_terminal_drops_low_priority_panels() {
        let mut app = make_app();
        feed_stubs(&mut app);
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        app.render_to(&mut terminal).unwrap();

        assert!(
            app.dropped_ids.contains(&ComponentId::Disk),
            "Disk should be dropped at 60x20; dropped = {:?}",
            app.dropped_ids
        );
        assert!(!app.rendered_ids.contains(&ComponentId::Disk));
        assert!(app.rendered_ids.contains(&ComponentId::Process));
    }

    #[test]
    fn focus_key_for_dropped_panel_brings_it_back() {
        let mut app = make_app();
        feed_stubs(&mut app);
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        app.render_to(&mut terminal).unwrap();
        assert!(app.dropped_ids.contains(&ComponentId::Disk));
        drain(&mut app);

        app.handle_key_event(key(app.config.keybindings.focus_disk))
            .unwrap();
        let actions = drain(&mut app);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::FocusComponent(ComponentId::Disk))),
            "focus key for a dropped panel must focus it; got {actions:?}"
        );

        app.focus = FocusState::Normal {
            focused: ComponentId::Disk,
        };
        app.render_to(&mut terminal).unwrap();
        assert!(app.rendered_ids.contains(&ComponentId::Disk));
        assert!(
            !app.dropped_ids.is_empty() && !app.dropped_ids.contains(&ComponentId::Disk),
            "another panel should be dropped instead; dropped = {:?}",
            app.dropped_ids
        );
    }

    #[test]
    fn tab_skips_dropped_panels() {
        let mut app = make_app();
        feed_stubs(&mut app);
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        app.render_to(&mut terminal).unwrap();
        assert!(!app.dropped_ids.is_empty());
        drain(&mut app);

        for focused in app.rendered_ids.clone() {
            app.focus = FocusState::Normal { focused };
            app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
                .unwrap();
            for action in drain(&mut app) {
                if let Action::FocusComponent(id) = action {
                    assert!(
                        !app.dropped_ids.contains(&id),
                        "Tab from {focused:?} focused dropped panel {id:?}"
                    );
                }
            }
        }
    }

    /// Render every preset across a range of terminal sizes. The status bar is
    /// hidden (it is platform-dependent and covered by its own tests) and the
    /// platform-neutral `stub()` data is used so snapshots match on any OS.
    fn render_matrix(preset: LayoutPreset) {
        for (w, h) in [(200, 50), (120, 40), (80, 24), (60, 20), (40, 12), (30, 8)] {
            let mut app = make_app();
            app.preset = preset;
            app.status_pos = StatusBarPosition::Hidden;
            feed_stubs(&mut app);
            let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("terminal");
            app.render_to(&mut terminal).expect("render");
            assert_snapshot!(format!("{preset}_{w}x{h}"), terminal.backend());
        }
    }

    #[test]
    fn size_matrix_sidebar() {
        render_matrix(LayoutPreset::Sidebar);
    }

    #[test]
    fn size_matrix_classic() {
        render_matrix(LayoutPreset::Classic);
    }

    #[test]
    fn size_matrix_dashboard() {
        render_matrix(LayoutPreset::Dashboard);
    }

    #[test]
    fn size_matrix_grid() {
        render_matrix(LayoutPreset::Grid);
    }

    #[test]
    fn too_small_terminal_shows_message() {
        let mut app = make_app();
        feed_stubs(&mut app);
        let mut terminal = Terminal::new(TestBackend::new(39, 10)).unwrap();
        app.render_to(&mut terminal).unwrap();

        let text = buffer_text(terminal.backend().buffer());
        assert!(
            text.contains("terminal too small (need 40×10)"),
            "expected too-small message, got:\n{text}"
        );
    }

    #[test]
    fn status_bar_hidden_when_content_would_be_under_10_rows() {
        let mut app = make_app();
        feed_stubs(&mut app);
        app.status_bar
            .update(&Action::SysUpdate(SysSnapshot::stub()))
            .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(80, 15)).unwrap();
        app.render_to(&mut terminal).unwrap();

        let hostname = SysSnapshot::stub().hostname;
        let row0 = row_text(terminal.backend().buffer(), 0);
        assert!(
            !row0.contains(&hostname),
            "status bar must be hidden at 80x15; row 0 = {row0:?}"
        );
    }

    #[test]
    fn fullscreen_with_dropped_panels_renders_focused_modal() {
        let mut app = make_app();
        feed_stubs(&mut app);
        app.focus = FocusState::Normal {
            focused: ComponentId::Disk,
        };
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        app.render_to(&mut terminal).unwrap();
        assert!(!app.dropped_ids.is_empty());

        app.focus = FocusState::FullScreen(ComponentId::Disk);
        app.render_to(&mut terminal).unwrap();

        // Status bar (no mem data → 6 rows) sits on top; the modal is centered
        // in the remaining content area.
        let status_height = app.status_bar.preferred_height().unwrap_or(6);
        let content = Rect::new(0, status_height, 60, 20 - status_height);
        let modal = centered_pct(content, 95, 95);
        let buf = terminal.backend().buffer().clone();
        assert!(has_content(&buf, modal));
        let top = row_text(&buf, modal.y);
        assert!(
            top.contains("[D]"),
            "modal top border should carry the Disk title; got {top:?}"
        );
    }

    #[test]
    fn toggle_back_to_normal_renders_full_layout() {
        let mut app = make_app();
        feed_stubs(&mut app);

        // Enter fullscreen then immediately return to normal.
        app.focus = FocusState::FullScreen(ComponentId::Process);
        app.focus = FocusState::Normal {
            focused: ComponentId::Process,
        };

        let mut terminal = Terminal::new(TestBackend::new(200, 50)).unwrap();
        app.render_to(&mut terminal).unwrap();

        let buf = terminal.backend().buffer().clone();
        let left_col = Rect::new(0, 1, 3, 49);
        assert!(
            has_content(&buf, left_col),
            "left column must have content after returning to normal mode"
        );
    }
}
