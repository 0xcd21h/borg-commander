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
    event::{
        DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyEventKind, MouseButton, MouseEvent,
        MouseEventKind,
    },
    execute, queue,
    terminal::{
        BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate, EnterAlternateScreen, LeaveAlternateScreen,
    },
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
    /// Agent reveals waiting for their panel to show the folder.
    reveals: Vec<(Side, PathBuf, tokio::sync::oneshot::Sender<crate::agent::Reply>, String)>,
    /// An agent's command typed into the host console, until the shell reports its end.
    host_capture: Option<(crate::agent::Typed, Vec<u8>)>,
    /// Text to put on the user's clipboard with the next frame.
    clipboard: Option<String>,
    /// Text shown alone in the terminal, outside borg's screen, for the user to select.
    plain: Option<Plain>,
    /// The terminal draws emoji two columns wide, as Borg lays them out.
    wide_emoji: bool,
    /// Borg cloud: this borg reachable from paired computers through a relay.
    link: crate::link::LinkServer,
    /// Files arriving from another computer, by token.
    uploads: HashMap<String, crate::agent::Upload>,
    /// Files another computer may download in pieces without asking again, by computer
    /// and path, once the user approved the first piece.
    downloads: std::collections::HashSet<(String, String)>,
    runtime_dir: PathBuf,
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
        state.agent.connect = crate::agent::connect_info();
        let relay = launch.config.relay.as_deref().or(crate::link::DEFAULT_RELAY).and_then(borg_link::Relay::parse);
        state.agent.connect.relay = relay.as_ref().map(ToString::to_string).unwrap_or_default();
        let link = crate::link::LinkServer::new(
            launch.state_dir.as_ref().map(|dir| dir.path().to_path_buf()),
            relay,
            agent.requeue_sender(),
            messages.clone(),
            agent.clients(),
        );
        state.agent.computers = link.computers();
        let runtime_dir = launch.runtime_dir.clone();
        tokio::spawn(async move {
            let mut refresh = tokio::time::interval(crate::launch::TTL_REFRESH);
            loop {
                refresh.tick().await;
                let _ = crate::launch::mark_in_use(&runtime_dir);
            }
        });
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
            reveals: Vec::new(),
            link,
            uploads: HashMap::new(),
            downloads: std::collections::HashSet::new(),
            runtime_dir: launch.runtime_dir.clone(),
            host_capture: None,
            clipboard: None,
            plain: None,
            wide_emoji: detected.wide_emoji,
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
        match self.plain.take() {
            Some(Plain::Waiting(text)) => {
                show_plain(terminal.backend_mut(), &text)?;
                self.plain = Some(Plain::Shown);
                return Ok(());
            }
            Some(Plain::Shown) => {
                self.plain = Some(Plain::Shown);
                return Ok(());
            }
            Some(Plain::Ending) => {
                // Cleared here and drawn whole: Terminal::clear would ask the terminal where
                // its cursor is, and not every terminal answers.
                execute!(terminal.backend_mut(), EnterAlternateScreen, EnableMouseCapture, Clear(ClearType::All))?;
                terminal.swap_buffers();
                terminal.swap_buffers();
            }
            None => {}
        }
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
        if let Some(text) = self.clipboard.take() {
            // OSC 52 reaches the clipboard of the computer the user sits at, over SSH too.
            use base64::Engine as _;
            write!(backend, "\x1b]52;c;{}\x07", base64::engine::general_purpose::STANDARD.encode(text))?;
        }
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
            placements = draw(frame, &self.scene(clock, now));
        })?;
        resized?;
        Ok(self.graphics.frame(&placements, frame.buffer))
    }

    fn scene<'a>(&'a self, clock: &'a Zoned, now: Instant) -> Scene<'a> {
        Scene {
            state: &self.state,
            screen: match &self.console {
                borg_core::Console::Container(id) => self.shells.screen(id).unwrap_or_else(|| self.emulator.screen()),
                borg_core::Console::Host => self.emulator.screen(),
            },
            docker_shell: self.state.docker.visible_shell().and_then(|shell| self.shells.screen(&shell.container)),
            palette: &self.palette,
            themes: &self.themes,
            clock,
            now,
            home: self.home.as_deref(),
            pixels: pixels(self.graphics.capabilities()),
        }
    }

    /// The screen as text, drawn now as the next frame will show it, so an agent sees what
    /// its last call changed.
    fn screen_text(&self) -> Vec<String> {
        let (clock, now) = (local_now(), Instant::now());
        let backend = ratatui::backend::TestBackend::new(self.state.cols, self.state.rows);
        let Ok(mut terminal) = ratatui::Terminal::new(backend);
        if terminal.draw(|frame| drop(draw(frame, &self.scene(&clock, now)))).is_err() {
            return Vec::new();
        }
        frame_text(terminal.backend().buffer())
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
        // While text is shown plainly, a key brings borg back and does nothing else.
        if self.plain.is_some() {
            if let Event::Key(key) = event
                && key.kind == KeyEventKind::Press
            {
                self.plain = Some(Plain::Ending);
            }
            return Ok(Flow::Continue);
        }
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
            let target = if self.state.effective_view() == View::Shell && self.state.overlay.is_none() {
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
                if let Some((_, captured)) = self.host_capture.as_mut() {
                    captured.extend_from_slice(&bytes);
                }
                let signals = self.emulator.feed(&bytes);
                for reply in self.emulator.drain_replies() {
                    self.pty.write(reply.as_bytes())?;
                }
                let ended = signals.iter().find_map(|signal| match signal {
                    borg_term::ShellSignal::CommandEnd { exit } => Some(*exit),
                    _ => None,
                });
                let console = self.console == borg_core::Console::Host;
                for signal in signals {
                    let msg = if console { Msg::Signal(signal) } else { Msg::ParkedSignal { container: None, signal } };
                    if let Flow::Quit = self.dispatch(msg)? {
                        return Ok(Flow::Quit);
                    }
                }
                if let (Some(exit), Some((command, captured))) = (ended, self.host_capture.take_if(|_| ended.is_some()))
                {
                    let output = crate::agent::shell_output(&captured, &command.line);
                    command.finish(exit, &output, &self.messages);
                }
                Ok(Flow::Continue)
            }
            PtyEvent::Closed => {
                if let Some((command, _)) = self.host_capture.take() {
                    command.fail("The shell closed before the command finished.", &self.messages);
                }
                self.dispatch(Msg::ShellExited)
            }
        }
    }

    /// Answers an agent's tool call from what the session knows. Reading happens off the
    /// event loop, a reveal answers once the panel shows it, and commands and changes to
    /// files wait for the user's approval.
    fn on_agent(
        &mut self,
        call: crate::agent::Call,
        reply: tokio::sync::oneshot::Sender<crate::agent::Reply>,
    ) -> Result<Flow> {
        use crate::agent::{self, Location, Reply, Spot, Target, Work};
        use borg_core::{AgentEvent, Ask, AskKind, DiffLine};
        let text = |key: &str| call.args[key].as_str().map(str::to_owned).filter(|value| !value.trim().is_empty());
        let fail = |reply: tokio::sync::oneshot::Sender<Reply>, text: String| {
            drop(reply.send(Reply::error(text)));
            Ok(Flow::Continue)
        };
        // With the user's say-so, what another computer reads waits for approval too: once per
        // file for a download, which comes in pieces.
        if let Some(origin) = call.origin.clone()
            && !call.approved
            && self.state.settings.preferences.agent_ask_reads
            && matches!(call.tool.as_str(), "screen" | "read" | "get")
            && !(call.tool == "get" && self.downloads.contains(&(origin.clone(), text("path").unwrap_or_default())))
        {
            let summary = match call.tool.as_str() {
                "screen" => "what borg shows".to_owned(),
                "get" => format!("download {}", text("path").unwrap_or_default()),
                _ => text("source").or_else(|| text("path")).unwrap_or_else(|| ".".into()),
            };
            let place = tr!(agent_asked_from).replace("{origin}", &origin);
            let ask =
                Ask { kind: AskKind::Read, summary: summary.clone(), place, body: vec![DiffLine::plain(summary)] };
            let id = self.agent.ask(reply, Work::Forward(call));
            return self.dispatch(Msg::Agent(AgentEvent::Ask { id, ask }));
        }
        match call.tool.as_str() {
            crate::link::PAIR_CALL => {
                let name = text("name").unwrap_or_default();
                // Emoji only where the terminal draws them as wide as they are laid out.
                let code =
                    call.args[if self.wide_emoji { "code" } else { "names" }].as_array().cloned().unwrap_or_default();
                let mut body = vec![DiffLine::plain(format!("Computer: {name}")), DiffLine::plain("")];
                body.extend(code.iter().filter_map(|row| row.as_str()).map(|row| DiffLine::plain(format!("  {row}"))));
                body.extend([
                    DiffLine::plain(""),
                    DiffLine::plain("Pair only if `borg pair` shows this code on that computer."),
                ]);
                let ask = Ask { kind: AskKind::Pair, summary: name.clone(), place: String::new(), body };
                let id = self.agent.ask(reply, Work::Pair { name });
                return self.dispatch(Msg::Agent(AgentEvent::Ask { id, ask }));
            }
            "get" => {
                let Some(path) = text("path") else { return fail(reply, "get needs a path".into()) };
                let offset = call.args["offset"].as_u64().unwrap_or(0);
                let location = agent::locate(&self.state, &path);
                let Some((spot, _, reply)) = self.spot(&location, &call, reply) else { return Ok(Flow::Continue) };
                if offset == 0 {
                    self.agent.note(&format!("downloading {}", spot.uri()));
                }
                tokio::spawn(async move {
                    let piece = agent::piece(&spot, offset).await;
                    drop(reply.send(match piece {
                        Ok(piece) => Reply::ok(piece),
                        Err(error) => Reply::error(format!("{}: {error:#}", spot.uri())),
                    }));
                });
            }
            "put" => return self.put(call, reply),
            "screen" => {
                self.agent.note("looked at the screen");
                let (emulator, _) = self.console();
                let console =
                    agent::ConsoleText { lines: emulator.screen().live_text(), typed: emulator.command_line() };
                drop(reply.send(Reply::ok(agent::screen(&self.state, &self.screen_text(), &console))));
            }
            "read" => match text("source").as_deref() {
                Some("processes") => {
                    const SHOWN: u64 = 40;
                    let by_cpu = text("sort").as_deref() == Some("cpu");
                    let shown = call.args["limit"].as_u64().unwrap_or(SHOWN).clamp(1, borg_core::PROCESS_LIMIT as u64);
                    self.agent.note("read the processes");
                    tokio::task::spawn_blocking(move || {
                        drop(reply.send(agent::processes(&crate::activity::processes(), by_cpu, shown as usize)));
                    });
                }
                Some("containers") => {
                    self.agent.note("read the containers");
                    tokio::spawn(async move { drop(reply.send(agent::containers().await)) });
                }
                Some(other) => {
                    return fail(reply, format!("read has no source {other}: use processes or containers, or a path"));
                }
                None => {
                    let lines = match text("lines").map(|lines| agent::parse_lines(&lines)).transpose() {
                        Ok(lines) => lines,
                        Err(error) => return fail(reply, error),
                    };
                    let find = call.args["find"].as_str().filter(|find| !find.is_empty()).map(str::to_owned);
                    let location = agent::locate(&self.state, &text("path").unwrap_or_else(|| ".".into()));
                    let Some((spot, _, reply)) = self.spot(&location, &call, reply) else { return Ok(Flow::Continue) };
                    self.agent.note(&format!("read {}", spot.uri()));
                    tokio::spawn(async move { drop(reply.send(agent::read(spot, lines, find).await)) });
                }
            },
            "show" => {
                let Some(path) = text("path") else { return fail(reply, "show needs a path".into()) };
                let location = agent::locate(&self.state, &path);
                let Some((spot, _, reply)) = self.spot(&location, &call, reply) else { return Ok(Flow::Continue) };
                // A folder opens and a file is selected in its folder; a container is asked which.
                let requeue = self.agent.requeue_sender();
                tokio::spawn(async move {
                    let folder = agent::is_folder(&spot).await;
                    let args = serde_json::json!({ "spot": spot, "folder": folder });
                    drop(requeue.send((call.follow("_show", args), reply)));
                });
            }
            "_show" => {
                let Ok(spot) = serde_json::from_value::<Spot>(call.args["spot"].clone()) else {
                    return fail(reply, "unreadable location".into());
                };
                let path = spot.path().to_path_buf();
                let (dir, focus) = if call.args["folder"] == true {
                    (path, None)
                } else {
                    let parent = path.parent().map_or_else(|| PathBuf::from("/"), Path::to_path_buf);
                    (parent, path.file_name().map(|name| name.to_string_lossy().into_owned()))
                };
                let mount = match &spot {
                    Spot::Container { id, name, .. } => Some((id.clone(), name.clone())),
                    Spot::Host(_) => None,
                };
                self.agent.note(&format!("showed {}", spot.uri()));
                let flow = self.dispatch(Msg::Agent(AgentEvent::Reveal { dir, focus, mount }))?;
                let side = self.state.active.other();
                let shown = self.state.panel(side).destination().to_path_buf();
                self.reveals.push((side, shown, reply, spot.uri()));
                self.settle_reveals();
                return Ok(flow);
            }
            "write" | "edit" => {
                let edit = call.tool == "edit";
                let Some(path) = text("path") else { return fail(reply, format!("{} needs a path", call.tool)) };
                let (content, old, new) =
                    (call.args["content"].as_str(), call.args["old"].as_str(), call.args["new"].as_str());
                let change = match (edit, content, old, new) {
                    (false, Some(content), ..) => Err(content.to_owned()),
                    (true, _, Some(old), Some(new)) if !old.is_empty() => Ok((old.to_owned(), new.to_owned())),
                    (false, ..) => return fail(reply, "write needs content".into()),
                    (true, ..) => return fail(reply, "edit needs old (not empty) and new".into()),
                };
                let all = call.args["all"] == true;
                let location = agent::locate(&self.state, &path);
                let Some((spot, _, reply)) = self.spot(&location, &call, reply) else { return Ok(Flow::Continue) };
                let requeue = self.agent.requeue_sender();
                tokio::spawn(async move {
                    let uri = spot.uri();
                    let before = match agent::load(&spot).await {
                        Ok(before) => before,
                        Err(error) => return drop(reply.send(Reply::error(format!("{uri}: {error:#}")))),
                    };
                    let after = match (change, &before) {
                        (Err(content), _) => content,
                        (Ok(_), None) => {
                            return drop(
                                reply.send(Reply::error(format!("{uri} does not exist; write creates files"))),
                            );
                        }
                        (Ok((old, new)), Some(text)) => match text.matches(old.as_str()).count() {
                            0 => {
                                return drop(reply.send(Reply::error(format!(
                                    "old was not found in {uri}: it must match exactly, spaces and line breaks included. Read the lines again."
                                ))));
                            }
                            count if count > 1 && !all => {
                                return drop(reply.send(Reply::error(format!(
                                    "old matches {count} places in {uri}: include more of the lines around it, or set all"
                                ))));
                            }
                            _ if all => text.replace(old.as_str(), &new),
                            _ => text.replacen(old.as_str(), &new, 1),
                        },
                    };
                    let args = serde_json::json!({ "spot": spot, "before": before, "after": after });
                    drop(requeue.send((call.follow("_propose", args), reply)));
                });
            }
            "_propose" => {
                let (Ok(spot), Some(after)) =
                    (serde_json::from_value::<Spot>(call.args["spot"].clone()), call.args["after"].as_str())
                else {
                    return fail(reply, "unreadable change".into());
                };
                let (before, after) = (call.args["before"].as_str().map(str::to_owned), after.to_owned());
                let body = match &before {
                    Some(old) => agent::diff(old, &after),
                    None => agent::whole(&after),
                };
                if before.is_some() && body.is_empty() {
                    drop(reply.send(Reply::ok(format!("{} already holds that; nothing changed", spot.uri()))));
                    return Ok(Flow::Continue);
                }
                let (summary, place) = match &spot {
                    Spot::Host(path) => (agent::home_relative(path), String::new()),
                    Spot::Container { name, path, .. } => {
                        (path.display().to_string(), format!(tr!(agent_container_place), name = name))
                    }
                };
                let kind = if before.is_some() { AskKind::Edit } else { AskKind::Create };
                let place = from_origin(place, call.origin.as_deref());
                let ask = Ask { kind, summary, place, body };
                let id = self.agent.ask(reply, Work::Write { spot, before, after });
                return self.dispatch(Msg::Agent(AgentEvent::Ask { id, ask }));
            }
            "run" => {
                let Some(command) = text("command") else { return fail(reply, "run needs a command".into()) };
                let at = text("at");
                let mut reply = reply;
                let target = match at.as_deref().map(|at| agent::locate(&self.state, at)) {
                    None => Target::Host(agent::host_dir(&self.state)),
                    Some(Location::Host(dir)) => Target::Host(dir),
                    Some(location) => {
                        let Some((spot, running, back)) = self.spot(&location, &call, reply) else {
                            return Ok(Flow::Continue);
                        };
                        reply = back;
                        let Spot::Container { id, name, path } = spot else {
                            return fail(reply, "unreadable location".into());
                        };
                        if !running {
                            return fail(reply, format!("{name} is not running."));
                        }
                        if borg_core::container_shell_busy(&self.state, &id) {
                            return fail(
                                reply,
                                format!(
                                    "The shell in {name} is running something; try again when it is back at its prompt."
                                ),
                            );
                        }
                        // A folder runs the command there; the container alone keeps the shell's.
                        let named = at
                            .as_deref()
                            .and_then(|at| at.trim().strip_prefix(agent::DOCKER_SCHEME))
                            .is_some_and(|rest| rest.contains('/'));
                        Target::Container { id, name, dir: named.then_some(path) }
                    }
                };
                if matches!(target, Target::Host(_))
                    && self.console == borg_core::Console::Host
                    && self.state.shell.integrated
                    && !borg_core::host_console_ready(&self.state)
                {
                    return fail(
                        reply,
                        "Borg's command line is busy: the user is typing there, or a program runs. Try again when it is back at its prompt.".into(),
                    );
                }
                let ask = Ask {
                    kind: AskKind::Run,
                    summary: command.clone(),
                    place: from_origin(agent::place(&target), call.origin.as_deref()),
                    body: command.lines().map(DiffLine::plain).collect(),
                };
                let id = self.agent.ask(reply, Work::Run { command, target });
                return self.dispatch(Msg::Agent(AgentEvent::Ask { id, ask }));
            }
            "type" => {
                let text = agent::typable(call.args["text"].as_str().unwrap_or_default());
                if text.trim().is_empty() {
                    return fail(reply, "type needs text".into());
                }
                // At a prompt without paste support, a tab would complete; it types as a space.
                let pastes = self.state.shell.observed.mode.contains(borg_term::TermMode::BRACKETED_PASTE);
                let text = if pastes { text } else { text.replace('\t', " ") };
                if let Some(reason) = borg_core::typing_refusal(&self.state, &text) {
                    return fail(reply, reason.into());
                }
                self.agent.note(&format!("typed into the console\n{text}"));
                let flow = self.dispatch(Msg::Agent(AgentEvent::Paste { text }))?;
                drop(reply.send(Reply::ok(
                    "Typed at the cursor in Borg's console; nothing ran. The user reviews it and runs or saves it.",
                )));
                return Ok(flow);
            }
            other => return fail(reply, format!("unknown tool {other}")),
        }
        Ok(Flow::Continue)
    }

    /// An upload from another computer: the first call announces the file and waits for the
    /// user's approval; the calls after it bring its pieces, the last one marked done.
    fn put(
        &mut self,
        call: crate::agent::Call,
        reply: tokio::sync::oneshot::Sender<crate::agent::Reply>,
    ) -> Result<Flow> {
        use crate::agent::{self, Reply, Work};
        use base64::Engine as _;
        use borg_core::{AgentEvent, Ask, AskKind, DiffLine};
        let Some(token) = call.args["token"].as_str().map(str::to_owned) else {
            let (Some(path), Some(size)) = (call.args["path"].as_str(), call.args["size"].as_u64()) else {
                drop(reply.send(Reply::error("put needs a path and a size")));
                return Ok(Flow::Continue);
            };
            let location = agent::locate(&self.state, path);
            let Some((spot, _, reply)) = self.spot(&location, &call, reply) else { return Ok(Flow::Continue) };
            // Small sizes show as bytes; larger ones as the panels show them, with the bytes.
            let shown = if size < 10_000 {
                format!("{size} bytes")
            } else {
                format!("{} ({size} bytes)", borg_ui::size_text(size))
            };
            let place = from_origin(shown.clone(), call.origin.as_deref());
            let body = vec![DiffLine::plain(spot.uri()), DiffLine::plain(shown)];
            let ask = Ask { kind: AskKind::Upload, summary: spot.uri(), place, body };
            let id = self.agent.ask(reply, Work::Upload { spot, size });
            return self.dispatch(Msg::Agent(AgentEvent::Ask { id, ask }));
        };
        let Some(upload) = self.uploads.get_mut(&token) else {
            drop(reply.send(Reply::error("no such upload; start it again")));
            return Ok(Flow::Continue);
        };
        let data = base64::engine::general_purpose::STANDARD.decode(call.args["data"].as_str().unwrap_or_default());
        let written = data.map_err(anyhow::Error::from).and_then(|data| upload.write(&data));
        if let Err(error) = written {
            self.uploads.remove(&token);
            drop(reply.send(Reply::error(format!("{error:#}"))));
            return Ok(Flow::Continue);
        }
        if call.args["done"] != true {
            drop(reply.send(Reply::ok("")));
            return Ok(Flow::Continue);
        }
        let Some(upload) = self.uploads.remove(&token) else { return Ok(Flow::Continue) };
        let uri = upload.spot.uri();
        self.agent.note(&format!("uploaded {uri}"));
        let messages = self.messages.clone();
        tokio::spawn(async move {
            let finished = upload.finish().await;
            drop(reply.send(match finished {
                Ok(size) => Reply::ok(format!("Uploaded {uri} ({size} bytes)")),
                Err(error) => Reply::error(format!("{uri}: {error:#}")),
            }));
            drop(
                messages
                    .send(Msg::Agent(AgentEvent::Finished { id: 0, notice: format!(tr!(agent_wrote), path = uri) })),
            );
        });
        Ok(Flow::Continue)
    }

    /// The spot a location names, and whether its container runs, when the container is
    /// known now: found earlier for this call, or in the Docker view's list. Otherwise the
    /// call comes back once Docker has listed the containers, or fails if none matches.
    fn spot(
        &self,
        location: &crate::agent::Location,
        call: &crate::agent::Call,
        reply: tokio::sync::oneshot::Sender<crate::agent::Reply>,
    ) -> Option<(crate::agent::Spot, bool, tokio::sync::oneshot::Sender<crate::agent::Reply>)> {
        use crate::agent::{self, Location, Reply, Spot};
        let (container, path) = match location {
            Location::Host(path) => return Some((Spot::Host(path.clone()), true, reply)),
            Location::Container { container, path } => (container.clone(), path.clone()),
        };
        let known = &call.args["_container"];
        if let (Some(id), Some(name)) = (known["id"].as_str(), known["name"].as_str()) {
            return Some((Spot::Container { id: id.into(), name: name.into(), path }, known["running"] == true, reply));
        }
        if let Some(found) = agent::find_container(&self.state.docker.containers, &container) {
            let running = found.state == borg_docker::State::Running;
            return Some((Spot::Container { id: found.id.clone(), name: found.name.clone(), path }, running, reply));
        }
        let requeue = self.agent.requeue_sender();
        let mut call = call.clone();
        tokio::spawn(async move {
            let containers = borg_docker::list().await.unwrap_or_default();
            match agent::find_container(&containers, &container) {
                Some(found) => {
                    let running = found.state == borg_docker::State::Running;
                    call.args["_container"] =
                        serde_json::json!({ "id": found.id, "name": found.name, "running": running });
                    drop(requeue.send((call, reply)));
                }
                None => drop(reply.send(Reply::error(format!(
                    "No container named {container}. read with source \"containers\" lists them."
                )))),
            }
        });
        None
    }

    /// Answers reveals once their panel shows the folder, or is going somewhere else.
    fn settle_reveals(&mut self) {
        let state = &self.state;
        let (done, waiting): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.reveals).into_iter().partition(|(side, dir, ..)| {
                let panel = state.panel(*side);
                panel.destination() != dir || (panel.dir() == dir && *panel.listing() != borg_core::Listing::Loading)
            });
        self.reveals = waiting;
        for (side, dir, reply, path) in done {
            let panel = self.state.panel(side);
            let answer = match panel.listing() {
                _ if panel.destination() != dir => {
                    crate::agent::Reply::ok(format!("Showed {path}; the panel has moved on since"))
                }
                borg_core::Listing::Failed(error) => crate::agent::Reply::error(format!("{path}: {error}")),
                _ => crate::agent::Reply::ok(format!("Showing {path} in Borg's other panel")),
            };
            drop(reply.send(answer));
        }
    }

    /// Carries out an approved ask: a command typed into the host console (or run in the
    /// background where that console cannot take it) or into a container's shell, which the
    /// Docker view then shows, or a change written to a file.
    fn agent_answer(&mut self, id: u64, approved: bool) -> Result<Flow> {
        use crate::agent::{Approved, Target, Typed, Work};
        let Some(Approved { id, reply, work }) = self.agent.answer(id, approved) else { return Ok(Flow::Continue) };
        let (command, target) = match work {
            Work::Write { spot, before, after } => {
                self.agent.write(id, reply, spot, before, after);
                return Ok(Flow::Continue);
            }
            Work::Pair { .. } => {
                drop(reply.send(crate::agent::Reply::ok("approved")));
                return Ok(Flow::Continue);
            }
            Work::Forward(mut call) => {
                if call.tool == "get"
                    && let (Some(origin), Some(path)) = (call.origin.clone(), call.args["path"].as_str())
                {
                    self.downloads.insert((origin, path.to_owned()));
                }
                call.approved = true;
                drop(self.agent.requeue_sender().send((call, reply)));
                return Ok(Flow::Continue);
            }
            Work::Upload { spot, size } => {
                let token = borg_link::hex(&borg_link::secret());
                let scratch = self.runtime_dir.join(DOCKER_SCRATCH);
                drop(reply.send(match crate::agent::Upload::begin(spot, size, &scratch, &token) {
                    Ok(upload) => {
                        self.uploads.insert(token.clone(), upload);
                        crate::agent::Reply::ok(token)
                    }
                    Err(error) => crate::agent::Reply::error(format!("{error:#}")),
                }));
                return Ok(Flow::Continue);
            }
            Work::Run { command, target } => (command, target),
        };
        let place = crate::agent::place(&target);
        match target {
            Target::Host(dir) if self.console == borg_core::Console::Host && self.state.shell.integrated => {
                let line = if dir == self.state.shell.cwd {
                    command.clone()
                } else {
                    format!("cd -- {} && {command}", borg_core::shell_quote(&dir.to_string_lossy()))
                };
                let typed = Typed { id, reply, line: line.clone(), command, place };
                if !borg_core::host_console_ready(&self.state) {
                    typed.fail(
                        "Borg's command line became busy before the command could run; try again.",
                        &self.messages,
                    );
                    return Ok(Flow::Continue);
                }
                self.host_capture = Some((typed, Vec::new()));
                self.dispatch(Msg::Agent(borg_core::AgentEvent::Type { line }))
            }
            Target::Host(dir) => {
                self.agent.run_in_background(id, reply, command, dir);
                Ok(Flow::Continue)
            }
            Target::Container { id: container, name, dir } => {
                let line = match dir {
                    Some(dir) => format!("cd -- {} && {command}", borg_core::shell_quote(&dir.to_string_lossy())),
                    None => command.clone(),
                };
                let typed = Typed { id, reply, line, command, place };
                let flow =
                    self.dispatch(Msg::Agent(borg_core::AgentEvent::ShowShell { container: container.clone(), name }))?;
                if let Err(typed) = self.shells.run(&container, typed) {
                    typed.fail("Borg could not open a shell in that container.", &self.messages);
                }
                Ok(flow)
            }
        }
    }

    fn on_container_shell(&mut self, container: String, event: PtyEvent) -> Result<Flow> {
        match event {
            PtyEvent::Output(bytes) => {
                let Some((mode, signals, captured)) = self.shells.feed(&container, &bytes) else {
                    return Ok(Flow::Continue);
                };
                if let Some(crate::docker::Captured { command, exit, output }) = captured {
                    command.finish(exit, &output, &self.messages);
                }
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
                if let Some(typed) = self.shells.close(&container) {
                    typed.fail("The shell closed before the command finished.", &self.messages);
                }
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
        self.settle_reveals();
        self.link.sync(self.state.settings.preferences.agent_cloud);
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
                Effect::Copy(text) => self.clipboard = Some(text),
                Effect::ShowPlain(text) => self.plain = Some(Plain::Waiting(text)),
                Effect::StartPairing => self.start_pairing(),
                Effect::SetRelay(text) => self.set_relay(&text),
                Effect::ForgetComputers => {
                    if let Err(error) = self.link.forget() {
                        self.state.notice = Some(format!("{error:#}"));
                    }
                }
                Effect::AgentAnswer { id, approved } => {
                    if let Flow::Quit = self.agent_answer(id, approved)? {
                        return Ok(Flow::Quit);
                    }
                }
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

    /// Makes a pairing link for the Connect dialog, or says why there is none.
    fn start_pairing(&mut self) {
        let pairing = if !self.state.settings.preferences.agent_cloud {
            Err(tr!(agent_cloud_off).to_owned())
        } else if self.link.relay().is_none() {
            Err(tr!(agent_cloud_no_relay).to_owned())
        } else {
            self.link.pair().map_err(|error| format!("{error:#}"))
        };
        self.state.agent.pairing = Some(pairing);
        self.link.sync(self.state.settings.preferences.agent_cloud);
    }

    /// Saves the relay typed into the Connect dialog, empty for none, in its full form, and
    /// asks it for a pairing link.
    fn set_relay(&mut self, text: &str) {
        let relay = borg_link::Relay::parse(text);
        if relay.is_none() && !text.is_empty() {
            self.state.agent.connect.relay = self.link.relay().map(ToString::to_string).unwrap_or_default();
            self.state.agent.pairing = Some(Err(tr!(agent_cloud_bad_relay).to_owned()));
            return;
        }
        let saved = relay.as_ref().map(ToString::to_string);
        self.state.agent.connect.relay = saved.clone().unwrap_or_default();
        if let Some(borg_core::Overlay::Connect(connect)) = self.state.overlay.as_mut() {
            connect.relay = borg_core::LineInput::new(self.state.agent.connect.relay.clone());
        }
        self.config.relay = saved;
        self.save_config();
        self.link.set_relay(relay);
        self.start_pairing();
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

/// Where an ask happens, with the computer that asked when it came through borg cloud.
fn from_origin(place: String, origin: Option<&str>) -> String {
    match origin {
        None => place,
        Some(origin) if place.is_empty() => tr!(agent_asked_from).replace("{origin}", origin),
        Some(origin) => tr!(agent_from).replace("{place}", &place).replace("{origin}", origin),
    }
}

/// A drawn frame's text row by row, a wide character once, trailing spaces left out.
fn frame_text(buffer: &ratatui::buffer::Buffer) -> Vec<String> {
    let area = buffer.area;
    (area.top()..area.bottom())
        .map(|y| {
            let row: String =
                (area.left()..area.right()).filter_map(|x| buffer.cell((x, y))).map(|cell| cell.symbol()).collect();
            row.trim_end().to_owned()
        })
        .collect()
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

/// Where text shown plainly stands: waiting for the next frame, on the terminal, or about to
/// give way to borg's screen again.
enum Plain {
    Waiting(String),
    Shown,
    Ending,
}

/// Leaves borg's screen and lets the mouse select again, then prints `text` as one line, which
/// the terminal wraps itself, so a selection copies it whole and without borg's frames.
fn show_plain(out: &mut impl std::io::Write, text: &str) -> Result<()> {
    execute!(out, LeaveAlternateScreen, DisableMouseCapture)?;
    // Raw mode is on, so each line ends with a carriage return too.
    write!(out, "\r\n{}\r\n\r\n{text}\r\n\r\n{}\r\n", tr!(agent_plain_intro), tr!(agent_plain_return))?;
    out.flush()?;
    Ok(())
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
