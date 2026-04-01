// SPDX-License-Identifier: GPL-3.0-only

use anyhow::Context;
use crossterm::{
    cursor,
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event as CrosstermEvent, EventStream, KeyEvent, KeyEventKind, MouseEvent,
    },
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::{FutureExt, StreamExt};
use ratatui::backend::CrosstermBackend as Backend;
use std::{
    io::{Stdout, stdout},
    ops::{Deref, DerefMut},
    time::Duration,
};
use tokio::{sync::mpsc, task::JoinHandle, time::interval};
use tokio_util::sync::CancellationToken;

// These variants are part of the event vocabulary even if not all are
// dispatched to app logic yet; suppress dead-code lint for the whole enum.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum Event {
    Init,
    Quit,
    Error,
    Closed,
    Tick,
    Render,
    FocusGained,
    FocusLost,
    Paste(String),
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
}

pub struct Tui {
    terminal: ratatui::Terminal<Backend<Stdout>>,
    task: JoinHandle<()>,
    cancellation_token: CancellationToken,
    event_rx: mpsc::Receiver<Event>,
    event_tx: mpsc::Sender<Event>,
    frame_rate: f64,
    tick_rate: f64,
    mouse: bool,
    paste: bool,
}

impl Tui {
    pub fn new() -> anyhow::Result<Self> {
        let (event_tx, event_rx) = mpsc::channel(256);
        Ok(Self {
            terminal: ratatui::Terminal::new(Backend::new(stdout()))
                .context("creating terminal")?,
            task: tokio::spawn(async {}),
            cancellation_token: CancellationToken::new(),
            event_rx,
            event_tx,
            frame_rate: 1.0,
            tick_rate: 1.0,
            mouse: false,
            paste: false,
        })
    }

    pub fn tick_rate(mut self, r: f64) -> Self {
        self.tick_rate = r;
        self
    }

    pub fn frame_rate(mut self, r: f64) -> Self {
        self.frame_rate = r;
        self
    }

    pub fn mouse(mut self, m: bool) -> Self {
        self.mouse = m;
        self
    }

    pub fn start(&mut self) {
        self.cancel();
        self.cancellation_token = CancellationToken::new();
        let event_loop = Self::event_loop(
            self.event_tx.clone(),
            self.cancellation_token.clone(),
            self.tick_rate,
            self.frame_rate,
        );
        self.task = tokio::spawn(event_loop);
    }

    async fn event_loop(
        tx: mpsc::Sender<Event>,
        token: CancellationToken,
        tick_rate: f64,
        frame_rate: f64,
    ) {
        let mut stream = EventStream::new();
        let mut tick = interval(Duration::from_secs_f64(1.0 / tick_rate));
        let mut render = interval(Duration::from_secs_f64(1.0 / frame_rate));

        let _ = tx.send(Event::Init).await;
        loop {
            let event = tokio::select! {
                _ = token.cancelled() => break,
                _ = tick.tick()   => Event::Tick,
                _ = render.tick() => Event::Render,
                ev = stream.next().fuse() => match ev {
                    Some(Ok(CrosstermEvent::Key(k))) if k.kind == KeyEventKind::Press => Event::Key(k),
                    Some(Ok(CrosstermEvent::Mouse(m)))     => Event::Mouse(m),
                    Some(Ok(CrosstermEvent::Resize(x, y))) => Event::Resize(x, y),
                    Some(Ok(CrosstermEvent::FocusLost))    => Event::FocusLost,
                    Some(Ok(CrosstermEvent::FocusGained))  => Event::FocusGained,
                    Some(Ok(CrosstermEvent::Paste(s)))     => Event::Paste(s),
                    Some(Err(_)) => Event::Error,
                    None => break,
                    _ => continue,
                },
            };
            if tx.send(event).await.is_err() {
                break;
            }
        }
        token.cancel();
    }

    pub fn stop(&self) -> anyhow::Result<()> {
        // Signal the event loop to exit gracefully, then abort the JoinHandle
        // to ensure cleanup without blocking. No thread::sleep — callers may
        // be on a Tokio worker thread.
        self.cancel();
        self.task.abort();
        Ok(())
    }

    pub fn enter(&mut self) -> anyhow::Result<()> {
        crossterm::terminal::enable_raw_mode().context("enabling raw mode")?;
        crossterm::execute!(stdout(), EnterAlternateScreen, cursor::Hide)
            .context("entering alternate screen")?;
        if self.mouse {
            crossterm::execute!(stdout(), EnableMouseCapture).context("enabling mouse capture")?;
        }
        if self.paste {
            crossterm::execute!(stdout(), EnableBracketedPaste)
                .context("enabling bracketed paste")?;
        }
        self.start();
        Ok(())
    }

    pub fn exit(&mut self) -> anyhow::Result<()> {
        self.stop()?;
        if crossterm::terminal::is_raw_mode_enabled().unwrap_or(false) {
            self.flush().context("flushing terminal")?;
            if self.mouse {
                crossterm::execute!(stdout(), DisableMouseCapture)?;
            }
            if self.paste {
                crossterm::execute!(stdout(), DisableBracketedPaste)?;
            }
            crossterm::execute!(stdout(), LeaveAlternateScreen, cursor::Show)?;
            crossterm::terminal::disable_raw_mode().context("disabling raw mode")?;
        }
        Ok(())
    }

    pub fn cancel(&self) {
        self.cancellation_token.cancel();
    }

    pub async fn next_event(&mut self) -> Option<Event> {
        self.event_rx.recv().await
    }

    pub fn resize(&mut self, rect: ratatui::layout::Rect) -> anyhow::Result<()> {
        self.terminal.resize(rect).context("resizing terminal")
    }
}

impl Deref for Tui {
    type Target = ratatui::Terminal<Backend<Stdout>>;

    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl DerefMut for Tui {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.exit();
    }
}
