use std::time::Instant;

use borg_term::{ShellSignal, encode_key, encode_paste};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    Action, AfterCommand, AppState, Effect, Msg, Row, Side, View, keymap,
    quote::{cd_command, insert_word, run_command},
};

const LINE_UP: isize = -1;
const LINE_DOWN: isize = 1;
const PAGE_UP: isize = -1;
const PAGE_DOWN: isize = 1;

pub fn update(state: &mut AppState, msg: Msg, now: Instant) -> Vec<Effect> {
    let effects = match msg {
        Msg::Key(key) => on_key(state, &key),
        Msg::Paste(text) => on_paste(state, &text),
        Msg::Resize { cols, rows } => {
            state.resize(cols, rows);
            Vec::new()
        }
        Msg::Batch { side, generation, entries } => {
            let show_hidden = state.settings.show_hidden;
            state.panel_mut(side).receive(generation, entries, show_hidden);
            Vec::new()
        }
        Msg::Listed { side, generation, error } => {
            state.notice = state.panel_mut(side).finish(generation, error).or(state.notice.take());
            Vec::new()
        }
        Msg::Observe(view) => {
            state.shell.observed = view;
            Vec::new()
        }
        Msg::Signal(signal) => on_signal(state, signal),
        Msg::ShellExited => vec![Effect::Quit],
    };

    state.sync_fade(now);
    effects
}

fn on_key(state: &mut AppState, key: &KeyEvent) -> Vec<Effect> {
    if state.shell.captured() {
        return forward(state, key);
    }

    state.notice = None;
    match keymap::lookup(key, state.view, state.shell.line_empty()) {
        Some(action) => perform(state, action),
        None => forward(state, key),
    }
}

fn on_paste(state: &mut AppState, text: &str) -> Vec<Effect> {
    let mut effects = prepare_shell(state);
    effects.push(Effect::Write(encode_paste(text, state.shell.observed.mode)));
    state.shell.typed = true;
    effects
}

fn perform(state: &mut AppState, action: Action) -> Vec<Effect> {
    let side = state.active;
    match action {
        Action::Quit => vec![Effect::Quit],
        Action::ToggleShell => toggle_shell(state),
        Action::SwitchPanel => {
            state.active = side.other();
            Vec::new()
        }
        Action::Up => moved(state, |panel| panel.move_by(LINE_UP)),
        Action::Down => moved(state, |panel| panel.move_by(LINE_DOWN)),
        Action::PageUp => moved(state, |panel| panel.page(PAGE_UP)),
        Action::PageDown => moved(state, |panel| panel.page(PAGE_DOWN)),
        Action::Top => moved(state, crate::Panel::to_start),
        Action::Bottom => moved(state, crate::Panel::to_end),
        Action::Open => open(state, side),
        Action::Descend => descend(state, side).into_iter().collect(),
        Action::Leave => leave(state, side).into_iter().collect(),
        Action::InsertName => insert_name(state, side),
        Action::ToggleHidden => {
            state.settings.show_hidden = !state.settings.show_hidden;
            Side::ALL.into_iter().map(|side| state.reload(side)).collect()
        }
    }
}

fn moved(state: &mut AppState, motion: impl FnOnce(&mut crate::Panel)) -> Vec<Effect> {
    motion(state.panel_mut(state.active));
    Vec::new()
}

fn toggle_shell(state: &mut AppState) -> Vec<Effect> {
    match state.view {
        View::Panels => {
            state.view = View::Shell;
            sync_cwd(state).into_iter().collect()
        }
        View::Shell => {
            state.view = View::Panels;
            Vec::new()
        }
    }
}

fn open(state: &mut AppState, side: Side) -> Vec<Effect> {
    let panel = state.panel(side);
    match panel.hovered() {
        Some(Row::Parent) => leave(state, side).into_iter().collect(),
        Some(Row::Entry(entry)) if entry.is_dir() => descend(state, side).into_iter().collect(),
        Some(Row::Entry(entry)) if entry.executable => {
            let command = run_command(&entry.name);
            let mut effects = prepare_shell(state);
            effects.push(Effect::Write(command));
            state.view = View::Shell;
            effects
        }
        Some(Row::Entry(_)) | None => Vec::new(),
    }
}

fn descend(state: &mut AppState, side: Side) -> Option<Effect> {
    let panel = state.panel(side);
    let Some(Row::Entry(entry)) = panel.hovered() else { return None };
    if !entry.is_dir() {
        return None;
    }
    let dir = panel.dir().join(&entry.name);
    Some(state.navigate(side, dir, None))
}

fn leave(state: &mut AppState, side: Side) -> Option<Effect> {
    let current = state.panel(side).dir();
    let parent = current.parent()?.to_path_buf();
    let child = current.file_name().map(|name| name.to_string_lossy().into_owned());
    Some(state.navigate(side, parent, child))
}

fn insert_name(state: &mut AppState, side: Side) -> Vec<Effect> {
    let Some(Row::Entry(entry)) = state.panel(side).hovered() else { return Vec::new() };
    let word = insert_word(&entry.name);
    let mut effects = prepare_shell(state);
    effects.push(Effect::Write(word));
    state.shell.typed = true;
    effects
}

fn forward(state: &mut AppState, key: &KeyEvent) -> Vec<Effect> {
    let Some(bytes) = encode_key(key, state.shell.observed.mode) else { return Vec::new() };

    let mut effects = if state.view == View::Panels { prepare_shell(state) } else { Vec::new() };
    effects.push(Effect::Write(bytes));

    match key.code {
        KeyCode::Enter => {
            state.shell.typed = false;
            state.view = View::Shell;
        }
        KeyCode::Char('c' | 'u') if key.modifiers == KeyModifiers::CONTROL => state.shell.typed = false,
        KeyCode::Char(_) if !key.modifiers.contains(KeyModifiers::CONTROL) => state.shell.typed = true,
        _ => {}
    }
    effects
}

fn prepare_shell(state: &mut AppState) -> Vec<Effect> {
    sync_cwd(state).into_iter().collect()
}

fn sync_cwd(state: &mut AppState) -> Option<Effect> {
    let dir = state.active_panel().dir();
    let idle = !state.shell.busy() && state.shell.line_empty();
    if !idle || state.shell.cwd == dir {
        return None;
    }
    let command = cd_command(dir);
    state.shell.cwd = dir.to_path_buf();
    Some(Effect::Write(command))
}

fn on_signal(state: &mut AppState, signal: ShellSignal) -> Vec<Effect> {
    match signal {
        ShellSignal::PromptStart | ShellSignal::InputStart => {
            state.shell.typed = false;
            Vec::new()
        }
        ShellSignal::CommandStart => {
            state.shell.running = true;
            Vec::new()
        }
        ShellSignal::CommandEnd { .. } if state.shell.running => {
            state.shell.running = false;
            if state.settings.after_command == AfterCommand::ReturnToPanels {
                state.view = View::Panels;
            }
            Side::ALL.into_iter().map(|side| state.reload(side)).collect()
        }
        ShellSignal::CommandEnd { .. } => Vec::new(),
        ShellSignal::Cwd { path, .. } => {
            let follow = path != state.active_panel().destination();
            state.shell.cwd = path.clone();
            if follow { vec![state.navigate(state.active, path, None)] } else { Vec::new() }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use borg_term::{Foreground, PromptState, TermMode};
    use borg_vfs::{Entry, EntryKind};

    use super::*;
    use crate::{Settings, ShellView};

    const COLS: u16 = 80;
    const ROWS: u16 = 24;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn dir(name: &str) -> Entry {
        Entry { name: name.into(), kind: EntryKind::Dir, size: 0, modified: None, executable: false }
    }

    fn idle_prompt() -> ShellView {
        ShellView { prompt: PromptState::Empty, foreground: Foreground::Shell, mode: TermMode::empty() }
    }

    fn setup() -> (AppState, Instant) {
        let now = Instant::now();
        let (mut state, effects) =
            AppState::new([PathBuf::from("/home"), PathBuf::from("/tmp")], COLS, ROWS, Settings::default(), now);
        for effect in effects {
            let Effect::List { side, generation, .. } = effect else { continue };
            update(&mut state, Msg::Batch { side, generation, entries: vec![dir("me")] }, now);
            update(&mut state, Msg::Listed { side, generation, error: None }, now);
        }
        update(&mut state, Msg::Observe(idle_prompt()), now);
        state.settle();
        (state, now)
    }

    fn writes(effects: &[Effect]) -> Vec<u8> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::Write(bytes) => Some(bytes.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn typing_goes_to_shell_after_syncing_directory() {
        let (mut state, now) = setup();
        update(&mut state, Msg::Key(key(KeyCode::Down)), now);
        let effects = update(&mut state, Msg::Key(key(KeyCode::Enter)), now);
        let Some(Effect::List { side, generation, dir }) = effects.first().cloned() else { panic!("expected listing") };
        assert_eq!(dir, Path::new("/home/me"));
        update(&mut state, Msg::Batch { side, generation, entries: vec![] }, now);
        state.settle();

        let effects = update(&mut state, Msg::Key(key(KeyCode::Char('l'))), now);
        assert_eq!(writes(&effects), b" cd '/home/me'\rl");
        assert_eq!(state.view, View::Panels);
    }

    #[test]
    fn enter_on_typed_line_runs_command_and_reveals_shell() {
        let (mut state, now) = setup();
        state.shell.observed.prompt = PromptState::Typed;
        let effects = update(&mut state, Msg::Key(key(KeyCode::Enter)), now);
        assert_eq!(writes(&effects), b"\r");
        assert_eq!(state.view, View::Shell);
        assert!(state.is_animating(now));
    }

    #[test]
    fn tab_switches_panels_only_on_empty_line() {
        let (mut state, now) = setup();
        update(&mut state, Msg::Key(key(KeyCode::Tab)), now);
        assert_eq!(state.active, Side::Right);

        state.shell.observed.prompt = PromptState::Typed;
        let effects = update(&mut state, Msg::Key(key(KeyCode::Tab)), now);
        assert_eq!(writes(&effects), b"\t");
        assert_eq!(state.active, Side::Right);
    }

    #[test]
    fn full_screen_programs_capture_every_key() {
        let (mut state, now) = setup();
        let captured = ShellView { foreground: Foreground::Program, mode: TermMode::ALT_SCREEN, ..idle_prompt() };
        update(&mut state, Msg::Observe(captured), now);
        assert_eq!(state.effective_view(), View::Shell);

        let effects = update(&mut state, Msg::Key(ctrl('o')), now);
        assert_eq!(writes(&effects), [0x0f]);
        assert_eq!(state.view, View::Panels);
    }

    #[test]
    fn ctrl_o_toggles_shell_view() {
        let (mut state, now) = setup();
        update(&mut state, Msg::Key(ctrl('o')), now);
        assert_eq!(state.view, View::Shell);
        update(&mut state, Msg::Key(ctrl('o')), now);
        assert_eq!(state.view, View::Panels);
    }

    #[test]
    fn panel_follows_shell_directory_changes() {
        let (mut state, now) = setup();
        let signal = ShellSignal::Cwd { host: String::new(), path: PathBuf::from("/var") };
        let effects = update(&mut state, Msg::Signal(signal), now);
        assert!(matches!(effects.as_slice(), [Effect::List { side: Side::Left, dir, .. }] if dir == Path::new("/var")));
        assert_eq!(state.shell.cwd, Path::new("/var"));
    }

    #[test]
    fn finished_command_refreshes_panels() {
        let (mut state, now) = setup();
        update(&mut state, Msg::Signal(ShellSignal::CommandStart), now);
        let effects = update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(0) }), now);
        assert_eq!(effects.iter().filter(|e| matches!(e, Effect::List { .. })).count(), 2);
    }
}
