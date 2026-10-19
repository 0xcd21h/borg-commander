use std::{
    collections::HashMap,
    io::Write as _,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use borg_core::{
    AMBIENT_FRAME, AppState, Appearance, Aside, Content, Effect, Image, JobCommand, Msg, Pace, Peeked, ShellView, Side,
    View, Viewer, update, viewer_size,
};
use borg_gfx::{Capabilities, Graphics, Output, Protocol, TerminalColors, decode, is_picture};
use borg_jobs::{Control, Spec, run};
use borg_term::{Emulator, GridSize, Pty, PtyEvent, Scroll, encode_mouse, shell_command};
use borg_ui::{Palette, Pixels, Rgb, Scene, ThemeSet, draw, local_now, mouse_input};
use borg_vfs::{Backend, Local};
use borg_view::{FileSource, Pager};
use crossterm::{
    event::{Event, EventStream, KeyEventKind, MouseButton, MouseEvent, MouseEventKind},
    queue,
    terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate},
};
use futures::StreamExt;
use jiff::Zoned;
use ratatui::{DefaultTerminal, layout::Size};
use tokio::{
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    task::JoinHandle,
    time::sleep_until,
};

use crate::{
    config::{Config, ConfigDir},
    launch::{self, Launch},
    peek,
    session::{RESTORED_LINES, RESTORED_MARKER, Session, StateDir},
};

const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const WHEEL_LINES: i32 = 3;

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
    home: Option<PathBuf>,
    state_dir: Option<StateDir>,
    pending: Vec<Effect>,
    graphics: Graphics,
    jobs: HashMap<u64, Arc<Control>>,
    last_click: Option<(u16, u16, Instant)>,
}

enum Flow {
    Continue,
    Quit,
}

impl App {
    pub fn start(launch: &Launch, size: Size, detected: launch::Terminal) -> Result<Self> {
        let grid = GridSize::console(size.width, size.height);
        let now = Instant::now();
        let appearance = appearance(launch);
        let (mut state, pending) =
            AppState::new(launch.startup.clone(), size.width, size.height, launch.settings.clone(), appearance, now);
        state.notice = launch.warnings.first().cloned();

        let mut emulator = Emulator::new(grid, state.settings.scrollback);
        if let Some(restored) = &launch.restored {
            emulator.feed(restored);
            emulator.feed(RESTORED_MARKER);
        }

        let shell_dir = state.active_panel().dir().to_path_buf();
        let command = shell_command(&launch.shell, &shell_dir, &launch.runtime_dir).context("preparing shell")?;
        let (pty_tx, pty_events) = unbounded_channel();
        let pty = Pty::spawn(command, grid, pty_tx)?;
        let (messages, inbox) = unbounded_channel();

        Ok(Self {
            emulator,
            backend: Arc::new(Local::new(state.settings.listing_batch)),
            state,
            pty,
            pty_events,
            messages,
            inbox,
            listings: [None, None],
            palette: palette(&detected.colors),
            themes: launch.themes.clone(),
            config_dir: launch.config_dir.clone(),
            config: launch.config.clone(),
            home: launch.home.clone(),
            state_dir: launch.state_dir.clone(),
            pending,
            graphics: Graphics::new(detected.capabilities),
            jobs: HashMap::new(),
            last_click: None,
        })
    }

    pub async fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let result = self.event_loop(terminal).await;
        self.save_session();
        let backend = terminal.backend_mut();
        backend.write_all(&self.graphics.reset())?;
        backend.flush()?;
        result
    }

    async fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let mut input = EventStream::new();
        let mut dirty = true;
        let mut last_draw = Instant::now() - self.state.settings.motion.frame;
        let mut animating = false;

        if let Some(dir) = self.state_dir.clone().filter(|_| self.config.update_check()) {
            crate::update::check_in_background(dir, self.messages.clone());
        }
        let initial = std::mem::take(&mut self.pending);
        if let Flow::Quit = self.apply(initial)? {
            return Ok(());
        }

        loop {
            let now = Instant::now();
            let pace = self.pace(now);
            let frame = match pace {
                Some(Pace::Ambient) => AMBIENT_FRAME,
                Some(Pace::Smooth) | None => self.state.settings.motion.frame,
            };
            if (dirty || animating) && now >= last_draw + frame {
                self.render(terminal, now)?;
                last_draw = now;
                dirty = false;
                animating = self.pace(now).is_some();
            }

            let redraw = dirty || animating;
            let deadline = if redraw { last_draw + frame } else { now + IDLE_WAIT };
            let wake = self.state.wake_at();
            let flow = tokio::select! {
                event = input.next() => match event {
                    Some(Ok(event)) => self.on_terminal_event(event)?,
                    Some(Err(e)) => return Err(e.into()),
                    None => Flow::Quit,
                },
                Some(event) = self.pty_events.recv() => self.on_pty(event)?,
                Some(msg) = self.inbox.recv() => self.dispatch(msg)?,
                () = sleep_until(deadline.into()), if redraw => Flow::Continue,
                () = sleep_until(wake.unwrap_or(now + IDLE_WAIT).into()), if wake.is_some() => self.dispatch(Msg::Tick)?,
            };

            if let Flow::Quit = flow {
                return Ok(());
            }
            dirty = true;
        }
    }

    fn pace(&self, now: Instant) -> Option<Pace> {
        let theme = self.themes.find(self.state.appearance.current_id());
        let breathing =
            theme.effects.breathe && self.state.settings.truecolor && self.state.motion().fade > Duration::ZERO;
        self.state.pace(now).or(breathing.then_some(Pace::Ambient))
    }

    fn save_session(&self) {
        let Some(dir) = &self.state_dir else { return };
        let _ = dir.save_session(&Session::capture(&self.state));
        let _ = if self.state.settings.preferences.restore_terminal {
            dir.save_terminal(&self.emulator.export_ansi(RESTORED_LINES))
        } else {
            dir.forget_terminal()
        };
    }

    fn render(&mut self, terminal: &mut DefaultTerminal, now: Instant) -> Result<()> {
        self.state.settle();
        let clock = local_now();
        queue!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
        let mut output = self.draw_frame(terminal, &clock, now)?;
        if output.repaint {
            terminal.clear()?;
            output = self.draw_frame(terminal, &clock, now)?;
        }
        let backend = terminal.backend_mut();
        backend.write_all(&output.bytes)?;
        queue!(backend, EndSynchronizedUpdate)?;
        backend.flush()?;
        if let Some(error) = self.graphics.take_errors().first() {
            self.state.notice = Some(format!("{error}"));
        }
        Ok(())
    }

    fn draw_frame(&mut self, terminal: &mut DefaultTerminal, clock: &Zoned, now: Instant) -> Result<Output> {
        let mut placements = Vec::new();
        let mut resized = Ok(());
        let frame = terminal.draw(|frame| {
            let area = frame.area();
            if (self.state.cols, self.state.rows) != (area.width, area.height) {
                resized = self.resize(area.width, area.height).map(|_| ());
                if resized.is_err() {
                    return;
                }
            }
            let scene = Scene {
                state: &self.state,
                screen: self.emulator.screen(),
                palette: &self.palette,
                themes: &self.themes,
                clock,
                now,
                home: self.home.as_deref(),
                pixels: pixels(self.graphics.capabilities()),
            };
            placements = draw(frame, &scene);
        })?;
        resized?;
        Ok(self.graphics.frame(&placements, frame.buffer))
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<Flow> {
        if (self.state.cols, self.state.rows) == (cols, rows) {
            return Ok(Flow::Continue);
        }
        let grid = GridSize::console(cols, rows);
        self.emulator.resize(grid);
        self.pty.resize(grid)?;
        self.last_click = None;
        self.dispatch(Msg::Resize { cols, rows })
    }

    fn on_terminal_event(&mut self, event: Event) -> Result<Flow> {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                self.last_click = None;
                self.observe(Msg::Observe)?;
                self.dispatch(Msg::Key(key))
            }
            Event::Paste(text) => {
                self.observe(Msg::Observe)?;
                self.dispatch(Msg::Paste(text))
            }
            Event::Resize(cols, rows) => self.resize(cols, rows),
            Event::Mouse(event) => {
                self.observe(Msg::Observe)?;
                self.on_mouse(event)
            }
            _ => Ok(Flow::Continue),
        }
    }

    fn on_mouse(&mut self, event: MouseEvent) -> Result<Flow> {
        let now = Instant::now();
        self.state.idle_since = now;
        if self.state.screensaver {
            return self.dispatch(Msg::Pointer(borg_core::Pointer::DismissMenu));
        }
        if self.state.effective_view() == View::Shell && self.state.overlay.is_none() {
            if event.row >= self.emulator.screen().rows() {
                return Ok(Flow::Continue);
            }
            if self.state.shell.captured() {
                if let Some(bytes) = encode_mouse(event, self.emulator.mode()) {
                    self.pty.write(&bytes)?;
                }
            } else {
                let scroll = match event.kind {
                    MouseEventKind::ScrollUp => Some(Scroll::Delta(WHEEL_LINES)),
                    MouseEventKind::ScrollDown => Some(Scroll::Delta(-WHEEL_LINES)),
                    _ => None,
                };
                if let Some(scroll) = scroll {
                    self.emulator.scroll(scroll);
                }
            }
            return Ok(Flow::Continue);
        }
        let left = event.kind == MouseEventKind::Down(MouseButton::Left);
        let double = left
            && self.last_click.is_some_and(|(col, row, at)| {
                (col, row) == (event.column, event.row) && now.saturating_duration_since(at) <= DOUBLE_CLICK
            });
        let msg = mouse_input(&self.state, event, double, now);
        if left {
            self.last_click = match &msg {
                Some(Msg::Pointer(borg_core::Pointer::Panel { mark: false, .. })) if !double => {
                    Some((event.column, event.row, now))
                }
                _ => None,
            };
        } else if matches!(event.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown) {
            self.last_click = None;
        }
        match msg {
            Some(msg) => self.dispatch(msg),
            None => Ok(Flow::Continue),
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
            command_rows: self.emulator.command_rows(),
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
                Effect::Write(bytes) => {
                    self.emulator.scroll(Scroll::Bottom);
                    self.pty.write(&bytes)?;
                }
                Effect::ScrollConsole(scroll) => self.emulator.scroll(scroll),
                Effect::View(path) => self.open_viewer(path),
                Effect::SaveConfig => self.save_config(),
                Effect::StartJob { id, spec } => self.start_job(id, spec),
                Effect::JobControl { id, command } => self.control_job(id, command),
                Effect::Peek { aside, path, size } => self.peek(aside, path, size),
                Effect::ReadCommandLine => drop(self.messages.send(Msg::CommandLine(self.emulator.command_line()))),
                Effect::Quit => return Ok(Flow::Quit),
            }
        }
        Ok(Flow::Continue)
    }

    fn start_job(&mut self, id: u64, spec: Spec) {
        let running: Vec<u64> = self.state.jobs.iter().map(|job| job.id).collect();
        self.jobs.retain(|id, _| running.contains(id));
        let control = Arc::new(Control::default());
        self.jobs.insert(id, Arc::clone(&control));
        let messages = self.messages.clone();
        tokio::task::spawn_blocking(move || {
            run(&spec, &control, &mut |event| drop(messages.send(Msg::Job { id, event })));
        });
    }

    fn control_job(&self, id: u64, command: JobCommand) {
        let Some(control) = self.jobs.get(&id) else { return };
        match command {
            JobCommand::Pause(paused) => control.set_paused(paused),
            JobCommand::Cancel => control.cancel(),
            JobCommand::Answer(decision) => control.answer(decision),
        }
    }

    fn save_config(&self) {
        let appearance = &self.state.appearance;
        let config =
            self.config.capture(appearance.current_id(), &self.state.settings.preferences, appearance.unlocked());
        let saved = self.config_dir.as_ref().map(|dir| dir.save(&config));
        if let Some(Err(e)) = saved {
            let _ = self.messages.send(Msg::Notice(format!("{e:#}")));
        }
    }

    fn open_viewer(&self, path: PathBuf) {
        let (width, height) = viewer_size(self.state.cols, self.state.rows);
        let cell = self.graphics.capabilities().cell;
        let bounds = (span(width, cell.width), span(height, cell.height));
        let messages = self.messages.clone();
        tokio::task::spawn_blocking(move || {
            let content =
                if is_picture(&path) { picture(&path, bounds).map(Content::Image) } else { text(&path, width, height) };
            let opened = content
                .map(|content| Msg::Opened(Viewer { path: path.clone(), content }))
                .unwrap_or_else(|e| Msg::Notice(format!("{}: {e:#}", path.display())));
            let _ = messages.send(opened);
        });
    }

    fn peek(&self, aside: Aside, path: PathBuf, size: (usize, usize)) {
        let cell = self.graphics.capabilities().cell;
        let messages = self.messages.clone();
        tokio::task::spawn_blocking(move || {
            let peek = peek::peek(aside, &path, size, (cell.width, cell.height));
            let _ = messages.send(Msg::Peeked(Peeked { aside, path, peek }));
        });
    }

    fn list(&mut self, side: Side, generation: u64, dir: PathBuf) {
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

fn palette(colors: &TerminalColors) -> Palette {
    let base = Palette::default();
    let rgb = |[r, g, b]: [u8; 3]| Rgb::new(r, g, b);
    Palette {
        ansi: std::array::from_fn(|index| colors.ansi[index].map_or(base.ansi[index], rgb)),
        foreground: colors.foreground.map_or(base.foreground, rgb),
        background: colors.background.map_or(base.background, rgb),
    }
}

fn pixels(capabilities: Capabilities) -> Pixels {
    match capabilities.protocol {
        Protocol::Cells => Pixels::Off,
        Protocol::Kitty | Protocol::Sixel | Protocol::Iterm => Pixels::Pictures,
    }
}

pub(crate) fn span(cells: usize, cell: u16) -> u32 {
    u32::try_from(cells).unwrap_or(u32::MAX).saturating_mul(u32::from(cell))
}

pub(crate) fn picture(path: &Path, bounds: (u32, u32)) -> Result<Image> {
    let bytes = std::fs::metadata(path)?.len();
    let decoded = decode(path, bounds)?;
    Ok(Image { picture: decoded.picture, original: decoded.original, format: decoded.format, bytes })
}

fn text(path: &Path, width: usize, height: usize) -> Result<Content> {
    let source = FileSource::open(path)?;
    Ok(Content::Text(Pager::for_path(Arc::new(source), path, width, height)?))
}

fn appearance(launch: &Launch) -> Appearance {
    launch.themes.appearance(launch.config.theme.as_deref(), launch.config.unlocked())
}
