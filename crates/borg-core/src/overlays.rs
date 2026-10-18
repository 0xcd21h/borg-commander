use std::{path::PathBuf, time::Instant};

use crossterm::event::{KeyCode, KeyEvent};

use crate::{
    AppState, Confirm, ConfirmPurpose, Effect, MenuCursor, Overlay, Pattern, Picker, Prompt, PromptPurpose,
    actions::{self, run_operation},
    input::Edit,
    menu::menus,
    ops::{self, ArchiveFormat},
};

const PREVIOUS: isize = -1;
const NEXT: isize = 1;
const ACCEPT_KEY: char = 'y';
const REJECT_KEY: char = 'n';

pub(crate) fn on_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    match state.overlay.clone() {
        Some(Overlay::Themes(picker)) => on_themes(state, picker, key, now),
        Some(Overlay::Menu(cursor)) => on_menu(state, cursor, key, now),
        Some(Overlay::Prompt(prompt)) => on_prompt(state, prompt, key, now),
        Some(Overlay::Confirm(confirm)) => on_confirm(state, confirm, key, now),
        None => Vec::new(),
    }
}

pub(crate) fn on_paste(state: &mut AppState, text: &str) -> Vec<Effect> {
    if let Some(Overlay::Prompt(prompt)) = state.overlay.as_mut() {
        let line = text.lines().next().unwrap_or_default();
        line.chars().for_each(|c| {
            prompt.input.apply(&KeyEvent::from(KeyCode::Char(c)));
        });
    }
    Vec::new()
}

fn on_themes(state: &mut AppState, picker: Picker, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    let last = state.appearance.choices().len().saturating_sub(1);
    let fade = state.motion().fade;
    let preview = |state: &mut AppState, selected: usize| {
        state.appearance.select(selected, now, fade);
        state.overlay = Some(Overlay::Themes(Picker { selected, ..picker }));
        Vec::new()
    };

    match key.code {
        KeyCode::Up => preview(state, picker.selected.saturating_sub(1)),
        KeyCode::Down => preview(state, (picker.selected + 1).min(last)),
        KeyCode::Home => preview(state, 0),
        KeyCode::End => preview(state, last),
        KeyCode::Enter => {
            state.overlay = None;
            vec![Effect::SaveConfig]
        }
        KeyCode::Esc => {
            state.overlay = None;
            state.appearance.select(picker.original, now, fade);
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn on_menu(state: &mut AppState, cursor: MenuCursor, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    let all = menus(state);
    let count = isize::try_from(all.len()).unwrap_or(1);
    let switch = |delta: isize| {
        let index = usize::try_from((isize::try_from(cursor.menu).unwrap_or(0) + delta).rem_euclid(count)).unwrap_or(0);
        MenuCursor { menu: index, item: all.get(index).and_then(crate::Menu::first_enabled) }
    };
    let step = |delta: isize| MenuCursor { item: all[cursor.menu].step(cursor.item, delta), ..cursor };

    let next = match key.code {
        KeyCode::Left => Some(switch(PREVIOUS)),
        KeyCode::Right => Some(switch(NEXT)),
        KeyCode::Up => Some(step(PREVIOUS)),
        KeyCode::Down => Some(step(NEXT)),
        KeyCode::Enter => {
            state.overlay = None;
            let action = cursor.item.and_then(|item| all[cursor.menu].item(item)).map(|item| item.action);
            return action.map(|action| actions::perform(state, action, now)).unwrap_or_default();
        }
        KeyCode::Esc | KeyCode::F(_) => None,
        _ => Some(cursor),
    };
    state.overlay = next.map(Overlay::Menu);
    Vec::new()
}

fn on_prompt(state: &mut AppState, mut prompt: Prompt, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc => {
            state.overlay = None;
            Vec::new()
        }
        KeyCode::Enter => {
            state.overlay = None;
            submit(state, prompt.purpose, prompt.input.text().trim(), now)
        }
        _ => {
            if prompt.input.apply(key) == Edit::Changed {
                state.overlay = Some(Overlay::Prompt(prompt));
            }
            Vec::new()
        }
    }
}

fn submit(state: &mut AppState, purpose: PromptPurpose, text: &str, now: Instant) -> Vec<Effect> {
    if text.is_empty() {
        return Vec::new();
    }
    match purpose {
        PromptPurpose::Select | PromptPurpose::Unselect => {
            let mark = purpose == PromptPurpose::Select;
            match Pattern::parse(text) {
                Ok(pattern) => {
                    let side = state.active;
                    let count = state.panel_mut(side).mark_matching(&pattern, mark);
                    state.notice = (count == 0).then(|| format!("Nothing matches {text}"));
                }
                Err(error) => state.notice = Some(error.to_string()),
            }
            Vec::new()
        }
        PromptPurpose::Mkdir => run_operation(state, "mkdir", &ops::mkdir(text), Some(first_component(text)), now),
        PromptPurpose::Compress { sources } => match ArchiveFormat::detect(text) {
            Some(format) => {
                let command = ops::compress(text, format, sources.iter().map(String::as_str));
                run_operation(state, "compress", &command, Some(text.to_owned()), now)
            }
            None => {
                state.notice = Some(format!("Unknown archive type for {text} · try .tar.gz, .zip or .7z"));
                Vec::new()
            }
        },
        PromptPurpose::Copy { sources } => {
            let dest = resolve(state, text);
            run_operation(state, "copy", &ops::copy(sources.iter().map(String::as_str), &dest), None, now)
        }
        PromptPurpose::Move { sources } => {
            let dest = resolve(state, text);
            run_operation(state, "move", &ops::move_to(sources.iter().map(String::as_str), &dest), None, now)
        }
    }
}

fn on_confirm(state: &mut AppState, confirm: Confirm, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    let decided = match key.code {
        KeyCode::Char(ACCEPT_KEY) => Some(true),
        KeyCode::Char(REJECT_KEY) | KeyCode::Esc => Some(false),
        KeyCode::Enter => Some(confirm.accept),
        KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
            state.overlay = Some(Overlay::Confirm(Confirm { accept: !confirm.accept, ..confirm }));
            return Vec::new();
        }
        _ => return Vec::new(),
    };

    state.overlay = None;
    match (decided, confirm.purpose) {
        (Some(true), ConfirmPurpose::Delete { sources }) => {
            run_operation(state, "delete", &ops::remove(sources.iter().map(String::as_str)), None, now)
        }
        _ => Vec::new(),
    }
}

fn resolve(state: &AppState, text: &str) -> PathBuf {
    state.active_panel().dir().join(text)
}

fn first_component(text: &str) -> String {
    text.split('/').find(|part| !part.is_empty()).unwrap_or(text).to_owned()
}
