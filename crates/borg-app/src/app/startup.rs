//! Starting Borg: the state from the launch and from what was saved, the shell under the panels,
//! and the parts that work for the app: previews, plugins, monitors, the agent, borg cloud and
//! the searches.

use std::{collections::HashMap, path::Path, path::PathBuf, sync::Arc, time::Instant};

use anyhow::{Context, Result};
use borg_core::{AppState, Dialect, Effect, Msg, ShellVariables, Side, shell_variables, update};
use borg_gfx::Graphics;
use borg_i18n::tr;
use borg_term::{Emulator, GridSize, Pty, PtyEvent, ShellKind, shell_command};
use borg_vfs::Local;
use ratatui::layout::Size;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::{App, INDEX_DELAY, WheelBurst, appearance, palette};
use crate::{
    launch::{self, Launch},
    preview,
    session::{RESTORED_MARKER, StateDir},
};

impl App {
    pub fn start(launch: &Launch, size: Size, detected: launch::Terminal) -> Result<Self> {
        let grid = GridSize::console(size.width, size.height);
        let now = Instant::now();
        let (mut state, pending, collections_writable) = initial_state(launch, size, now);
        let emulator = console(launch, grid, &state);
        let (pty, pty_events) = spawn_shell(launch, &state, grid)?;
        let (messages, inbox) = unbounded_channel();
        crate::link::register_unlock_prompt(messages.clone());

        let drives = launch
            .state_dir
            .as_ref()
            .map_or_else(borg_drives::Drives::default, |dir| borg_drives::Drives::load(dir.path()));
        state.kept_answers = drives.kept();
        let preview = preview::Loader::new(messages.clone(), drives.clone());
        let plugins = crate::plugins::Plugins::new(messages.clone());
        let monitor = crate::activity::Monitor::new(messages.clone());
        let backup = crate::backup::Backup::new(messages.clone());
        state.backup.available = crate::backup::available();
        state.network.access = crate::network::capture_access();
        state.network.shell_variables = ShellVariables::new(
            Dialect::of(ShellKind::detect(&launch.shell)),
            shell_variables().filter_map(|name| std::env::var_os(name).map(|value| (name, value))).collect(),
        );
        state.network.authority = launch.config.proxy_authority();
        state.network.keep_free_mib = launch.config.capture_keep_free_mib();
        let docker = crate::docker::Docker::new(messages.clone(), launch.runtime_dir.join(borg_core::DOCKER_SCRATCH));
        let network = crate::network::Network::new(messages.clone(), launch.runtime_dir.clone());
        let startup_items = crate::startup_items::StartupItems::new(messages.clone());
        let mut alerts = crate::watch::Alerts::new(messages.clone());
        if state.settings.preferences.process_watch {
            alerts.follow_log();
        }
        let agent = crate::agent::Agent::start(&launch.runtime_dir, launch.session_id.as_deref(), messages.clone());
        let link = start_link(launch, &mut state, &agent, &messages);
        let native_mirror =
            crate::native_mirror::Host::start(&launch.runtime_dir, launch.session_id.as_deref().unwrap_or("default"))?;
        let membership = start_membership(launch, &agent, native_mirror.socket(), &messages);
        keep_runtime_dir(launch.runtime_dir.clone());
        let search = start_search(launch, &state, &messages);
        let storage = start_storage(launch, &search, &messages);
        Ok(Self {
            search,
            emulator,
            backend: Arc::new(Local::new(state.settings.listing_batch)),
            state,
            pty,
            cd_request: borg_term::cd_request_file(&launch.runtime_dir),
            pty_events,
            host_asks: crate::host_asks::Watchers::new(messages.clone()),
            messages,
            inbox,
            listings: [None, None],
            remote_listings: crate::remote_fs::Listings::default(),
            archives: crate::archive::Indexes::default(),
            drives,
            remote_catalog: None,
            remote_consoles: crate::remote_console::Clients::new(),
            console_server: crate::remote_console::Server::new(launch.shell.clone(), launch.runtime_dir.clone()),
            native_mirror,
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
            viewer_search: None,
            viewer_profile: None,
            plugins,
            monitor,
            backup,
            docker,
            network,
            startup_items,
            alerts,
            storage,
            shells: crate::docker::Shells::new(),
            console: borg_core::Console::Host,
            agent,
            reveals: Vec::new(),
            link,
            run: borg_link::hex(&borg_link::secret()),
            ask_board: tokio::sync::watch::Sender::new(borg_core::AskBoard::default()),
            uploads: HashMap::new(),
            downloads: std::collections::HashSet::new(),
            approve_all: std::collections::HashSet::new(),
            memory_readers: std::collections::HashSet::new(),
            saved_captures: super::agent_calls::SavedCaptures::default(),
            runtime_dir: launch.runtime_dir.clone(),
            mirror_connect: None,
            mirror_ready: None,
            cloud_suppressed: launch.remote.is_some() || service_child(),
            membership,
            host_capture: None,
            clipboard: None,
            plain: None,
            wide_emoji: detected.wide_emoji,
        })
    }
}

/// The state Borg starts in: the launch's panels and settings, the names of remote Borgs, a
/// remote session to mount, the launch's warning, the intro, and what was saved. Also whether
/// the collections may be saved, which they may not when they would not load.
fn initial_state(launch: &Launch, size: Size, now: Instant) -> (AppState, Vec<Effect>, bool) {
    let appearance = appearance(launch);
    let (mut state, mut pending) =
        AppState::new(launch.startup.clone(), size.width, size.height, launch.settings.clone(), appearance, now);
    if let Some(dir) = StateDir::locate() {
        load_remote_names(&mut state, dir.path());
    }
    if let Some(target) = &launch.remote {
        pending.retain(|effect| !matches!(effect, Effect::List { side: Side::Left, .. }));
        pending.extend(update(
            &mut state,
            Msg::MountRemote {
                side: Side::Left,
                machine: target.machine.clone(),
                session: target.session.clone(),
                name: target.name.clone(),
            },
            now,
        ));
    }
    state.notice = launch.warnings.first().cloned().or(state.notice.take());
    if state.settings.motion == borg_core::Motion::LOCAL {
        state.intro = Some(now);
    }
    let recent = launch.state_dir.as_ref().map_or_else(Vec::new, |dir| recent_folders(&state.settings, dir));
    state.folders = borg_core::Folders::new(launch.config.hotlist(), recent, launch.home.clone());
    let collections_writable = launch.state_dir.as_ref().is_none_or(|dir| load_saved(&mut state, dir));
    (state, pending, collections_writable)
}

/// The paired servers' names and keys, and the names given to remote machines and sessions.
fn load_remote_names(state: &mut AppState, dir: &Path) {
    state.remote_identities = crate::link::servers(dir).into_iter().map(|server| (server.name, server.key)).collect();
    match crate::remote_names::load(dir) {
        Ok(names) => state.remote_names = names,
        Err(error) => state.notice = Some(tr!(remote_names_error, error = error)),
    }
}

/// What the state folder keeps: the 2048 game, the collections, the log of what agents did, and
/// the search history; the log and the history are cleared while they are off. False when the
/// collections would not load.
fn load_saved(state: &mut AppState, dir: &StateDir) -> bool {
    state.game2048 = dir.load_game2048().ok().flatten();
    if state.settings.preferences.agent_log {
        state.agent.restore_log(dir.load_agent_log().unwrap_or_default());
    } else {
        let _ = dir.forget_agent_log();
    }
    let loaded = match dir.load_collections() {
        Ok(collections) => {
            state.collections = collections;
            true
        }
        Err(error) => {
            state.notice = Some(tr!(collections_load_error, error = error));
            false
        }
    };
    if state.settings.search.history {
        state.search_history = dir.load_search_history().unwrap_or_default();
    } else {
        let _ = dir.save_search_history(&borg_core::SearchHistory::default());
    }
    loaded
}

/// The folders the panels visited before, which are forgotten while folder history is off.
fn recent_folders(settings: &borg_core::Settings, dir: &StateDir) -> Vec<PathBuf> {
    if settings.folder_history {
        return dir.load_recent_folders().unwrap_or_default();
    }
    let _ = dir.save_recent_folders(&[]);
    Vec::new()
}

/// The console's screen, showing what the last session left on it, marked as restored.
fn console(launch: &Launch, grid: GridSize, state: &AppState) -> Emulator {
    let mut emulator = Emulator::new(grid, state.settings.scrollback);
    if let Some(restored) = &launch.restored {
        emulator.feed(restored);
        emulator.feed(RESTORED_MARKER);
    }
    emulator
}

/// The shell under the panels, started in the active panel's folder.
fn spawn_shell(launch: &Launch, state: &AppState, grid: GridSize) -> Result<(Pty, UnboundedReceiver<PtyEvent>)> {
    let shell_dir = state.active_panel().dir().to_path_buf();
    let mut command = shell_command(&launch.shell, &shell_dir, &launch.runtime_dir).context("preparing shell")?;
    command.env("BORG_CONFIRM_EXIT", "1");
    let (pty_tx, pty_events) = unbounded_channel();
    let pty = Pty::spawn(command, grid, pty_tx)?;
    Ok((pty, pty_events))
}

/// The link to borg cloud through the relay the settings name, and what the agent's dialog
/// shows of it.
fn start_link(
    launch: &Launch,
    state: &mut AppState,
    agent: &crate::agent::Agent,
    messages: &UnboundedSender<Msg>,
) -> crate::link::LinkServer {
    state.agent.connect = crate::agent::connect_info();
    let relay = launch.config.relay.as_deref().or(crate::link::DEFAULT_RELAY).and_then(borg_link::Relay::parse);
    state.agent.connect.relay = relay.as_ref().map(ToString::to_string).unwrap_or_default();
    let link = crate::link::LinkServer::new(
        launch.state_dir.as_ref().map(|dir| dir.path().to_path_buf()),
        launch.runtime_dir.clone(),
        launch.session_id.clone(),
        relay,
        agent.requeue_sender(),
        messages.clone(),
        agent.clients(),
    );
    state.agent.computers = link.computers();
    link
}

fn service_child() -> bool {
    std::env::var_os(crate::service::SERVICE_CHILD).is_some()
}

/// This Borg's registration with the background service, for a named session on this machine.
fn start_membership(
    launch: &Launch,
    agent: &crate::agent::Agent,
    mirror: &Path,
    messages: &UnboundedSender<Msg>,
) -> Option<crate::service::Membership> {
    let session = launch.session_id.as_deref().filter(|_| launch.remote.is_none() && launch.state_dir.is_some())?;
    let place = if service_child() { crate::service::Place::Background } else { crate::service::Place::Terminal };
    let inbox =
        crate::service::Inbox { calls: agent.requeue_sender(), messages: messages.clone(), clients: agent.clients() };
    Some(crate::service::Membership::start(session, mirror.to_path_buf(), place, inbox))
}

/// Marks the runtime folder as in use now and then, for as long as Borg runs.
fn keep_runtime_dir(runtime_dir: PathBuf) {
    tokio::spawn(async move {
        let mut refresh = tokio::time::interval(crate::launch::TTL_REFRESH);
        loop {
            refresh.tick().await;
            let _ = crate::launch::mark_in_use(&runtime_dir);
        }
    });
}

/// Borg's own files: measured for the Borg's files window, and cleaned up while Borg runs.
fn start_storage(
    launch: &Launch,
    search: &crate::search::Search,
    messages: &UnboundedSender<Msg>,
) -> crate::storage::Storage {
    let places = crate::storage::Places {
        runtime: launch.runtime_dir.clone(),
        state: launch.state_dir.as_ref().map(|dir| dir.path().to_path_buf()),
        cache: crate::size_cache::cache_dir(),
        config: launch.config_dir.as_ref().map(|dir| dir.path().to_path_buf()),
    };
    let indexes = search.indexes.clone();
    let roots: crate::storage::Roots = std::sync::Arc::new(move || indexes.roots());
    crate::storage::Storage::start(places, launch.runtime_idle, roots, messages.clone())
}

/// The searches and their indexes, which refresh the stale ones once Borg is up.
fn start_search(launch: &Launch, state: &AppState, messages: &UnboundedSender<Msg>) -> crate::search::Search {
    let rules = launch.config_dir.as_ref().map(crate::config::ConfigDir::rules);
    let search = crate::search::Search::new(launch.config.index_refresh(), rules);
    search.indexes.connect(messages.clone());
    search.indexes.set_roots(state.settings.index_roots.clone());
    let (indexes, settings) = (search.indexes.clone(), state.settings.search.clone());
    tokio::spawn(async move {
        tokio::time::sleep(INDEX_DELAY).await;
        indexes.refresh_stale(&settings);
    });
    search
}
