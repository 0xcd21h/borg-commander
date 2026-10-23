use borg_i18n::tr;
use std::{
    collections::HashMap,
    io::Write as _,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use borg_core::{
    AMBIENT_FRAME, AppState, Appearance, Aside, Effect, Image, JobCommand, Msg, Pace, ShellView, Side, View, Viewer,
    update, viewer_size,
};
use borg_gfx::{Capabilities, Graphics, Output, Protocol, TerminalColors, decode};
use borg_jobs::{Control, Spec, run};
use borg_term::{Emulator, GridSize, Pty, PtyEvent, Scroll, encode_mouse, shell_command};
use borg_ui::{Palette, Pixels, Rgb, Scene, ThemeSet, draw, local_now, mouse_input};
use borg_vfs::{Backend, Local};
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
    preview,
    session::{RESTORED_LINES, RESTORED_MARKER, Session, StateDir},
};

const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// The folder under the runtime directory where container files are copied for viewing.
const DOCKER_SCRATCH: &str = "docker";
const WHEEL_LINES: i32 = 3;
/// Wheel events closer together than this belong to one burst.
const WHEEL_BURST: Duration = Duration::from_millis(200);

/// What a wheel event scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WheelTarget {
    Console,
    Viewer,
    Dialog,
    Panels,
}

/// Trackpads keep sending wheel events after the fingers lift. A burst stays with what it
/// started scrolling, so switching screens mid-burst never scrolls the new one.
#[derive(Debug, Default)]
struct WheelBurst {
    last: Option<(Instant, WheelTarget)>,
}

impl WheelBurst {
    fn accepts(&mut self, target: WheelTarget, now: Instant) -> bool {
        let owner = match self.last {
            Some((at, owner)) if now.saturating_duration_since(at) <= WHEEL_BURST => owner,
            _ => target,
        };
        self.last = Some((now, owner));
        owner == target
    }
}

const IDLE_WAIT: Duration = Duration::from_secs(3600);

pub struct App {
    search: crate::search::Search,
    state: AppState,
    emulator: Emulator,
    pty: Pty,
    /// Where the shell reads the folder to change into without a command line.
    cd_request: PathBuf,
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
    collections_writable: bool,
    pending: Vec<Effect>,
    graphics: Graphics,
    jobs: HashMap<u64, Arc<Control>>,
    last_click: Option<(u16, u16, Instant)>,
    wheel: WheelBurst,
    preview: preview::Loader,
    plugins: crate::plugins::Plugins,
    monitor: crate::activity::Monitor,
    docker: crate::docker::Docker,
    shells: crate::docker::Shells,
    /// Whose terminal the command line and Ctrl-O console show, as the core decided.
    console: borg_core::Console,
    agent: crate::agent::Agent,
}

enum Flow {
    Continue,
    Quit,
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.cd_request);
    }
}

impl App {
    pub fn start(launch: &Launch, size: Size, detected: launch::Terminal) -> Result<Self> {
        let grid = GridSize::console(size.width, size.height);
        let now = Instant::now();
        let appearance = appearance(launch);
        let (mut state, pending) =
            AppState::new(launch.startup.clone(), size.width, size.height, launch.settings.clone(), appearance, now);
        state.notice = launch.warnings.first().cloned();
        // The glitch intro plays only where animations do: not over SSH or without truecolor.
        if state.settings.motion == borg_core::Motion::LOCAL {
            state.intro = Some(now);
        }
        let mut collections_writable = true;
        if let Some(dir) = &launch.state_dir {
            state.game2048 = dir.load_game2048().ok().flatten();
            match dir.load_collections() {
                Ok(collections) => state.collections = collections,
                Err(error) => {
                    collections_writable = false;
                    state.notice = Some(format!(tr!(collections_load_error), error = error));
                }
            }
        }

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

        let preview = preview::Loader::new(messages.clone());
        let plugins = crate::plugins::Plugins::new(messages.clone());
        let monitor = crate::activity::Monitor::new(messages.clone());
        let docker = crate::docker::Docker::new(messages.clone(), launch.runtime_dir.join(DOCKER_SCRATCH));
        let agent = crate::agent::Agent::start(&launch.runtime_dir, messages.clone());
        Ok(Self {
            search: crate::search::Search::new(),
            emulator,
            backend: Arc::new(Local::new(state.settings.listing_batch)),
            state,
            pty,
            cd_request: borg_term::cd_request_file(&launch.runtime_dir),
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
            collections_writable,
            pending,
            graphics: Graphics::new(detected.capabilities),
            jobs: HashMap::new(),
            last_click: None,
            wheel: WheelBurst::default(),
            preview,
            plugins,
            monitor,
            docker,
            shells: crate::docker::Shells::new(),
            console: borg_core::Console::Host,
            agent,
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
                Some((container, event)) = self.shells.incoming.recv() => self.on_container_shell(container, event)?,
                Some((call, reply)) = self.agent.incoming.recv() => self.on_agent(call, reply)?,
                Some(msg) = self.inbox.recv() => self.dispatch(msg)?,
                Some(msg) = self.search.incoming.recv() => self.dispatch(msg)?,
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
        if let Some(game) = &self.state.game2048 {
            let _ = dir.save_game2048(game);
        }
        if self.collections_writable {
            let _ = dir.save_collections(&self.state.collections);
        }
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
                screen: match &self.console {
                    borg_core::Console::Container(id) => {
                        self.shells.screen(id).unwrap_or_else(|| self.emulator.screen())
                    }
                    borg_core::Console::Host => self.emulator.screen(),
                },
                docker_shell: self.state.docker.visible_shell().and_then(|shell| self.shells.screen(&shell.container)),
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

    /// The terminal and process of the console: the host's shell, or the container shell
    /// whose panel is active.
    fn console(&mut self) -> (&mut Emulator, &mut Pty) {
        if let borg_core::Console::Container(id) = &self.console
            && self.shells.screen(id).is_some()
        {
            let id = id.clone();
            if let Some(parts) = self.shells.parts(&id) {
                return parts;
            }
        }
        (&mut self.emulator, &mut self.pty)
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
        if matches!(
            event.kind,
            MouseEventKind::ScrollUp
                | MouseEventKind::ScrollDown
                | MouseEventKind::ScrollLeft
                | MouseEventKind::ScrollRight
        ) {
            let target = if self.state.effective_view() == View::Shell {
                WheelTarget::Console
            } else if self.state.viewer.is_some() {
                WheelTarget::Viewer
            } else if self.state.overlay.is_some() {
                WheelTarget::Dialog
            } else {
                WheelTarget::Panels
            };
            if !self.wheel.accepts(target, now) {
                return Ok(Flow::Continue);
            }
        }
        if self.state.effective_view() == View::Shell && self.state.overlay.is_none() {
            let captured = self.state.shell.captured();
            let (emulator, pty) = self.console();
            if event.row >= emulator.screen().rows() {
                return Ok(Flow::Continue);
            }
            if captured {
                if let Some(bytes) = encode_mouse(event, emulator.mode()) {
                    pty.write(&bytes)?;
                }
            } else {
                let scroll = match event.kind {
                    MouseEventKind::ScrollUp => Some(Scroll::Delta(WHEEL_LINES)),
                    MouseEventKind::ScrollDown => Some(Scroll::Delta(-WHEEL_LINES)),
                    _ => None,
                };
                if let Some(scroll) = scroll {
                    emulator.scroll(scroll);
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
            Flow::Continue if self.console == borg_core::Console::Host => self.observe(Msg::ShellOutput),
            flow => Ok(flow),
        }
    }

    fn consume(&mut self, event: PtyEvent) -> Result<Flow> {
        match event {
            PtyEvent::Output(bytes) => {
                let signals = self.emulator.feed(&bytes);
                for reply in self.emulator.drain_replies() {
                    self.pty.write(reply.as_bytes())?;
                }
                let console = self.console == borg_core::Console::Host;
                for signal in signals {
                    let msg = if console { Msg::Signal(signal) } else { Msg::ParkedSignal { container: None, signal } };
                    if let Flow::Quit = self.dispatch(msg)? {
                        return Ok(Flow::Quit);
                    }
                }
                Ok(Flow::Continue)
            }
            PtyEvent::Closed => self.dispatch(Msg::ShellExited),
        }
    }

    /// Answers an agent's tool call from what the session knows. Reading happens off the
    /// event loop; commands wait for the user's approval.
    fn on_agent(
        &mut self,
        call: crate::agent::Call,
        reply: tokio::sync::oneshot::Sender<crate::agent::Reply>,
    ) -> Result<Flow> {
        use crate::agent::{self, Reply};
        let text = |key: &str| call.args[key].as_str().map(str::to_owned);
        match call.tool.as_str() {
            "borg_screen" => drop(reply.send(Reply { ok: true, text: agent::screen(&self.state) })),
            "list_folder" | "read_file" => {
                let path = agent::resolve(&self.state, &text("path").unwrap_or_else(|| ".".into()));
                let offset = call.args["offset"].as_u64().unwrap_or(0);
                let limit = call.args["limit"].as_u64().unwrap_or(agent::READ_DEFAULT);
                let list = call.tool == "list_folder";
                tokio::task::spawn_blocking(move || {
                    drop(reply.send(if list {
                        agent::list_folder(&path)
                    } else {
                        agent::read_file(&path, offset, limit)
                    }));
                });
            }
            "show_in_panel" => {
                let path = agent::resolve(&self.state, &text("path").unwrap_or_default());
                let (dir, focus) = if path.is_dir() {
                    (path.clone(), None)
                } else {
                    let parent = path.parent().map_or_else(|| PathBuf::from("/"), Path::to_path_buf);
                    (parent, path.file_name().map(|name| name.to_string_lossy().into_owned()))
                };
                drop(reply.send(Reply { ok: true, text: format!("Showing {} in Borg's other panel", path.display()) }));
                return self.dispatch(Msg::Agent(borg_core::AgentEvent::Reveal { dir, focus }));
            }
            "run_command" => {
                let Some(command) = text("command").filter(|c| !c.trim().is_empty()) else {
                    drop(reply.send(Reply { ok: false, text: "run_command needs a command".into() }));
                    return Ok(Flow::Continue);
                };
                let dir =
                    text("cwd").map_or_else(|| agent::host_dir(&self.state), |cwd| agent::resolve(&self.state, &cwd));
                let id = self.agent.ask(reply, command.clone(), dir.clone());
                return self.dispatch(Msg::Agent(borg_core::AgentEvent::Ask { id, command, dir }));
            }
            other => drop(reply.send(Reply { ok: false, text: format!("unknown tool {other}") })),
        }
        Ok(Flow::Continue)
    }

    fn on_container_shell(&mut self, container: String, event: PtyEvent) -> Result<Flow> {
        match event {
            PtyEvent::Output(bytes) => {
                let Some((mode, signals)) = self.shells.feed(&container, &bytes) else { return Ok(Flow::Continue) };
                let console = self.console == borg_core::Console::Container(container.clone());
                for signal in signals {
                    let msg = if console {
                        Msg::Signal(signal)
                    } else {
                        Msg::ParkedSignal { container: Some(container.clone()), signal }
                    };
                    if let Flow::Quit = self.dispatch(msg)? {
                        return Ok(Flow::Quit);
                    }
                }
                if console {
                    self.observe(Msg::ShellOutput)
                } else {
                    self.dispatch(Msg::DockerShellOutput { container, mode })
                }
            }
            PtyEvent::Closed => {
                self.shells.close(&container);
                self.dispatch(Msg::DockerShellClosed { container })
            }
        }
    }

    /// Keeps container shells the size they show at: the Docker view's panel, or the whole
    /// screen while one is the console.
    fn fit_container_shell(&mut self) {
        if let borg_core::Console::Container(id) = &self.console {
            let id = id.clone();
            self.shells.fit(&id, GridSize::console(self.state.cols, self.state.rows));
        }
        let docker = &self.state.docker;
        let (Some(shell), Some(side)) = (docker.visible_shell(), docker.side) else { return };
        let area = borg_core::shell_rect(self.state.layout(Instant::now()).panels[side.other().index()]);
        if area.width > 0 && area.height > 0 {
            let container = shell.container.clone();
            self.shells.fit(&container, GridSize { cols: area.width, rows: area.height });
        }
    }

    fn observe(&mut self, message: fn(ShellView) -> Msg) -> Result<Flow> {
        let (emulator, pty) = self.console();
        let view = ShellView {
            prompt: emulator.prompt_state(),
            command_rows: emulator.command_rows(),
            foreground: pty.foreground(),
            mode: emulator.mode(),
        };
        self.dispatch(message(view))
    }

    fn dispatch(&mut self, msg: Msg) -> Result<Flow> {
        let effects = update(&mut self.state, msg, Instant::now());
        if self.state.activity.side.is_none() || self.state.activity.pending.is_none() {
            self.monitor.cancel();
        }
        if self.state.docker.pending.is_none() {
            self.docker.cancel();
        }
        if !self.state.settings.preferences.git_plugin {
            self.plugins.cancel();
        }
        if self.state.aside.is_none() || self.state.active_panel().hovered().is_none() {
            self.preview.cancel();
        }
        for side in Side::ALL {
            if !self.state.panel(side).is_results() && self.state.last_results[side.index()].is_none() {
                self.search.cancel(side);
            } else if self.state.panel(side).is_results()
                && let Some(task) = self.listings[side.index()].take()
            {
                task.abort();
            }
        }
        let flow = self.apply(effects)?;
        self.fit_container_shell();
        Ok(flow)
    }

    fn apply(&mut self, effects: Vec<Effect>) -> Result<Flow> {
        for effect in effects {
            match effect {
                Effect::MeasureSizes { id, root } => self.search.measure(id, root),
                Effect::CancelMeasure => self.search.cancel_measure(),
                Effect::CancelMemorySearch => self.monitor.cancel_search(),
                Effect::SizeChildren { side, generation, root, hidden, parent } => {
                    let cache = self.search.cache.clone();
                    let messages = self.messages.clone();
                    tokio::task::spawn_blocking(move || {
                        let result = cache
                            .children(&root, hidden, &parent)
                            .ok_or_else(|| tr!(size_branch_unavailable).to_owned());
                        let _ = messages.send(Msg::SizeChildren { side, generation, parent, result });
                    });
                }
                Effect::Activity { id, request } => self.monitor.request(id, request),
                Effect::Docker { id, request } => self.docker.request(id, request),
                Effect::DockerWatch(on) => self.docker.watch(on),
                Effect::DockerShell(request) => self.container_shell(request),
                Effect::ExportMemory { path, bytes } => {
                    let messages = self.messages.clone();
                    tokio::task::spawn_blocking(move || {
                        let notice = match crate::activity::export(&path, &bytes) {
                            Ok(()) => format!(tr!(activity_exported), path = path.display()),
                            Err(error) => format!(tr!(activity_export_error), error = error),
                        };
                        let _ = messages.send(Msg::Notice(notice));
                    });
                }
                Effect::SaveGame2048 => {
                    if let (Some(dir), Some(game)) = (&self.state_dir, &self.state.game2048)
                        && let Err(error) = dir.save_game2048(game)
                    {
                        self.state.notice = Some(format!(tr!(game2048_save_error), error = error));
                    }
                }
                Effect::Search { side, generation, request } => self.search.search(side, generation, request),
                Effect::RefreshCollection { side, generation, entries } => {
                    self.search.refresh(side, generation, entries)
                }
                Effect::CancelSearch(side) => self.search.cancel(side),
                Effect::SaveCollections => {
                    if !self.collections_writable {
                        self.state.notice = Some(tr!(collections_preserved).into());
                        continue;
                    }
                    if let Some(dir) = &self.state_dir
                        && let Err(error) = dir.save_collections(&self.state.collections)
                    {
                        self.state.notice = Some(format!(tr!(collections_save_error), error = error));
                    }
                }
                Effect::List { side, generation, dir } => {
                    self.search.cancel(side);
                    self.list(side, generation, dir);
                }
                Effect::GitScan { side, generation, dir } => self.plugins.scan(side, generation, dir),
                Effect::GitTool { id, tool, dir, paths } => {
                    let messages = self.messages.clone();
                    let size = viewer_size(self.state.cols, self.state.rows);
                    tokio::spawn(async move {
                        let result =
                            crate::plugins::run(dir, tool, paths, size).await.map_err(|error| format!("{error:#}"));
                        let _ = messages.send(Msg::GitFinished { id, result });
                    });
                }
                Effect::Write(bytes) => {
                    let (emulator, pty) = self.console();
                    emulator.scroll(Scroll::Bottom);
                    pty.write(&bytes)?;
                }
                Effect::ChangeShellDir { dir, typed } => {
                    self.emulator.scroll(Scroll::Bottom);
                    match borg_term::request_cd(&self.cd_request, &dir) {
                        Ok(()) => self.pty.write(borg_term::SILENT_CD_KEY)?,
                        Err(_) => self.pty.write(&typed)?,
                    }
                }
                Effect::ScrollConsole(scroll) => self.console().0.scroll(scroll),
                Effect::UseConsole(console) => self.console = console,
                Effect::AgentAnswer { id, approved } => self.agent.answer(id, approved, self.messages.clone()),
                Effect::View(path) => self.open_viewer(path),
                Effect::SaveConfig => self.save_config(),
                Effect::StartJob { id, spec } => self.start_job(id, spec),
                Effect::StartDockerCopy { id, container, inbound, sources, target } => {
                    self.start_docker_copy(id, crate::docker::CopyJob { container, inbound, sources, target });
                }
                Effect::JobControl { id, command } => self.control_job(id, command),
                Effect::Peek { aside, path, size } => self.peek(aside, path, size),
                Effect::Quit => return Ok(Flow::Quit),
            }
        }
        Ok(Flow::Continue)
    }

    fn container_shell(&mut self, request: borg_core::ShellRequest) {
        use borg_core::ShellRequest;
        match request {
            ShellRequest::Open { container, prompt, size, workdir } => {
                if let Err(error) = self.shells.open(container.clone(), &prompt, size, workdir.as_deref()) {
                    let _ = self.messages.send(Msg::Notice(format!("{error:#}")));
                    let _ = self.messages.send(Msg::DockerShellClosed { container });
                }
            }
            ShellRequest::Input { container, bytes } => self.shells.input(&container, &bytes),
            ShellRequest::Scroll { container, scroll } => self.shells.scroll(&container, scroll),
        }
    }

    fn start_job(&mut self, id: u64, spec: Spec) {
        let cache = self.search.cache.clone();
        let mut changed = match &spec {
            Spec::Copy { .. } => Vec::new(),
            _ => spec.sources().to_vec(),
        };
        if let Spec::Copy { target, .. } | Spec::Move { target, .. } = &spec {
            changed.push(target.clone());
        }
        cache.begin_changes();
        let running: Vec<u64> = self.state.jobs.iter().map(|job| job.id).collect();
        self.jobs.retain(|id, _| running.contains(id));
        let control = Arc::new(Control::default());
        self.jobs.insert(id, Arc::clone(&control));
        let messages = self.messages.clone();
        tokio::task::spawn_blocking(move || {
            run(&spec, &control, &mut |event| {
                if matches!(event, borg_jobs::Event::Finished(_)) {
                    cache.invalidate(&changed);
                }
                drop(messages.send(Msg::Job { id, event }));
            });
        });
    }

    fn start_docker_copy(&mut self, id: u64, job: crate::docker::CopyJob) {
        let running: Vec<u64> = self.state.jobs.iter().map(|job| job.id).collect();
        self.jobs.retain(|id, _| running.contains(id));
        let control = Arc::new(Control::default());
        self.jobs.insert(id, Arc::clone(&control));
        let messages = self.messages.clone();
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            let outcome = job.run(&runtime, &control, &mut |event| drop(messages.send(Msg::Job { id, event })));
            drop(messages.send(Msg::Job { id, event: borg_jobs::Event::Finished(outcome) }));
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
        let messages = self.messages.clone();
        tokio::task::spawn_blocking(move || {
            let content = crate::document::open(
                &path,
                (width, height),
                (cell.width, cell.height),
                Arc::new(AtomicBool::new(false)),
            );
            let opened = content
                .map(|content| Msg::Opened(Viewer { path: path.clone(), content }))
                .unwrap_or_else(|e| Msg::Notice(format!("{}: {e:#}", path.display())));
            let _ = messages.send(opened);
        });
    }

    fn peek(&mut self, aside: Aside, path: PathBuf, size: (usize, usize)) {
        let cell = self.graphics.capabilities().cell;
        self.preview.request(aside, path, size, (cell.width, cell.height));
    }

    fn list(&mut self, side: Side, generation: u64, dir: PathBuf) {
        let mount = self.state.panel(side).mount.clone();
        let mut batches = match &mount {
            Some(mount) => crate::docker::listing(mount.id.clone(), dir.clone()),
            None => self.backend.list(&dir),
        };
        let messages = self.messages.clone();
        // Measured folder sizes belong to this machine, not to a container.
        let cache = mount.is_none().then(|| self.search.cache.clone());
        let task = tokio::spawn(async move {
            let observed_dir = dir.clone();
            let totals = match cache {
                Some(cache) => {
                    tokio::task::spawn_blocking(move || cache.totals(&observed_dir)).await.unwrap_or_default()
                }
                None => Default::default(),
            };
            while let Some(batch) = batches.next().await {
                let msg = match batch {
                    Ok(mut entries) => {
                        for entry in entries.iter_mut().filter(|e| e.kind == borg_vfs::EntryKind::Dir) {
                            if let Some((bytes, usage)) = totals.get(&entry.path(&dir)) {
                                entry.size = *bytes;
                                entry.usage = Some(usage.clone());
                            }
                        }
                        Msg::Batch { side, generation, entries }
                    }
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

fn appearance(launch: &Launch) -> Appearance {
    launch.themes.appearance(launch.config.theme.as_deref(), launch.config.unlocked())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wheel_burst_stays_with_what_it_started_scrolling() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut wheel = WheelBurst::default();
        assert!(wheel.accepts(WheelTarget::Console, at(0)));
        assert!(wheel.accepts(WheelTarget::Console, at(16)));
        assert!(!wheel.accepts(WheelTarget::Panels, at(32)), "momentum after switching screens is dropped");
        assert!(!wheel.accepts(WheelTarget::Panels, at(200)));
        assert!(wheel.accepts(WheelTarget::Panels, at(200) + WHEEL_BURST + Duration::from_millis(1)));
    }
}
