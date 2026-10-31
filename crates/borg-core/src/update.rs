//! The update step: each message changes the state and answers with the effects to carry out,
//! and then every part of the state that follows another catches up. Keys and pastes are in
//! `keys`, and the shell's output, signals and commands in `shell`.

use std::time::Instant;

use borg_i18n::tr;
use borg_term::ShellSignal;

use crate::{AppState, Effect, Msg};

mod keys;
mod shell;

pub(crate) use keys::ESCAPE_WINDOW;
pub(crate) use shell::{prepare_shell, shell_ready};

pub fn update(state: &mut AppState, msg: Msg, now: Instant) -> Vec<Effect> {
    borg_i18n::with_locale(state.settings.preferences.language, || update_localized(state, msg, now))
}

fn update_localized(state: &mut AppState, msg: Msg, now: Instant) -> Vec<Effect> {
    let before = crate::motion::before(state);
    let overlay = state.overlay.clone();
    let mut effects = receive(state, msg, now);
    effects.extend(follow(state, now));
    state.refit();
    state.sync_fade(now);
    state.sync_overlay(overlay, now);
    crate::motion::animate(state, &before, now);
    effects
}

/// Hands each message to the part of the state it is for.
fn receive(state: &mut AppState, msg: Msg, now: Instant) -> Vec<Effect> {
    match msg {
        Msg::Key(key) => keys::on_key(state, &key, now),
        Msg::Pointer(pointer) => crate::pointer::on_pointer(state, pointer, now),
        Msg::Paste(text) => keys::on_paste(state, &text),
        Msg::Resize { cols, rows } => {
            state.resize(cols, rows, now);
            Vec::new()
        }
        Msg::Tick => tick(state, now),
        Msg::Notice(text) => {
            state.notice = Some(text);
            Vec::new()
        }

        Msg::Batch { side, generation, entries } => batch(state, side, generation, entries),
        Msg::Listed { side, generation, error } => listed(state, side, generation, error),
        message @ (Msg::Sized { .. } | Msg::SizeProgress { .. } | Msg::SizesFinished { .. }) => {
            crate::measure::receive(state, message)
        }
        Msg::SizeChildren { side, generation, parent, result } => {
            crate::results::size_children(state, side, generation, parent, result);
            Vec::new()
        }
        Msg::SearchProgress { side, generation, progress } => {
            crate::results::progress(state, side, generation, progress);
            Vec::new()
        }
        Msg::Indexed { root, entries, error } => indexed(state, root, entries, error),
        Msg::Peeked(peeked) => {
            crate::aside::receive(state, peeked);
            Vec::new()
        }
        Msg::GitStatus { side, generation, dir, result } => {
            crate::plugins::receive(state, side, generation, dir, result, now);
            Vec::new()
        }
        Msg::GitFinished { id, result } => crate::plugins::finish(state, id, result),
        Msg::Job { id, event } => crate::jobs::on_event(state, id, event),
        Msg::Backup(event) => crate::backup::receive(state, event),

        Msg::Observe(view) => shell::observe(state, view, false),
        Msg::ShellOutput(view) => shell::observe(state, view, true),
        Msg::Signal(signal) => {
            state.idle_since = now;
            shell::on_signal(state, signal, now)
        }
        Msg::ParkedSignal { container: None, signal: ShellSignal::ExitRequested { token } } => {
            shell::request_shell_exit(state, token, now)
        }
        Msg::ParkedSignal { container, signal } => shell::parked_signal(state, container.as_deref(), &signal),
        Msg::ShellExited => vec![Effect::Quit],

        Msg::EditorOpened(slot, result) => crate::editor::opened(state, slot, result),
        Msg::EditorSaved(slot, result) => crate::editor::saved(state, slot, result),
        Msg::Opened(viewer) => {
            crate::viewer::opened(state, viewer);
            Vec::new()
        }
        Msg::OpenFailed { path, notice } => {
            crate::viewer::failed(state, path, notice);
            Vec::new()
        }
        Msg::FoundInViewer { id, found } => {
            crate::viewer::found(state, id, found);
            Vec::new()
        }
        Msg::SearchingViewer { id, percent } => {
            crate::viewer::searching(state, id, percent);
            Vec::new()
        }

        Msg::Activity { id, result } => {
            crate::activity::receive(state, id, result, now);
            Vec::new()
        }
        Msg::MemorySearchProgress { id, scanned, total, hits } => {
            if state.activity.pending.as_ref().is_some_and(|(pending, _)| *pending == id) {
                state.activity.search_progress = Some((scanned, total, hits));
            }
            Vec::new()
        }
        Msg::Docker { id, result } => crate::docker::receive(state, id, result, now),
        Msg::DockerStats(frame) => {
            crate::docker::stats(state, frame);
            Vec::new()
        }
        Msg::DockerShellOutput { container, mode } => {
            crate::docker::shell_output(state, &container, mode);
            Vec::new()
        }
        Msg::DockerShellClosed { container } => {
            crate::docker::shell_closed(state, &container, now);
            Vec::new()
        }

        Msg::Agent(event) => crate::agent::receive(state, event),
        Msg::MountRemote { side, machine, session, name } => crate::remote::mount(state, side, machine, session, name),
        Msg::RemoteConsoleOutput { echoed, machine, session, view, signals } => {
            shell::remote_output(state, machine, session, view, signals, echoed, now)
        }
        Msg::RemoteConsoleClosed { machine, session, error } => shell::remote_closed(state, machine, session, error),
        Msg::RemoteNames(result) => remote_names(state, result),
        Msg::RemoteMirrorConnected(result) => {
            crate::remote::connected(state, result);
            Vec::new()
        }
        Msg::RemoteClients(clients) => {
            crate::remote::clients(state, clients);
            Vec::new()
        }
        Msg::RemoteSessionDeleted { machine, session, result } => {
            crate::remote::deleted(state, machine, session, result)
        }
        Msg::RemoteHostForgotten { machine, result } => crate::remote::forgotten(state, machine, result),
        Msg::RemoteCatalog { choices, complete } => {
            crate::remote::catalog(state, choices, complete);
            Vec::new()
        }
    }
}

/// What follows the rest of the state catches up: the preview, the plugins, the backup, the
/// monitors, the sizes and the busy marker, and the jobs' and the agent's questions show.
fn follow(state: &mut AppState, now: Instant) -> Vec<Effect> {
    let mut effects = Vec::new();
    effects.extend(crate::aside::sync(state));
    effects.extend(crate::plugins::sync(state, now));
    effects.extend(crate::backup::sync(state));
    effects.extend(crate::activity::sync(state, now));
    effects.extend(crate::activity::sync_file_preview(state));
    effects.extend(crate::docker::sync(state, now));
    effects.extend(crate::docker::sync_console(state, now));
    effects.extend(crate::results::sync_sizes(state));
    crate::busy::sync(state, now);
    crate::jobs::surface_questions(state);
    crate::agent::surface(state, now);
    effects
}

fn tick(state: &mut AppState, now: Instant) -> Vec<Effect> {
    shell::reveal_slow_operation(state, now);
    tick_timers(state, now);
    Vec::new()
}

fn tick_timers(state: &mut AppState, now: Instant) {
    if state.rain_until.is_some_and(|until| now >= until) {
        state.rain_until = None;
    }
    if !state.function_prefix(now) {
        state.escaped = None;
    }
    if state.intro.is_some_and(|start| now >= start + crate::state::INTRO) {
        state.intro = None;
        // Idle time counts from when the panels appear, not from launch.
        state.idle_since = now;
    }
    let due = state.idle_since + state.settings.screensaver_after;
    if state.settings.preferences.screensaver && state.can_screensave() && now >= due && !state.screensaver {
        state.screensaver = true;
        // No randomness in core: the moment it starts is random enough to pick one.
        let tick = now.saturating_duration_since(state.epoch).as_micros();
        state.saver = if tick.is_multiple_of(2) { crate::Saver::Cube } else { crate::Saver::Glitch };
    }
}

/// A batch of a listing's entries, for the panel still waiting for them.
fn batch(state: &mut AppState, side: crate::Side, generation: u64, entries: Vec<borg_vfs::Entry>) -> Vec<Effect> {
    let show_hidden = state.settings.preferences.show_hidden;
    if let Some(panel) = state.receiving_panel(side, generation) {
        panel.receive(generation, entries, show_hidden);
    }
    Vec::new()
}

/// The end of a listing. A collection keeps what it lists, and the sizes measured in the folder
/// show again.
fn listed(state: &mut AppState, side: crate::Side, generation: u64, error: Option<String>) -> Vec<Effect> {
    let visible = state.panel(side).generation() == generation;
    if let Some(panel) = state.receiving_panel(side, generation) {
        let notice = panel.finish(generation, error);
        panel.settle();
        let collection = match panel.results.as_ref().map(|r| &r.source) {
            Some(crate::ResultSource::Collection(name)) => Some((name.clone(), panel.entries().to_vec())),
            _ => None,
        };
        if let Some((name, entries)) = collection
            && let Some(saved) = state.collections.iter_mut().find(|c| c.name == name)
        {
            saved.entries = entries;
        }
        state.notice = notice.or(state.notice.take());
    }
    if visible {
        crate::measure::reapply(state, side);
    }
    Vec::new()
}

/// A folder indexed for searching, which says so when Borg was asked to index it.
fn indexed(state: &mut AppState, root: std::path::PathBuf, entries: usize, error: Option<String>) -> Vec<Effect> {
    let folder = root.display().to_string();
    if let Some(error) = error.filter(|_| state.settings.index_roots.contains(&root)) {
        state.notice = Some(tr!(index_failed, folder = folder, error = error));
    } else if state.indexing.as_ref() == Some(&root) {
        state.notice = Some(tr!(index_built, folder = folder, entries = entries));
    }
    if state.indexing.as_ref() == Some(&root) {
        state.indexing = None;
    }
    Vec::new()
}

/// The names given to remote machines and sessions.
fn remote_names(state: &mut AppState, result: Result<crate::RemoteNames, String>) -> Vec<Effect> {
    match result {
        Ok(names) => {
            state.remote_names = names;
            crate::remote::apply_names(state);
        }
        Err(error) => state.notice = Some(error),
    }
    Vec::new()
}
