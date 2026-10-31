//! Keys and pastes: which part of the screen takes them, the function keys typed as Esc or Alt
//! and a digit, and the keys that go to the shell under the panels.

use std::time::{Duration, Instant};

use borg_i18n::tr;
use borg_term::{Scroll, encode_key, encode_paste};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::shell::prepare_shell;
use crate::{Action, AppState, Effect, View, actions, keymap, overlays};

pub(super) fn on_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
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
    if let Some(scroll) = console_scroll(state, key) {
        return vec![Effect::ScrollConsole(scroll)];
    }
    // Alt-1 and Alt-2 pick Docker's tabs, and a container shell gets keys as typed, so they
    // reach the Docker view before Alt-digits turn into function keys.
    if state.docker.is_open()
        && state.view == View::Panels
        && state.viewer.is_none()
        && state.overlay.is_none()
        && (state.docker.typing() || crate::docker::tab_key(key).is_some())
    {
        return crate::docker::route_key(state, key, now);
    }
    let key = &function_alias(*key);
    if let Some(effects) = in_front(state, key, now) {
        return effects;
    }
    if let Some(action) = size_view_action(state, key) {
        return actions::perform(state, action, now);
    }
    if state.preview_focused && state.view == View::Panels {
        return crate::aside::on_key(state, key, now);
    }
    panels_key(state, key, now)
}

/// Shift and the paging keys scroll back through the shell's output.
fn console_scroll(state: &AppState, key: &KeyEvent) -> Option<Scroll> {
    if state.view != View::Shell || state.overlay.is_some() || key.modifiers != KeyModifiers::SHIFT {
        return None;
    }
    match key.code {
        KeyCode::PageUp => Some(Scroll::PageUp),
        KeyCode::PageDown => Some(Scroll::PageDown),
        KeyCode::Home => Some(Scroll::Top),
        KeyCode::End => Some(Scroll::Bottom),
        _ => None,
    }
}

/// The key for what shows in front of the panels: the editor or the viewer, unless an agent's
/// approval shows over them, a field that pastes, the open window, a monitor beside the panels,
/// or the editor in a panel.
fn in_front(state: &mut AppState, key: &KeyEvent, now: Instant) -> Option<Vec<Effect>> {
    let approval = matches!(state.overlay, Some(crate::Overlay::Approval(_)));
    if state.editor.is_some() && !approval {
        return Some(crate::editor::on_key(state, crate::EditorSlot::Screen, key));
    }
    if state.viewer.is_some() && !approval {
        return Some(crate::viewer::on_key(state, key));
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
        return Some(on_paste(state, &text));
    }
    if state.overlay.is_some() {
        return Some(overlays::on_key(state, key, now));
    }
    let panels = state.view == View::Panels;
    if state.docker.is_open() && panels {
        return Some(crate::docker::route_key(state, key, now));
    }
    if state.activity.side.is_some() && panels {
        return Some(crate::activity::route_key(state, key, now));
    }
    if let Some(crate::EditorSlot::Panel(side)) = crate::editor::focused(state) {
        return Some(crate::editor::on_panel_key(state, side, key, now));
    }
    None
}

/// In a panel of sizes, Left and Right move through the tree and Ctrl-R measures again.
fn size_view_action(state: &AppState, key: &KeyEvent) -> Option<Action> {
    let panel = state.active_panel();
    if !panel.is_size_view() || state.preview_focused || state.view != View::Panels {
        return None;
    }
    match (key.code, key.modifiers) {
        (KeyCode::Left, KeyModifiers::NONE) if panel.is_tree() => Some(Action::Leave),
        (KeyCode::Right, KeyModifiers::NONE) if panel.is_tree() => Some(Action::Descend),
        (KeyCode::Char('r'), KeyModifiers::CONTROL) => Some(Action::RefreshSizes),
        _ => None,
    }
}

/// A key for the panels or the shell under them: the key map's action for it, or else the
/// shell gets it. Any key takes the notice away, and Esc and a digit stand for a function key.
fn panels_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    let dismissed = state.notice.take().is_some();
    let pending = state.function_prefix(now);
    state.escaped = None;
    if key.code == KeyCode::Esc && key.modifiers == KeyModifiers::NONE && state.view == View::Panels {
        return escape(state, !dismissed && !pending, now);
    }
    let (key, mut effects) = match (pending, escaped_function(key)) {
        (true, Some(function)) => (function, Vec::new()),
        (true, None) if !state.shell.line_empty() => (*key, vec![Effect::Write(ESCAPE.to_vec())]),
        _ => (*key, Vec::new()),
    };
    if let Some(action) = results_action(state, &key) {
        return actions::perform(state, action, now);
    }
    let context =
        keymap::Context { view: state.view, line_empty: state.shell.line_empty(), at_prompt: !state.shell.busy() };
    effects.extend(match keymap::lookup(&key, context) {
        Some(action) => actions::perform(state, action, now),
        None => forward(state, &key),
    });
    effects
}

/// Esc backs out of one thing at a time: a file still opening in the viewer, results, then the
/// side pane. Only an Esc with nothing to close, not even a notice, starts the Esc-digit
/// function keys (`prefix`), and a second Esc cancels.
fn escape(state: &mut AppState, prefix: bool, now: Instant) -> Vec<Effect> {
    if state.opening.take().is_some() {
        return Vec::new();
    }
    if state.active_panel().is_results() {
        let effects = crate::results::back(state);
        state.notice = Some(tr!(results_hidden_alt_r_resumes_in_the).into());
        return effects;
    }
    if let Some(aside) = state.aside {
        return crate::aside::toggle(state, aside);
    }
    if prefix {
        state.escaped = Some(now);
    }
    Vec::new()
}

/// The keys of a panel of results: Tab goes to the other panel, and Ctrl-C stops a search still
/// running.
fn results_action(state: &AppState, key: &KeyEvent) -> Option<Action> {
    let panel = state.active_panel();
    if state.view != View::Panels || !panel.is_results() {
        return None;
    }
    if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
        return Some(Action::SwitchPanel);
    }
    let searching = panel.listing() == &crate::Listing::Loading
        && matches!(panel.results.as_ref().map(|r| &r.source), Some(crate::ResultSource::Search { .. }));
    (searching && key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL)
        .then_some(Action::CancelSearch)
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

pub(super) fn on_paste(state: &mut AppState, text: &str) -> Vec<Effect> {
    state.intro = None;
    if let Some(slot) = crate::editor::focused(state).filter(|_| state.overlay.is_none()) {
        return crate::editor::paste(state, slot, text);
    }
    if state.docker.is_open() && state.view == View::Panels && state.overlay.is_none() {
        return crate::docker::paste(state, text);
    }
    if state.activity.side.is_some() && state.view == View::Panels {
        crate::activity::paste(&mut state.activity, text);
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

pub(super) fn forward(state: &mut AppState, key: &KeyEvent) -> Vec<Effect> {
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
        KeyCode::Enter if state.view == View::Panels && !state.console_owns_panel() => {
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
