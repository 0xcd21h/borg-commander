//! The running app: the state, and the event loop that feeds it keys, the mouse, the shell's
//! output and the messages of the parts working for it, carrying out the effects it answers with.

use std::{
    collections::HashMap,
    io::Write as _,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use anyhow::Result;
use borg_core::{AMBIENT_FRAME, AppState, Effect, Msg, Pace, Side, update};
use borg_gfx::Graphics;
use borg_jobs::Control;
use borg_term::{Emulator, Pty, PtyEvent};
use borg_ui::{Palette, ThemeSet};
use borg_vfs::Backend;
use crossterm::{
    event::{EnableBracketedPaste, EnableMouseCapture, EventStream},
    execute,
    terminal::{EndSynchronizedUpdate, EnterAlternateScreen},
};
use futures::StreamExt;
use ratatui::DefaultTerminal;
use tokio::{
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
    time::sleep_until,
};

use crate::{
    config::{Config, ConfigDir},
    preview,
    session::{RESTORED_LINES, Session, StateDir},
};

mod agent_calls;
mod cloud;
mod console;
mod effects;
mod input;
mod jobs;
mod remotes;
mod render;
mod startup;
mod viewers;

use input::WheelBurst;
use render::{Plain, appearance, clear_screen, palette};
pub(crate) use render::{picture, span};

/// Indexes are checked after startup, once the panels and shell are up.
const INDEX_DELAY: Duration = Duration::from_secs(5);
/// Larger copies stay in Borg's clipboard: terminals drop or cut longer OSC 52 writes.
const TERMINAL_CLIPBOARD_LIMIT: usize = 256 * 1024;
const REMOTE_CLIENT_REFRESH: Duration = Duration::from_secs(1);
/// The folder under the runtime directory where container files are copied for viewing.
const DOCKER_SCRATCH: &str = "docker";

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
    remote_listings: crate::remote_fs::Listings,
    remote_catalog: Option<JoinHandle<()>>,
    remote_consoles: crate::remote_console::Clients,
    console_server: crate::remote_console::Server,
    native_mirror: crate::native_mirror::Host,
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
    /// Stops the viewer's search running in the background.
    viewer_search: Option<Arc<AtomicBool>>,
    plugins: crate::plugins::Plugins,
    monitor: crate::activity::Monitor,
    backup: crate::backup::Backup,
    docker: crate::docker::Docker,
    network: crate::network::Network,
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
    approve_all: std::collections::HashSet<String>,
    /// Processes an agent may search the memory of without asking again, by the computer it
    /// asks from (`None` for this one), once the user approved the first search.
    memory_readers: std::collections::HashSet<(Option<String>, borg_core::ProcessKey)>,
    runtime_dir: PathBuf,
    mirror_connect: Option<futures::future::BoxFuture<'static, Result<crate::mirror::Connection>>>,
    mirror_ready: Option<crate::mirror::Connection>,
    cloud_suppressed: bool,
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
                    Some(Ok(event)) => self.on_local_event(event)?,
                    Some(Err(e)) => return Err(e.into()),
                    None => Flow::Quit,
                },
                Some(input) = self.native_mirror.incoming.recv() => self.on_mirror_input(input)?,
                Some(event) = self.pty_events.recv() => self.on_pty(event)?,
                Some((key, event)) = self.remote_consoles.incoming.recv() => self.on_remote_console(key, event)?,
                Some((container, event)) = self.shells.incoming.recv() => self.on_container_shell(container, event)?,
                Some((call, reply)) = self.agent.incoming.recv() => self.on_agent(call, reply)?,
                Some(msg) = self.inbox.recv() => self.dispatch(msg)?,
                Some(msg) = self.search.incoming.recv() => self.dispatch(msg)?,
                connected = async {
                    match self.mirror_connect.as_mut() {
                        Some(connecting) => connecting.await,
                        None => std::future::pending().await,
                    }
                } => {
                    self.mirror_connect = None;
                    match connected {
                        Ok(connection) => {
                            self.mirror_ready = Some(connection);
                            self.dispatch(Msg::RemoteMirrorConnected(Ok(())))?
                        }
                        Err(error) => self.dispatch(Msg::RemoteMirrorConnected(Err(format!("{error:#}"))))?,
                    }
                },
                () = sleep_until(deadline.into()), if redraw => Flow::Continue,
                () = sleep_until(wake.unwrap_or(now + IDLE_WAIT).into()), if wake.is_some() => self.dispatch(Msg::Tick)?,
            };

            if let Flow::Quit = flow {
                return Ok(());
            }
            if !matches!(
                &self.state.overlay,
                Some(borg_core::Overlay::Remotes(_))
                    | Some(borg_core::Overlay::Prompt(borg_core::Prompt {
                        purpose: borg_core::PromptPurpose::RemoteName { .. },
                        ..
                    }))
            ) && let Some(catalog) = self.remote_catalog.take()
            {
                catalog.abort();
            }
            if let Some(connection) = self.mirror_ready.take() {
                terminal.backend_mut().write_all(&self.graphics.reset())?;
                terminal.backend_mut().flush()?;
                let mirrored = crate::mirror::run(connection, &mut input).await;
                execute!(
                    terminal.backend_mut(),
                    EndSynchronizedUpdate,
                    EnterAlternateScreen,
                    EnableBracketedPaste,
                    EnableMouseCapture
                )?;
                clear_screen(terminal)?;
                if let Err(error) = mirrored {
                    self.state.notice = Some(format!("Remote mirror: {error:#}"));
                }
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

    fn dispatch(&mut self, msg: Msg) -> Result<Flow> {
        let effects = update(&mut self.state, msg, Instant::now());
        if self.state.activity.side.is_none() || self.state.activity.pending.is_none() {
            self.monitor.cancel();
        }
        if self.state.docker.pending.is_none() {
            self.docker.cancel();
        }
        if self.state.network.pending.is_none() {
            self.network.cancel();
        }
        if !self.state.settings.preferences.git_plugin {
            self.plugins.cancel();
        }
        // The quick view and the open files' details share the loader; a load either waits
        // for must not end with the next message.
        let quick_view = self.state.aside.is_some() && self.state.active_panel().hovered().is_some();
        if !quick_view && !self.state.activity.file_preview_loading() {
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
        self.link.sync(self.state.settings.preferences.agent_cloud && !self.cloud_suppressed);
        Ok(flow)
    }
}
