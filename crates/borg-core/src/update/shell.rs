//! The shell under the panels: what it shows and signals, following the active panel's folder,
//! the commands Borg runs in it, and the consoles of remote Borgs.

use std::time::Instant;

use borg_i18n::tr;
use borg_term::ShellSignal;

use crate::{AfterCommand, AppState, Effect, ShellView, View, actions, command::CommandLine};

/// What the shell shows now. Its output, unlike a fresh look at its screen, means it echoed what
/// was typed.
pub(super) fn observe(state: &mut AppState, view: ShellView, echoed: bool) -> Vec<Effect> {
    state.shell.observe(view);
    if echoed {
        state.shell.unechoed = false;
    }
    state.refit();
    Vec::new()
}

/// A remote console's screen and signals. The console in use acts on them; any other keeps
/// them for when it is chosen.
pub(super) fn remote_output(
    state: &mut AppState,
    machine: String,
    session: String,
    view: ShellView,
    signals: Vec<ShellSignal>,
    echoed: bool,
    now: Instant,
) -> Vec<Effect> {
    let in_use =
        matches!(&state.console, crate::Console::Remote { machine: m, session: s } if *m == machine && *s == session);
    if !in_use {
        if let Some(status) = state.remote_consoles.get_mut(&(machine, session)) {
            for signal in signals {
                status.note(&signal);
            }
            status.observe(view);
        }
        return Vec::new();
    }
    let effects = signals.into_iter().flat_map(|signal| on_signal(state, signal, now)).collect();
    state.shell.observe(view);
    if echoed {
        state.shell.unechoed = false;
    }
    effects
}

/// A remote console that closed, or ended as asked. The host's console comes back if it was in
/// use, saying why.
pub(super) fn remote_closed(state: &mut AppState, machine: String, session: String, error: String) -> Vec<Effect> {
    let key = (machine.clone(), session.clone());
    state.remote_consoles.remove(&key);
    let ended = state.ending_remote.take_if(|ending| *ending == key).is_some();
    let error = if ended { tr!(remote_shell_ended, machine = machine) } else { error };
    // Either way the console stays closed until the session is chosen again.
    state.failed_remote_console = Some(key);
    state.remote_console_failure = Some(error.clone());
    if matches!(
        state.overlay,
        Some(crate::Overlay::Confirm(crate::Confirm { purpose: crate::ConfirmPurpose::RemoteShellExit { .. }, .. }))
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

/// A signal from a shell out of sight: the host's, or a container's.
pub(super) fn parked_signal(state: &mut AppState, container: Option<&str>, signal: &ShellSignal) -> Vec<Effect> {
    if container.is_none() && matches!(signal, ShellSignal::CommandEnd { .. }) {
        clear_shell_exit_confirmation(state);
    }
    crate::docker::parked_signal(state, container, signal);
    Vec::new()
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
    if !shell_ready(state) || state.shell.cwd == dir || !state.console_owns_panel() {
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

pub(super) fn reveal_slow_operation(state: &mut AppState, now: Instant) {
    let running = state.shell.running;
    let Some(operation) = state.operation.as_mut() else { return };
    if !operation.revealed && now >= operation.reveal_at && running {
        operation.revealed = true;
        state.view = View::Shell;
    }
}

pub(super) fn request_shell_exit(state: &mut AppState, token: String, now: Instant) -> Vec<Effect> {
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

pub(super) fn on_signal(state: &mut AppState, signal: ShellSignal, now: Instant) -> Vec<Effect> {
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
                path != state.shell.cwd && path != state.active_panel().destination() && state.console_owns_panel();
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
