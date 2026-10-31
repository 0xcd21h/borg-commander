//! The shell and its command line: showing it, typing the selection's names and the panels'
//! folders into it or copying the names, and running programs and operations there.

use std::time::Instant;

use borg_i18n::tr;

use crate::{
    AppState, Effect, Operation, PathOf, Side, View,
    command::CommandLine,
    state::REVEAL_DELAY,
    update::{prepare_shell, shell_ready},
};

/// Ctrl-O: shows the shell, or the panels again.
pub(super) fn toggle_shell(state: &mut AppState) -> Vec<Effect> {
    if state.view == View::Panels && state.active_panel().remote_mount.is_some() && !state.console_owns_panel() {
        state.notice = Some(crate::remote::blocked_console_notice(state));
        return Vec::new();
    }
    match state.view {
        View::Panels => {
            state.view = View::Shell;
            state.shell.return_view = View::Shell;
            prepare_shell(state)
        }
        View::Shell => {
            state.view = View::Panels;
            state.shell.return_view = View::Panels;
            vec![Effect::ScrollConsole(borg_term::Scroll::Bottom)]
        }
    }
}

pub(super) fn insert_newline(state: &mut AppState) -> Vec<Effect> {
    let mut effects = prepare_shell(state);
    effects.push(Effect::Write(borg_term::encode_newline(state.shell.observed.mode)));
    state.shell.mark_typed();
    effects
}

/// Types the selected names into the command line; results come from several folders, so
/// theirs go in as paths.
pub(super) fn insert_names(state: &mut AppState, side: Side) -> Vec<Effect> {
    let panel = state.panel(side);
    let selected = panel.selection();
    if selected.is_empty() {
        return Vec::new();
    }
    let words = selected.iter().fold(CommandLine::default(), |line, entry| {
        let path = entry.path(panel.dir());
        if panel.is_results() {
            line.path(&path)
        } else {
            line.path(std::path::Path::new(path.file_name().unwrap_or_default()))
        }
    });
    type_words(state, words)
}

/// Types the folder of the active or the other panel into the command line.
pub(super) fn insert_path(state: &mut AppState, which: PathOf) -> Vec<Effect> {
    let side = match which {
        PathOf::Active => state.active,
        PathOf::Other => state.active.other(),
    };
    let dir = state.panel(side).dir().to_path_buf();
    type_words(state, CommandLine::default().path(&dir))
}

/// Copies the selected names, one per line, or their full paths. Results come from several
/// folders, so their names are always copied as paths.
pub(super) fn copy_selection(state: &mut AppState, side: Side, full: bool) -> Vec<Effect> {
    let panel = state.panel(side);
    let paths = full || panel.is_results();
    let lines: Vec<String> = panel
        .selection()
        .iter()
        .map(|entry| if paths { entry.path(panel.dir()).to_string_lossy().into_owned() } else { entry.name.clone() })
        .collect();
    if lines.is_empty() {
        return Vec::new();
    }
    state.notice = Some(if paths { tr!(paths_copied) } else { tr!(names_copied) }.into());
    crate::clipboard::copy(state, lines.join("\n"))
}

fn type_words(state: &mut AppState, words: CommandLine) -> Vec<Effect> {
    let mut effects = prepare_shell(state);
    effects.push(Effect::Write([&words.line()[..], b" "].concat()));
    state.shell.mark_typed();
    effects
}

/// Runs `command` in the shell, showing the shell while it does.
pub(super) fn interactive(state: &mut AppState, command: &CommandLine) -> Vec<Effect> {
    if !shell_ready(state) {
        state.notice = Some(tr!(shell_is_busy_finish_or_clear_the).to_owned());
        return Vec::new();
    }
    let mut effects = prepare_shell(state);
    effects.push(Effect::Write(command.submit()));
    state.shell.return_view = state.view;
    state.shell.submit();
    state.view = View::Shell;
    effects
}

/// Runs `command` out of sight in a shell that says when commands end, showing the shell only
/// when it takes a while; any other shell shows at once.
pub(crate) fn run_operation(
    state: &mut AppState,
    label: &'static str,
    command: &CommandLine,
    focus: Option<String>,
    now: Instant,
) -> Vec<Effect> {
    if !shell_ready(state) {
        state.notice = Some(tr!(shell_is_busy_finish_or_clear_the).to_owned());
        return Vec::new();
    }
    let mut effects = prepare_shell(state);
    effects.push(Effect::Write(command.submit_hidden()));
    state.shell.return_view = state.view;

    if state.shell.integrated {
        state.shell.running = true;
        state.operation =
            Some(Operation { label, side: state.active, reveal_at: now + REVEAL_DELAY, revealed: false, focus });
    } else {
        state.view = View::Shell;
    }
    effects
}
