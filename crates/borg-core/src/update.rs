use borg_i18n::tr;
use std::time::{Duration, Instant};

use borg_term::{Scroll, ShellSignal, encode_key, encode_paste};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{AfterCommand, AppState, Effect, Msg, View, actions, command::CommandLine, keymap, overlays};

pub fn update(state: &mut AppState, msg: Msg, now: Instant) -> Vec<Effect> {
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
    effects.extend(crate::results::sync_sizes(state));
    crate::jobs::surface_questions(state);

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
    state.notice = None;
    let pending = state.escaped.take().is_some_and(|at| now.saturating_duration_since(at) <= ESCAPE_WINDOW);
    if key.code == KeyCode::Esc && key.modifiers == KeyModifiers::NONE && state.view == View::Panels {
        let hidden = state.active_panel().is_results();
        let effects = if hidden { crate::results::back(state) } else { Vec::new() };
        state.escaped = Some(now);
        state.notice = Some(if hidden {
            tr!(results_hidden_alt_r_resumes_in_the).into()
        } else {
            tr!(esc_then_1_9_f1_f9_esc).into()
        });
        return effects;
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
    let mut effects = prepare_shell(state);
    effects.push(Effect::Write(encode_paste(text, state.shell.observed.mode)));
    state.shell.mark_typed();
    effects
}

fn forward(state: &mut AppState, key: &KeyEvent) -> Vec<Effect> {
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
            state.notice = Some(format!(tr!(failed_exit_ctrl_o_shows_the_output), operation.label, code = code));
        }
        None => state.notice = Some(format!(tr!(finished_ctrl_o_shows_the_output), operation.label)),
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
