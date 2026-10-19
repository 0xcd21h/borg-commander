use std::time::{Duration, Instant};

use borg_term::{Scroll, ShellSignal, encode_key, encode_paste};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{AfterCommand, AppState, Effect, Msg, View, actions, command::CommandLine, keymap, overlays};

pub fn update(state: &mut AppState, msg: Msg, now: Instant) -> Vec<Effect> {
    let before = crate::motion::before(state);
    let overlay = state.overlay.clone();
    let effects = match msg {
        Msg::Key(key) => on_key(state, &key, now),
        Msg::Pointer(pointer) => crate::pointer::on_pointer(state, pointer, now),
        Msg::Paste(text) => on_paste(state, &text),
        Msg::Resize { cols, rows } => {
            state.resize(cols, rows, now);
            Vec::new()
        }
        Msg::Batch { side, generation, entries } => {
            let show_hidden = state.settings.preferences.show_hidden;
            state.panel_mut(side).receive(generation, entries, show_hidden);
            Vec::new()
        }
        Msg::Listed { side, generation, error } => {
            state.notice = state.panel_mut(side).finish(generation, error).or(state.notice.take());
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
            on_signal(state, signal)
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
        Msg::Job { id, event } => crate::jobs::on_event(state, id, event),
        Msg::Peeked(peeked) => {
            crate::aside::receive(state, peeked);
            Vec::new()
        }
        Msg::CommandLine(text) => {
            crate::composer::open(text, state);
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
    crate::jobs::surface_questions(state);

    state.sync_fade(now);
    state.sync_overlay(overlay, now);
    crate::motion::animate(state, &before, now);
    effects
}

fn tick_timers(state: &mut AppState, now: Instant) {
    if state.rain_until.is_some_and(|until| now >= until) {
        state.rain_until = None;
    }
    let due = state.idle_since + state.settings.screensaver_after;
    if state.settings.preferences.screensaver && state.can_screensave() && now >= due {
        state.screensaver = true;
    }
}

fn on_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    state.idle_since = now;
    let resting = state.screensaver;
    state.screensaver = false;
    if resting {
        return Vec::new();
    }
    if state.shell.captured() {
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
    let key = &function_alias(*key);
    if state.viewer.is_some() {
        return crate::viewer::on_key(state, key);
    }
    if state.overlay.is_some() {
        return overlays::on_key(state, key, now);
    }

    if state.preview_focused && state.view == View::Panels {
        return crate::aside::on_key(state, key, now);
    }
    state.notice = None;
    let pending = state.escaped.take().is_some_and(|at| now.saturating_duration_since(at) <= ESCAPE_WINDOW);
    if key.code == KeyCode::Esc && key.modifiers == KeyModifiers::NONE && state.view == View::Panels {
        state.escaped = Some(now);
        state.notice = Some("Esc then 1–9 = F1–F9 · Esc then 0 = F10 · Esc 9 opens menus".into());
        return Vec::new();
    }
    let (key, mut effects) = match (pending, escaped_function(key)) {
        (true, Some(function)) => (function, Vec::new()),
        (true, None) if !state.shell.line_empty() => (*key, vec![Effect::Write(ESCAPE.to_vec())]),
        _ => (*key, Vec::new()),
    };
    let key = &key;
    let context =
        keymap::Context { view: state.view, line_empty: state.shell.line_empty(), at_prompt: !state.shell.busy() };
    effects.extend(match keymap::lookup(key, context) {
        Some(action) => actions::perform(state, action, now),
        None => forward(state, key),
    });
    effects
}

const FUNCTION_DIGITS: &str = "1234567890";
const ESCAPE_WINDOW: Duration = Duration::from_millis(1500);
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
    if state.overlay.is_some() {
        return overlays::on_paste(state, text);
    }
    let mut effects = prepare_shell(state);
    effects.push(Effect::Write(encode_paste(text, state.shell.observed.mode)));
    state.shell.mark_typed();
    effects
}

fn forward(state: &mut AppState, key: &KeyEvent) -> Vec<Effect> {
    let Some(bytes) = encode_key(key, state.shell.observed.mode) else { return Vec::new() };

    let mut effects = if state.view == View::Panels { prepare_shell(state) } else { Vec::new() };
    effects.push(Effect::Write(bytes));

    match key.code {
        KeyCode::Enter => {
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
    if !shell_ready(state) || state.shell.cwd == dir {
        return None;
    }
    let command = CommandLine::program("cd").path(dir).submit_hidden();
    state.shell.cwd = dir.to_path_buf();
    Some(Effect::Write(command))
}

fn reveal_slow_operation(state: &mut AppState, now: Instant) {
    let running = state.shell.running;
    let Some(operation) = state.operation.as_mut() else { return };
    if !operation.revealed && now >= operation.reveal_at && running {
        operation.revealed = true;
        state.view = View::Shell;
    }
}

fn on_signal(state: &mut AppState, signal: ShellSignal) -> Vec<Effect> {
    match signal {
        ShellSignal::PromptStart | ShellSignal::InputStart => {
            state.shell.clear_line();
            state.shell.integrated = true;
            Vec::new()
        }
        ShellSignal::CommandStart => {
            state.shell.running = true;
            Vec::new()
        }
        ShellSignal::CommandEnd { exit } if state.shell.running || state.shell.integrated => {
            state.shell.running = false;
            finish_command(state, exit)
        }
        ShellSignal::CommandEnd { .. } => Vec::new(),
        ShellSignal::Cwd { path, .. } => {
            let follow = path != state.shell.cwd && path != state.active_panel().destination();
            state.shell.cwd = path.clone();
            if follow { vec![state.navigate(state.active, path, None)] } else { Vec::new() }
        }
    }
}

fn finish_command(state: &mut AppState, exit: Option<i32>) -> Vec<Effect> {
    let full_screen = std::mem::take(&mut state.shell.went_full_screen);
    let Some(operation) = state.operation.take() else {
        if full_screen || state.settings.preferences.after_command == AfterCommand::ReturnToPanels {
            state.view = View::Panels;
        }
        return state.reload_all();
    };

    if operation.revealed || full_screen {
        state.view = View::Panels;
    }
    match exit {
        Some(0) => state.panel_mut(operation.side).clear_marks(),
        Some(code) => {
            state.notice = Some(format!("{} failed (exit {code}) · Ctrl-O shows the output", operation.label));
        }
        None => state.notice = Some(format!("{} finished · Ctrl-O shows the output", operation.label)),
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
