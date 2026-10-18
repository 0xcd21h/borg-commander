use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use borg_core::{AppState, Appearance, Effect, Msg, ShellView, Side, ThemeChoice, update};
use borg_term::{Emulator, GridSize, Pty, PtyEvent, shell_command};
use borg_ui::{Palette, Scene, ThemeSet, draw, local_now};
use borg_vfs::{Backend, Local};
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures::StreamExt;
use ratatui::{DefaultTerminal, layout::Size};
use tokio::{
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    task::JoinHandle,
    time::sleep_until,
};

use crate::{
    config::{Config, ConfigDir},
    launch::Launch,
};

const IDLE_WAIT: Duration = Duration::from_secs(3600);

pub struct App {
    state: AppState,
    emulator: Emulator,
    pty: Pty,
    pty_events: UnboundedReceiver<PtyEvent>,
    backend: Arc<dyn Backend>,
    messages: UnboundedSender<Msg>,
    inbox: UnboundedReceiver<Msg>,
    listings: [Option<JoinHandle<()>>; 2],
    palette: Palette,
    themes: ThemeSet,
    config_dir: Option<ConfigDir>,
    config: Config,
    pending: Vec<Effect>,
}

enum Flow {
    Continue,
    Quit,
}

impl App {
    pub fn start(launch: &Launch, size: Size) -> Result<Self> {
        let grid = GridSize { cols: size.width, rows: size.height };
        let now = Instant::now();
        let dirs = [launch.cwd.clone(), launch.cwd.clone()];
        let appearance = appearance(launch);
        let (mut state, pending) =
            AppState::new(dirs, size.width, size.height, launch.settings.clone(), appearance, now);
        state.notice = launch.warnings.first().cloned();

        let command = shell_command(&launch.shell, &launch.cwd, &launch.runtime_dir).context("preparing shell")?;
        let (pty_tx, pty_events) = unbounded_channel();
        let pty = Pty::spawn(command, grid, pty_tx)?;
        let (messages, inbox) = unbounded_channel();

        Ok(Self {
            emulator: Emulator::new(grid, state.settings.scrollback),
            backend: Arc::new(Local::new(state.settings.listing_batch)),
            state,
            pty,
            pty_events,
            messages,
            inbox,
            listings: [None, None],
            palette: Palette::default(),
            themes: launch.themes.clone(),
            config_dir: launch.config_dir.clone(),
            config: launch.config.clone(),
            pending,
        })
    }

    pub async fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let mut input = EventStream::new();
        let mut dirty = true;
        let mut last_draw = Instant::now() - self.state.settings.motion.frame;

        let initial = std::mem::take(&mut self.pending);
        if let Flow::Quit = self.apply(initial)? {
            return Ok(());
        }

        loop {
            let now = Instant::now();
            let frame = self.state.settings.motion.frame;
            if dirty && now >= last_draw + frame {
                self.render(terminal, now)?;
                last_draw = now;
                dirty = self.state.is_animating(now);
            }

            let deadline = if dirty { last_draw + frame } else { now + IDLE_WAIT };
            let flow = tokio::select! {
                event = input.next() => match event {
                    Some(Ok(event)) => self.on_terminal_event(event)?,
                    Some(Err(e)) => return Err(e.into()),
                    None => Flow::Quit,
                },
                Some(event) = self.pty_events.recv() => self.on_pty(event)?,
                Some(msg) = self.inbox.recv() => self.dispatch(msg)?,
                () = sleep_until(deadline.into()), if dirty => Flow::Continue,
            };

            if let Flow::Quit = flow {
                return Ok(());
            }
            dirty = true;
        }
    }

    fn render(&mut self, terminal: &mut DefaultTerminal, now: Instant) -> Result<()> {
        self.state.settle();
        let clock = local_now();
        let scene = Scene {
            state: &self.state,
            screen: self.emulator.screen(),
            palette: &self.palette,
            themes: &self.themes,
            clock: &clock,
            now,
        };
        terminal.draw(|frame| draw(frame, &scene))?;
        Ok(())
    }

    fn on_terminal_event(&mut self, event: Event) -> Result<Flow> {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                self.observe(Msg::Observe)?;
                self.dispatch(Msg::Key(key))
            }
            Event::Paste(text) => {
                self.observe(Msg::Observe)?;
                self.dispatch(Msg::Paste(text))
            }
            Event::Resize(cols, rows) => {
                let grid = GridSize { cols, rows };
                self.emulator.resize(grid);
                self.pty.resize(grid)?;
                self.dispatch(Msg::Resize { cols, rows })
            }
            _ => Ok(Flow::Continue),
        }
    }

    fn on_pty(&mut self, event: PtyEvent) -> Result<Flow> {
        let mut flow = self.consume(event)?;
        while let (Flow::Continue, Ok(next)) = (&flow, self.pty_events.try_recv()) {
            flow = self.consume(next)?;
        }
        match flow {
            Flow::Continue => self.observe(Msg::ShellOutput),
            Flow::Quit => Ok(Flow::Quit),
        }
    }

    fn consume(&mut self, event: PtyEvent) -> Result<Flow> {
        match event {
            PtyEvent::Output(bytes) => {
                let signals = self.emulator.feed(&bytes);
                for reply in self.emulator.drain_replies() {
                    self.pty.write(reply.as_bytes())?;
                }
                for signal in signals {
                    if let Flow::Quit = self.dispatch(Msg::Signal(signal))? {
                        return Ok(Flow::Quit);
                    }
                }
                Ok(Flow::Continue)
            }
            PtyEvent::Closed => self.dispatch(Msg::ShellExited),
        }
    }

    fn observe(&mut self, message: fn(ShellView) -> Msg) -> Result<Flow> {
        let view = ShellView {
            prompt: self.emulator.prompt_state(),
            foreground: self.pty.foreground(),
            mode: self.emulator.mode(),
        };
        self.dispatch(message(view))
    }

    fn dispatch(&mut self, msg: Msg) -> Result<Flow> {
        let effects = update(&mut self.state, msg, Instant::now());
        self.apply(effects)
    }

    fn apply(&mut self, effects: Vec<Effect>) -> Result<Flow> {
        for effect in effects {
            match effect {
                Effect::List { side, generation, dir } => self.list(side, generation, dir),
                Effect::Write(bytes) => self.pty.write(&bytes)?,
                Effect::SaveTheme(id) => self.save_theme(id),
                Effect::Quit => return Ok(Flow::Quit),
            }
        }
        Ok(Flow::Continue)
    }

    fn save_theme(&mut self, id: String) {
        self.config.theme = Some(id);
        let saved = self.config_dir.as_ref().map(|dir| dir.save(&self.config));
        if let Some(Err(e)) = saved {
            let _ = self.messages.send(Msg::Notice(format!("{e:#}")));
        }
    }

    fn list(&mut self, side: Side, generation: u64, dir: std::path::PathBuf) {
        let mut batches = self.backend.list(&dir);
        let messages = self.messages.clone();
        let task = tokio::spawn(async move {
            while let Some(batch) = batches.next().await {
                let msg = match batch {
                    Ok(entries) => Msg::Batch { side, generation, entries },
                    Err(e) => {
                        let _ = messages.send(Msg::Listed { side, generation, error: Some(e.to_string()) });
                        return;
                    }
                };
                if messages.send(msg).is_err() {
                    return;
                }
            }
            let _ = messages.send(Msg::Listed { side, generation, error: None });
        });

        if let Some(previous) = self.listings[side.index()].replace(task) {
            previous.abort();
        }
    }
}

fn appearance(launch: &Launch) -> Appearance {
    let choices = launch
        .themes
        .themes()
        .iter()
        .map(|theme| ThemeChoice { id: theme.id.clone(), name: theme.name.clone() })
        .collect();
    let current = launch.config.theme.as_deref().and_then(|id| launch.themes.position(id)).unwrap_or_default();
    Appearance::new(choices, current)
}
