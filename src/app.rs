use anyhow::{Context, Result};
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use std::{collections::HashSet, str::FromStr};
use tokio::sync::mpsc;
use tracing::debug;

use crate::{
    action::Action,
    components::{Component, ComponentId},
    components::help::HelpComponent,
    config::Config,
    layout::{LayoutHints, LayoutPreset, SlotOverrides, StatusBarPosition, split_status_bar},
    stats::spawn_collector,
    tui::{Event, Tui},
};

#[derive(Debug, Clone, PartialEq)]
pub enum FocusState {
    Normal { focused: ComponentId },
    FullScreen(ComponentId),
}

pub struct App {
    config: Config,
    action_tx: mpsc::Sender<Action>,
    action_rx: mpsc::Receiver<Action>,
    components: Vec<(ComponentId, Box<dyn Component>)>,
    status_bar: Box<dyn Component>,
    debug_comp: Box<dyn Component>,
    help_comp: Box<dyn Component>,
    focus: FocusState,
    show_debug: bool,
    show_help: bool,
    should_quit: bool,
    should_suspend: bool,
    preset: LayoutPreset,
    slot_overrides: SlotOverrides,
    status_pos: StatusBarPosition,
    visible: HashSet<ComponentId>,
    /// Ordered list of component IDs that currently have a layout slot AND are
    /// in `visible`. Kept in sync each render; used to restrict Tab cycling to
    /// components the user can actually see.
    rendered_ids: Vec<ComponentId>,
}

impl App {
    pub fn new(config: Config, show_debug: bool) -> Result<Self> {
        let (action_tx, action_rx) = mpsc::channel(config.general.channel_capacity);
        let palette = config.general.theme.palette();

        let components: Vec<(ComponentId, Box<dyn Component>)> = vec![
            (
                ComponentId::Cpu,
                Box::new(crate::components::cpu::CpuComponent::new(palette.clone())),
            ),
            (
                ComponentId::Mem,
                Box::new(crate::components::mem::MemComponent::new(palette.clone())),
            ),
            (
                ComponentId::Net,
                Box::new(crate::components::net::NetComponent::new(palette.clone())),
            ),
            (
                ComponentId::Disk,
                Box::new(crate::components::disk::DiskComponent::new(palette.clone())),
            ),
            (
                ComponentId::Process,
                Box::new(crate::components::process::ProcessComponent::new(
                    palette.clone(),
                    &config.process,
                )),
            ),
        ];

        let visible = config
            .layout
            .show
            .iter()
            .filter_map(|s| ComponentId::from_str(s).ok())
            .collect();

        let preset = LayoutPreset::from_str(&config.layout.preset).unwrap_or_default();
        let status_pos = match config.layout.status_bar.as_str() {
            "bottom" => StatusBarPosition::Bottom,
            "hidden" => StatusBarPosition::Hidden,
            _ => StatusBarPosition::Top,
        };

        Ok(Self {
            action_tx,
            action_rx,
            status_bar: Box::new(crate::components::status_bar::StatusBarComponent::new(
                palette.clone(),
            )),
            debug_comp: Box::new(crate::components::debug::DebugComponent::new(
                palette.clone(),
            )),
            help_comp: Box::new(HelpComponent::new(
                palette.clone(),
                config.keybindings.clone(),
            )),
            focus: FocusState::Normal {
                focused: ComponentId::Process,
            },
            show_debug,
            show_help: false,
            should_quit: false,
            should_suspend: false,
            preset,
            slot_overrides: SlotOverrides::default(),
            status_pos,
            visible,
            rendered_ids: Vec::new(),
            config,
            components,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut tui = Tui::new()?.mouse(true);
        tui.enter().context("entering TUI")?;

        let size = tui.size().context("getting terminal size")?;
        // Initialize components with terminal size so they can pre-allocate layout state.
        // Components that need size hints implement init() implicitly via draw(); no-op here.
        let _ = size;

        let collector_token = tokio_util::sync::CancellationToken::new();
        spawn_collector(
            self.action_tx.clone(),
            collector_token.child_token(),
            self.config.general.refresh_rate_ms,
        );

        loop {
            self.handle_events(&mut tui)
                .await
                .context("handling events")?;
            self.handle_actions(&mut tui).context("handling actions")?;

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

    async fn handle_events(&mut self, tui: &mut Tui) -> Result<()> {
        let Some(event) = tui.next_event().await else {
            return Ok(());
        };

        // When the help overlay is visible, only Esc and ? close it; all other
        // keys are swallowed so nothing behind the overlay reacts.
        if self.show_help {
            if let Event::Key(key) = &event {
                use crossterm::event::KeyCode;
                let is_close = key.code == KeyCode::Esc
                    || key.code == KeyCode::Char(self.config.keybindings.help);
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
            Event::Tick => {
                let _ = tx.try_send(Action::Tick);
            }
            Event::Render => {
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
                    .and_then(|(_, comp)| comp.handle_key_event(*key).ok().flatten());

                if let Some(action) = consumed {
                    let _ = self.action_tx.try_send(action);
                } else {
                    self.handle_key_event(*key)?;
                }
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
            if c == kb.focus_proc {
                let _ = self
                    .action_tx
                    .try_send(Action::FocusComponent(ComponentId::Process));
            } else if c == kb.focus_cpu {
                let _ = self
                    .action_tx
                    .try_send(Action::FocusComponent(ComponentId::Cpu));
            } else if c == kb.focus_mem {
                let _ = self
                    .action_tx
                    .try_send(Action::FocusComponent(ComponentId::Mem));
            } else if c == kb.focus_net {
                let _ = self
                    .action_tx
                    .try_send(Action::FocusComponent(ComponentId::Net));
            } else if c == kb.focus_disk {
                let _ = self
                    .action_tx
                    .try_send(Action::FocusComponent(ComponentId::Disk));
            } else if c == kb.fullscreen {
                let _ = self.action_tx.try_send(Action::ToggleFullScreen);
            } else if c == kb.debug {
                let _ = self.action_tx.try_send(Action::ToggleDebug);
            } else if c == 'q' {
                let _ = self.action_tx.try_send(Action::Quit);
            }
            // help key uses the raw char (not lowercased) since '?' requires Shift
            if key.code == KeyCode::Char(kb.help) {
                let _ = self.action_tx.try_send(Action::ToggleHelp);
            }
        }
        if key.code == KeyCode::Tab || key.code == KeyCode::BackTab {
            // Only cycle through components that have a layout slot AND are visible;
            // cycling through a component with no slot would appear to do nothing.
            let visible_ids = &self.rendered_ids;
            if !visible_ids.is_empty() {
                let focused_id = match &self.focus {
                    FocusState::Normal { focused } | FocusState::FullScreen(focused) => *focused,
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

    fn handle_actions(&mut self, tui: &mut Tui) -> Result<()> {
        while let Ok(action) = self.action_rx.try_recv() {
            if !matches!(action, Action::Tick | Action::Render) {
                debug!("action: {action}");
            }
            match &action {
                Action::Tick => {}
                Action::Quit => self.should_quit = true,
                Action::Suspend => self.should_suspend = true,
                Action::Resume => self.should_suspend = false,
                Action::ClearScreen => {
                    tui.terminal.clear().context("clearing screen")?;
                }
                Action::ToggleDebug => self.show_debug = !self.show_debug,
                Action::ToggleHelp => self.show_help = !self.show_help,
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
                Action::Render => self.render(tui).context("rendering")?,
                _ => {}
            }
            // Fan out to all components
            for (_, comp) in &mut self.components {
                if let Some(new_action) = comp.update(action.clone())? {
                    let _ = self.action_tx.try_send(new_action);
                }
            }
            self.status_bar.update(action.clone())?;

            // Feed debug snapshot directly to debug component when sidebar is visible.
            // Avoids double-sending through the channel by calling update() directly.
            if self.show_debug {
                let snapshot = self
                    .components
                    .iter()
                    .map(|(id, comp)| format!("[{id}]\n{comp:#?}"))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                self.debug_comp.update(Action::DebugSnapshot(snapshot))?;
            }
        }
        Ok(())
    }

    fn render(&mut self, tui: &mut Tui) -> Result<()> {
        let focused_id = match &self.focus {
            FocusState::Normal { focused } | FocusState::FullScreen(focused) => *focused,
        };
        for (id, comp) in &mut self.components {
            comp.set_focused(*id == focused_id);
        }

        let focus = self.focus.clone();
        let preset = self.preset;
        let visible = self.visible.clone();
        let show_debug = self.show_debug;
        let show_help = self.show_help;
        let status_pos = self.status_pos;
        let slot_overrides = self.slot_overrides.clone();
        let cpu_height = self
            .components
            .iter()
            .find(|(id, _)| *id == ComponentId::Cpu)
            .and_then(|(_, c)| c.preferred_height());
        let hints = LayoutHints {
            left_top: cpu_height,
            left_mid: self
                .components
                .iter()
                .find(|(id, _)| *id == ComponentId::Mem)
                .and_then(|(_, c)| c.preferred_height()),
            right_top: cpu_height,
        };
        // Compute which component IDs have a layout slot so Tab cycling only
        // visits components the user can see. Use a zero-size rect — we only
        // need the ID mapping, not actual coordinates.
        let probe_map = preset.compute(Rect::new(0, 0, 0, 0), &slot_overrides, &hints);
        self.rendered_ids = self
            .components
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| visible.contains(id) && probe_map.values().any(|(cid, _)| cid == id))
            .collect();

        tui.draw(|frame| {
            let total_area = frame.area();

            let (status_rect, content_area) = split_status_bar(total_area, status_pos);
            if status_pos != StatusBarPosition::Hidden {
                let _ = self.status_bar.draw(frame, status_rect);
            }

            let (main_area, debug_area) = if show_debug {
                let cols = ratatui::layout::Layout::horizontal([
                    ratatui::layout::Constraint::Fill(1),
                    ratatui::layout::Constraint::Length(40),
                ])
                .split(content_area);
                (cols[0], Some(cols[1]))
            } else {
                (content_area, None)
            };

            if let Some(da) = debug_area {
                let _ = self.debug_comp.draw(frame, da);
            }

            match &focus {
                FocusState::FullScreen(id) => {
                    if let Some((_, comp)) = self.components.iter_mut().find(|(cid, _)| cid == id) {
                        let _ = comp.draw(frame, main_area);
                    }
                }
                FocusState::Normal { .. } => {
                    let slot_map = preset.compute(main_area, &slot_overrides, &hints);
                    for (component_id, rect) in slot_map.values() {
                        if !visible.contains(component_id) {
                            continue;
                        }
                        if let Some((_, comp)) = self
                            .components
                            .iter_mut()
                            .find(|(cid, _)| cid == component_id)
                        {
                            let _ = comp.draw(frame, *rect);
                        }
                    }
                }
            }

            // Help overlay is drawn last so it appears on top of everything else.
            if show_help {
                let _ = self.help_comp.draw(frame, total_area);
            }
        })?;
        Ok(())
    }
}
