use borg_i18n::tr;
use std::time::{Duration, Instant};

use borg_term::{Scroll, ShellSignal, encode_key, encode_paste};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{AfterCommand, AppState, Effect, Msg, View, actions, command::CommandLine, keymap, overlays};

pub fn update(state: &mut AppState, msg: Msg, now: Instant) -> Vec<Effect> {
    borg_i18n::with_locale(state.settings.preferences.language, || update_localized(state, msg, now))
}

fn update_localized(state: &mut AppState, msg: Msg, now: Instant) -> Vec<Effect> {
    let before = crate::motion::before(state);
    let overlay = state.overlay.clone();
    let effects = match msg {
        message @ (Msg::Sized { .. } | Msg::SizeProgress { .. } | Msg::SizesFinished { .. }) => {
            crate::measure::receive(state, message)
        }
        Msg::MemorySearchProgress { id, scanned, total, hits } => {
            if state.activity.pending.as_ref().is_some_and(|(pending, _)| *pending == id) {
                state.activity.search_progress = Some((scanned, total, hits));
            }
            Vec::new()
        }
        Msg::SizeChildren { side, generation, parent, result } => {
            crate::results::size_children(state, side, generation, parent, result);
            Vec::new()
        }
        Msg::Activity { id, result } => {
            crate::activity::receive(state, id, result, now);
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
            if state.console == (crate::Console::Remote { machine: machine.clone(), session: session.clone() }) {
                let effects = signals.into_iter().flat_map(|signal| on_signal(state, signal, now)).collect();
                state.shell.observe(view);
                if echoed {
                    state.shell.unechoed = false;
                }
                effects
            } else {
                if let Some(status) = state.remote_consoles.get_mut(&(machine, session)) {
                    for signal in signals {
                        status.note(&signal);
                    }
                    status.observe(view);
                }
                Vec::new()
            }
        }
        Msg::RemoteConsoleClosed { machine, session, error } => {
            let key = (machine.clone(), session.clone());
            state.remote_consoles.remove(&key);
            let ended = state.ending_remote.take_if(|ending| *ending == key).is_some();
            let error = if ended { tr!(remote_shell_ended, machine = machine) } else { error };
            // Either way the console stays closed until the session is chosen again.
            state.failed_remote_console = Some(key);
            state.remote_console_failure = Some(error.clone());
            if matches!(
                state.overlay,
                Some(crate::Overlay::Confirm(crate::Confirm {
                    purpose: crate::ConfirmPurpose::RemoteShellExit { .. },
                    ..
                }))
            ) {
                state.overlay = None;
            }
            if state.console == (crate::Console::Remote { machine: machine.clone(), session }) {
                state.shell = state.parked_host.take().unwrap_or_else(|| crate::ShellStatus::new(Default::default()));
                state.console = crate::Console::Host;
                state.view = View::Panels;
                state.notice = Some(error);
                vec![Effect::UseConsole(crate::Console::Host)]
            } else {
                Vec::new()
            }
        }
        Msg::RemoteNames(result) => {
            match result {
                Ok(names) => {
                    state.remote_names = names;
                    crate::remote::apply_names(state);
                }
                Err(error) => state.notice = Some(error),
            }
            Vec::new()
        }
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
        Msg::ParkedSignal { container: None, signal: ShellSignal::ExitRequested { token } } => {
            request_shell_exit(state, token, now)
        }
        Msg::ParkedSignal { container, signal } => {
            if container.is_none() && matches!(signal, ShellSignal::CommandEnd { .. }) {
                clear_shell_exit_confirmation(state);
            }
            crate::docker::parked_signal(state, container.as_deref(), &signal);
            Vec::new()
        }
        Msg::SearchProgress { side, generation, progress } => {
            crate::results::progress(state, side, generation, progress);
            Vec::new()
        }
        Msg::Key(key) => on_key(state, &key, now),
        Msg::Pointer(pointer) => crate::pointer::on_pointer(state, pointer, now),
        Msg::Paste(text) => on_paste(state, &text),
        Msg::Resize { cols, rows } => {
            state.resize(cols, rows, now);
            Vec::new()
        }
        Msg::Batch { side, generation, entries } => {
            let show_hidden = state.settings.preferences.show_hidden;
            if let Some(panel) = state.receiving_panel(side, generation) {
                panel.receive(generation, entries, show_hidden);
            }
            Vec::new()
        }
        Msg::Listed { side, generation, error } => {
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
        Msg::Observe(view) => {
            state.shell.observe(view);
            state.refit();
            Vec::new()
        }
        Msg::ShellOutput(view) => {
            state.shell.observe(view);
            state.shell.unechoed = false;
            state.refit();
            Vec::new()
        }
        Msg::Signal(signal) => {
            state.idle_since = now;
            on_signal(state, signal, now)
        }
        Msg::Opened(viewer) => {
            state.viewer = Some(viewer);
            crate::viewer::resize(state);
            Vec::new()
        }
        Msg::Tick => {
            reveal_slow_operation(state, now);
            tick_timers(state, now);
            Vec::new()
        }
        Msg::ShellExited => vec![Effect::Quit],
        Msg::Notice(text) => {
            state.notice = Some(text);
            Vec::new()
        }
        Msg::Indexed { root, entries, error } => {
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
        Msg::Job { id, event } => crate::jobs::on_event(state, id, event),
        Msg::Peeked(peeked) => {
            crate::aside::receive(state, peeked);
            Vec::new()
        }
        Msg::GitStatus { side, generation, dir, result } => {
            crate::plugins::receive(state, side, generation, dir, result, now);
            Vec::new()
        }
        Msg::GitFinished { id, result } => crate::plugins::finish(state, id, result),
    };
    let mut effects = effects;
    effects.extend(crate::aside::sync(state));
    effects.extend(crate::plugins::sync(state, now));
    effects.extend(crate::activity::sync(state, now));
    effects.extend(crate::docker::sync(state, now));
    effects.extend(crate::docker::sync_console(state, now));
    effects.extend(crate::results::sync_sizes(state));
    crate::busy::sync(state, now);
    crate::jobs::surface_questions(state);
    crate::agent::surface(state, now);

    state.refit();
    state.sync_fade(now);
    state.sync_overlay(overlay, now);
    crate::motion::animate(state, &before, now);
    effects
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

fn on_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    state.idle_since = now;
    state.intro = None;
    let resting = state.screensaver;
    state.screensaver = false;
    if resting {
        return Vec::new();
    }
    // A full-screen program gets the keys, unless a dialog shows over it.
    if state.shell.captured() && state.overlay.is_none() {
        return forward(state, key);
    }
    if state.view == View::Shell && state.overlay.is_none() && key.modifiers == KeyModifiers::SHIFT {
        let scroll = match key.code {
            KeyCode::PageUp => Some(Scroll::PageUp),
            KeyCode::PageDown => Some(Scroll::PageDown),
            KeyCode::Home => Some(Scroll::Top),
            KeyCode::End => Some(Scroll::Bottom),
            _ => None,
        };
        if let Some(scroll) = scroll {
            return vec![Effect::ScrollConsole(scroll)];
        }
    }
    // Alt-1 and Alt-2 pick Docker's tabs, and a container shell gets keys as typed, so they
    // reach the Docker view before Alt-digits turn into function keys.
    let docker_view = state.docker.is_open() && state.view == View::Panels;
    if docker_view
        && state.viewer.is_none()
        && state.overlay.is_none()
        && (state.docker.typing() || crate::docker::tab_key(key).is_some())
    {
        return crate::docker::route_key(state, key, now);
    }
    let key = &function_alias(*key);
    // An agent's approval shows over the viewer, so it takes the keys first.
    if state.viewer.is_some() && !matches!(state.overlay, Some(crate::Overlay::Approval(_))) {
        return crate::viewer::on_key(state, key);
    }
    // Borg's own fields paste what was copied in Borg; elsewhere Ctrl-V stays the shell's.
    let field = state.overlay.is_some()
        || (state.activity.side.is_some()
            && state.view == View::Panels
            && (state.activity.search_form.is_some() || state.activity.filtering));
    if field
        && crate::clipboard::is_paste(key)
        && let Some(text) = state.clipboard.latest().map(str::to_owned)
    {
        return on_paste(state, &text);
    }
    if state.overlay.is_some() {
        return overlays::on_key(state, key, now);
    }
    if state.docker.is_open() && state.view == View::Panels {
        return crate::docker::route_key(state, key, now);
    }
    if state.activity.side.is_some() && state.view == View::Panels {
        return crate::activity::route_key(state, key, now);
    }
    if state.active_panel().is_size_view() && !state.preview_focused && state.view == View::Panels {
        let action = match (key.code, key.modifiers) {
            (KeyCode::Left, KeyModifiers::NONE) if state.active_panel().is_tree() => Some(crate::Action::Leave),
            (KeyCode::Right, KeyModifiers::NONE) if state.active_panel().is_tree() => Some(crate::Action::Descend),
            (KeyCode::Char('r'), KeyModifiers::CONTROL) => Some(crate::Action::RefreshSizes),
            _ => None,
        };
        if let Some(action) = action {
            return actions::perform(state, action, now);
        }
    }

    if state.preview_focused && state.view == View::Panels {
        return crate::aside::on_key(state, key, now);
    }
    let dismissed = state.notice.take().is_some();
    let pending = state.function_prefix(now);
    state.escaped = None;
    // Esc backs out of one thing at a time: a notice, results, then the side pane. Only an
    // Esc with nothing to close starts the Esc-digit function keys, and a second Esc cancels.
    if key.code == KeyCode::Esc && key.modifiers == KeyModifiers::NONE && state.view == View::Panels {
        if state.active_panel().is_results() {
            let effects = crate::results::back(state);
            state.notice = Some(tr!(results_hidden_alt_r_resumes_in_the).into());
            return effects;
        }
        if let Some(aside) = state.aside {
            return crate::aside::toggle(state, aside);
        }
        if !dismissed && !pending {
            state.escaped = Some(now);
        }
        return Vec::new();
    }
    let (key, mut effects) = match (pending, escaped_function(key)) {
        (true, Some(function)) => (function, Vec::new()),
        (true, None) if !state.shell.line_empty() => (*key, vec![Effect::Write(ESCAPE.to_vec())]),
        _ => (*key, Vec::new()),
    };
    let key = &key;
    if state.view == View::Panels
        && state.active_panel().is_results()
        && matches!(key.code, KeyCode::Tab | KeyCode::BackTab)
    {
        return actions::perform(state, crate::Action::SwitchPanel, now);
    }
    if state.view == View::Panels
        && state.active_panel().is_results()
        && state.active_panel().listing() == &crate::Listing::Loading
        && matches!(state.active_panel().results.as_ref().map(|r| &r.source), Some(crate::ResultSource::Search { .. }))
        && key.code == KeyCode::Char('c')
        && key.modifiers == KeyModifiers::CONTROL
    {
        return actions::perform(state, crate::Action::CancelSearch, now);
    }
    let context =
        keymap::Context { view: state.view, line_empty: state.shell.line_empty(), at_prompt: !state.shell.busy() };
    effects.extend(match keymap::lookup(key, context) {
        Some(action) => actions::perform(state, action, now),
        None => forward(state, key),
    });
    effects
}

const FUNCTION_DIGITS: &str = "1234567890";
pub(crate) const ESCAPE_WINDOW: Duration = Duration::from_millis(1500);
const ESCAPE: &[u8] = b"\x1b";

fn function_key(digit: char) -> Option<KeyEvent> {
    FUNCTION_DIGITS
        .find(digit)
        .and_then(|index| u8::try_from(index + 1).ok())
        .map(|n| KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE))
}

fn function_alias(key: KeyEvent) -> KeyEvent {
    match key.code {
        KeyCode::Char(digit) if key.modifiers == KeyModifiers::ALT => function_key(digit).unwrap_or(key),
        _ => key,
    }
}

fn escaped_function(key: &KeyEvent) -> Option<KeyEvent> {
    match key.code {
        KeyCode::Char(digit) if key.modifiers == KeyModifiers::NONE => function_key(digit),
        _ => None,
    }
}

fn on_paste(state: &mut AppState, text: &str) -> Vec<Effect> {
    state.intro = None;
    if state.docker.is_open() && state.view == View::Panels && state.overlay.is_none() {
        return crate::docker::paste(state, text);
    }
    if state.activity.side.is_some() && state.view == View::Panels {
        if let Some(form) = state.activity.search_form.as_mut() {
            if form.selected == 0 {
                for c in text.chars().take(512) {
                    if form.input.text().len() + c.len_utf8() > 512 {
                        break;
                    }
                    form.input.apply(&KeyCode::Char(c).into());
                }
            }
        } else if state.activity.filtering {
            state.activity.filter.extend(text.chars().take(128));
            state.activity.rebuild();
        }
        return Vec::new();
    }

    if state.overlay.is_some() {
        return overlays::on_paste(state, text);
    }
    if state.active_panel().remote_mount.is_some() && !state.console_owns_panel() {
        state.notice = Some(crate::remote::blocked_console_notice(state));
        return Vec::new();
    }
    let mut effects = prepare_shell(state);
    effects.push(Effect::Write(encode_paste(text, state.shell.observed.mode)));
    state.shell.mark_typed();
    effects
}

fn forward(state: &mut AppState, key: &KeyEvent) -> Vec<Effect> {
    if state.active_panel().remote_mount.is_some() && !state.console_owns_panel() {
        state.notice = Some(crate::remote::blocked_console_notice(state));
        return Vec::new();
    }
    let shell_key = if !state.shell.busy() && !state.shell.captured() && key.modifiers == KeyModifiers::ALT {
        match key.code {
            KeyCode::Left => KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT),
            KeyCode::Right => KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT),
            _ => *key,
        }
    } else {
        *key
    };
    let Some(bytes) = encode_key(&shell_key, state.shell.observed.mode) else { return Vec::new() };

    let mut effects = if state.view == View::Panels { prepare_shell(state) } else { Vec::new() };
    effects.push(Effect::Write(bytes));

    match key.code {
        // The host's command line under a container's files would run on the wrong machine,
        // so it waits until a host panel is active.
        KeyCode::Enter if state.view == View::Panels && !console_owns_panel(state) => {
            state.notice = Some(tr!(docker_command_line_host).into());
            return Vec::new();
        }
        KeyCode::Enter => {
            if !state.shell.busy() && !state.shell.captured() {
                state.shell.return_view = state.view;
            }
            state.shell.submit();
            state.view = View::Shell;
        }
        KeyCode::Char('c' | 'u') if key.modifiers == KeyModifiers::CONTROL => state.shell.clear_line(),
        KeyCode::Char(_) if !key.modifiers.contains(KeyModifiers::CONTROL) => state.shell.mark_typed(),
        _ => {}
    }
    effects
}

pub(crate) fn prepare_shell(state: &mut AppState) -> Vec<Effect> {
    sync_cwd(state).into_iter().collect()
}

pub(crate) fn shell_ready(state: &AppState) -> bool {
    !state.shell.busy() && state.shell.line_empty()
}

fn sync_cwd(state: &mut AppState) -> Option<Effect> {
    let dir = state.active_panel().dir();
    // A shell follows only a panel on its own machine: the host's, or its container's.
    if !shell_ready(state) || state.shell.cwd == dir || !console_owns_panel(state) {
        return None;
    }
    let typed = CommandLine::program("cd").path(dir).submit_hidden();
    let effect = if state.shell.silent_cd && !matches!(state.console, crate::Console::Remote { .. }) {
        Effect::ChangeShellDir { dir: dir.to_path_buf(), typed }
    } else if state.shell.integrated {
        Effect::WriteConcealed(typed)
    } else {
        Effect::Write(typed)
    };
    state.shell.cwd = dir.to_path_buf();
    Some(effect)
}

fn console_owns_panel(state: &AppState) -> bool {
    state.console_owns_panel()
}

fn reveal_slow_operation(state: &mut AppState, now: Instant) {
    let running = state.shell.running;
    let Some(operation) = state.operation.as_mut() else { return };
    if !operation.revealed && now >= operation.reveal_at && running {
        operation.revealed = true;
        state.view = View::Shell;
    }
}

fn request_shell_exit(state: &mut AppState, token: String, now: Instant) -> Vec<Effect> {
    if state.overlay.is_some() {
        return vec![Effect::CancelShellExit { token }];
    }
    state.screensaver = false;
    let effects = actions::perform(state, crate::Action::Quit, now);
    if let Some(crate::Overlay::Confirm(confirm)) = &mut state.overlay {
        if state.jobs.is_empty() && state.docker.shells.is_empty() {
            confirm.message = tr!(quit_shell_exit).into();
        } else {
            confirm.message = format!("{}\n{}", tr!(quit_shell_exit), confirm.message);
        }
        confirm.purpose = crate::ConfirmPurpose::ShellExit { token };
    }
    effects
}

fn clear_shell_exit_confirmation(state: &mut AppState) {
    if matches!(
        state.overlay,
        Some(crate::Overlay::Confirm(crate::Confirm { purpose: crate::ConfirmPurpose::ShellExit { .. }, .. }))
    ) {
        state.overlay = None;
    }
}

fn on_signal(state: &mut AppState, signal: ShellSignal, now: Instant) -> Vec<Effect> {
    match signal {
        ShellSignal::ExitRequested { token } if state.console == crate::Console::Host => {
            request_shell_exit(state, token, now)
        }
        ShellSignal::ExitRequested { token } => match state.console.clone() {
            crate::Console::Remote { machine, .. } if state.overlay.is_none() => {
                state.screensaver = false;
                state.overlay = Some(crate::Overlay::Confirm(crate::Confirm {
                    title: tr!(remote_shell_exit_title),
                    message: tr!(remote_shell_exit, machine = machine),
                    purpose: crate::ConfirmPurpose::RemoteShellExit { token },
                    accept: false,
                }));
                Vec::new()
            }
            crate::Console::Remote { .. } => vec![Effect::Write(format!("{token}:no\n").into_bytes())],
            _ => Vec::new(),
        },
        ShellSignal::PromptStart | ShellSignal::InputStart => {
            if signal == ShellSignal::PromptStart {
                state.shell.silent_cd = false;
            }
            state.shell.clear_line();
            state.shell.integrated = true;
            Vec::new()
        }
        ShellSignal::SilentCd => {
            state.shell.silent_cd = true;
            Vec::new()
        }
        ShellSignal::CommandStart => {
            state.shell.running = true;
            Vec::new()
        }
        ShellSignal::CommandEnd { exit } if state.shell.running || state.shell.integrated => {
            if state.console == crate::Console::Host {
                clear_shell_exit_confirmation(state);
            }
            state.shell.running = false;
            finish_command(state, exit)
        }
        ShellSignal::CommandEnd { .. } => Vec::new(),
        ShellSignal::Cwd { path, .. } => {
            let follow =
                path != state.shell.cwd && path != state.active_panel().destination() && console_owns_panel(state);
            state.shell.cwd = path.clone();
            if follow { vec![state.navigate(state.active, path, None)] } else { Vec::new() }
        }
    }
}

fn finish_command(state: &mut AppState, exit: Option<i32>) -> Vec<Effect> {
    let full_screen = std::mem::take(&mut state.shell.went_full_screen);
    let Some(operation) = state.operation.take() else {
        if state.shell.return_view == View::Panels
            && (full_screen || state.settings.preferences.after_command == AfterCommand::ReturnToPanels)
        {
            state.view = View::Panels;
        }
        return state.reload_all();
    };

    if operation.revealed || full_screen {
        state.view = state.shell.return_view;
    }
    match exit {
        Some(0) => state.panel_mut(operation.side).clear_marks(),
        Some(code) => {
            state.notice = Some(tr!(failed_exit_ctrl_o_shows_the_output, operation.label, code = code));
        }
        None => state.notice = Some(tr!(finished_ctrl_o_shows_the_output, operation.label)),
    }

    let side = operation.side;
    let mut effects: Vec<Effect> = state
        .reload_all()
        .into_iter()
        .filter(|effect| operation.focus.is_none() || !matches!(effect, Effect::List { side: s, .. } if *s == side))
        .collect();
    if let Some(focus) = operation.focus {
        let dir = state.panel(side).dir().to_path_buf();
        effects.push(state.navigate(side, dir, Some(focus)));
    }
    effects
}
