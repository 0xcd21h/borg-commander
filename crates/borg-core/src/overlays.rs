use borg_i18n::tr;
use std::{path::PathBuf, time::Instant};

use crossterm::event::{KeyCode, KeyEvent};

use borg_jobs::Spec;

use crate::{
    AppState, Confirm, ConfirmPurpose, Effect, JobKind, MenuCursor, Overlay, Pattern, Picker, Prompt, PromptPurpose,
    actions::{self, run_operation},
    input::Edit,
    jobs,
    menu::menus,
    ops::{self, ArchiveFormat},
};

const PREVIOUS: isize = -1;
const NEXT: isize = 1;
const ACCEPT_KEY: char = 'y';
const REJECT_KEY: char = 'n';

pub(crate) fn on_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    match state.overlay.clone() {
        Some(Overlay::Game2048) => crate::game2048::on_key(state, key),
        Some(Overlay::Collections(picker)) => crate::discovery::collections_key(state, picker, key),
        Some(Overlay::Search(form)) => crate::discovery::search_key(state, form, key),
        Some(Overlay::About(about)) => crate::delight::on_about_key(state, about, key, now),
        Some(Overlay::Find(find)) => crate::find::on_key(state, find, key),
        Some(Overlay::Question(question)) => crate::jobs::on_question_key(state, question, key),
        Some(Overlay::Jobs(list)) => crate::jobs::on_list_key(state, list, key),
        Some(Overlay::Settings { selected } | Overlay::Plugins { selected }) => on_settings(state, selected, key, now),
        Some(Overlay::Themes(picker)) => on_themes(state, picker, key, now),
        Some(Overlay::Languages(picker)) => on_languages(state, picker, key),
        Some(Overlay::Menu(cursor)) => on_menu(state, cursor, key, now),
        Some(Overlay::Prompt(prompt)) => on_prompt(state, prompt, key, now),
        Some(Overlay::Confirm(confirm)) => on_confirm(state, confirm, key, now),
        Some(Overlay::Approval(approval)) => crate::agent::on_approval_key(state, approval, key, now),
        Some(Overlay::AgentLog { scroll }) => crate::agent::on_log_key(state, scroll, key),
        Some(Overlay::Connect(connect)) => crate::agent::on_connect_key(state, connect, key),
        Some(Overlay::Remotes(browser)) => crate::remote::on_key(state, browser, key),
        Some(Overlay::TimeMachine) => crate::backup::on_key(state, key, now),
        None => Vec::new(),
    }
}

pub(crate) fn on_paste(state: &mut AppState, text: &str) -> Vec<Effect> {
    let line = text.lines().next().unwrap_or_default();
    let input = match state.overlay.as_mut() {
        Some(Overlay::Collections(crate::CollectionPicker { creating: Some(input), .. })) => input,
        Some(Overlay::Search(form)) => match form.selected {
            0 => &mut form.pattern,
            1 => &mut form.root,
            3 => &mut form.minimum,
            4 => &mut form.files,
            _ => return Vec::new(),
        },
        Some(Overlay::Prompt(prompt)) if !prompt.option_focused => &mut prompt.input,
        Some(Overlay::Find(find)) => &mut find.input,
        Some(Overlay::Connect(connect)) => {
            match crate::agent::connect_field(connect, state.agent.connect.ssh.is_some()) {
                Some(input) => input,
                None => return Vec::new(),
            }
        }
        _ => return Vec::new(),
    };
    line.chars().for_each(|c| {
        input.apply(&KeyEvent::from(KeyCode::Char(c)));
    });
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
    state.last_menu = Some(cursor);

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
    if let PromptPurpose::RemoteName { browser, identity, session, original } = &prompt.purpose {
        if key.code == KeyCode::Esc {
            state.overlay = Some(Overlay::Remotes(*browser.clone()));
            return Vec::new();
        }
        if key.code == KeyCode::Enter {
            if let Err(error) = crate::RemoteNames::validate(prompt.input.text()) {
                state.notice = Some(error);
                state.overlay = Some(Overlay::Prompt(prompt));
                return Vec::new();
            }
            let name = prompt.input.text().trim();
            let effect = Effect::SaveRemoteName {
                identity: identity.clone(),
                session: session.clone(),
                name: if name == original { String::new() } else { name.into() },
            };
            state.overlay = Some(Overlay::Remotes(*browser.clone()));
            return vec![effect];
        }
    }
    if prompt.purpose.option().is_some() {
        match key.code {
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {
                prompt.option_focused = !prompt.option_focused;
                state.overlay = Some(Overlay::Prompt(prompt));
                return Vec::new();
            }
            KeyCode::Char(' ') if prompt.option_focused => {
                prompt.purpose.toggle_option();
                state.overlay = Some(Overlay::Prompt(prompt));
                return Vec::new();
            }
            KeyCode::Enter | KeyCode::Esc => {}
            _ if prompt.option_focused => return Vec::new(),
            _ => {}
        }
    }
    if let PromptPurpose::BackupAdd { fixed } = prompt.purpose {
        match key.code {
            KeyCode::Esc => {
                state.overlay = Some(Overlay::TimeMachine);
                return Vec::new();
            }
            KeyCode::Enter => return crate::backup::add_path(state, prompt.input.text().trim(), fixed),
            _ => {}
        }
    }
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
    if text.is_empty() && purpose != PromptPurpose::ResultsFilter {
        return Vec::new();
    }
    match purpose {
        PromptPurpose::RemoteName { .. } | PromptPurpose::BackupAdd { .. } => Vec::new(),
        PromptPurpose::Search { root } => {
            let hidden = state.settings.preferences.show_hidden;
            match crate::results::name_search(state, root, text, hidden, true, Default::default()) {
                Ok(request) => crate::results::start_search(state, request),
                Err(error) => {
                    state.notice = Some(error.message());
                    Vec::new()
                }
            }
        }
        PromptPurpose::Collection => crate::results::collection(state, text, state.active),
        PromptPurpose::ResultsFilter => {
            let side = state.active;
            state.panel_mut(side).filter_results(text.to_owned());
            Vec::new()
        }
        PromptPurpose::Select | PromptPurpose::Unselect => {
            let mark = purpose == PromptPurpose::Select;
            match Pattern::parse(text) {
                Ok(pattern) => {
                    let side = state.active;
                    let count = state.panel_mut(side).mark_matching(&pattern, mark);
                    state.notice = (count == 0).then(|| tr!(nothing_matches, text = text));
                }
                Err(error) => state.notice = Some(error.to_string()),
            }
            Vec::new()
        }
        PromptPurpose::Mkdir if state.active_panel().mount.is_some() => crate::docker::mkdir(state, text),
        PromptPurpose::Mkdir => {
            run_operation(state, tr!(overlays_mkdir), &ops::mkdir(text), Some(first_component(text)), now)
        }
        PromptPurpose::RemoteCopy { sources, from, to } => {
            let target = state.panel(state.active.other()).dir().join(text);
            let (id, mut effects) = jobs::begin(state, JobKind::Copy, now);
            effects.push(Effect::StartRemoteCopy { id, from, to, sources, target });
            effects
        }
        PromptPurpose::DockerCopy { sources } => crate::docker::copy(state, sources, text, now),
        PromptPurpose::Compress { sources } => match ArchiveFormat::detect(text) {
            Some(format) => {
                let command = ops::compress(text, format, sources.iter().map(String::as_str));
                run_operation(state, tr!(overlays_compress), &command, Some(text.to_owned()), now)
            }
            None => {
                state.notice = Some(tr!(unknown_archive_type_for_try_tar_gz, text = text));
                Vec::new()
            }
        },
        PromptPurpose::Copy { sources, optimize_hdd } => {
            jobs::start(state, JobKind::Copy, Spec::Copy { sources, target: resolve(state, text), optimize_hdd }, now)
        }
        PromptPurpose::Move { sources } => {
            jobs::start(state, JobKind::Move, Spec::Move { sources, target: resolve(state, text) }, now)
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
        (answer, ConfirmPurpose::RemoteHostForget { machine, mut browser }) => {
            browser.error = None;
            if answer == Some(true) {
                browser.forgetting = Some(machine.clone());
                state.overlay = Some(Overlay::Remotes(*browser));
                vec![Effect::ForgetRemoteHost { machine }]
            } else {
                state.overlay = Some(Overlay::Remotes(*browser));
                vec![Effect::OpenRemotes]
            }
        }
        (answer, ConfirmPurpose::RemoteSessionDelete { machine, session, mut browser }) => {
            browser.error = None;
            if answer == Some(true) {
                browser.deleting = Some((machine.clone(), session.clone()));
                state.overlay = Some(Overlay::Remotes(*browser));
                vec![Effect::DeleteRemoteSession { machine, session }]
            } else {
                state.overlay = Some(Overlay::Remotes(*browser));
                vec![Effect::OpenRemotes]
            }
        }
        (Some(true), ConfirmPurpose::Delete { sources }) if state.active_panel().mount.is_some() => {
            crate::docker::delete(state, sources)
        }
        (Some(true), ConfirmPurpose::Delete { sources }) => {
            jobs::start(state, JobKind::Delete, Spec::Delete { sources }, now)
        }
        (Some(true), ConfirmPurpose::Docker { action, ids }) => crate::docker::start(state, action, ids),
        (Some(true), ConfirmPurpose::Quit | ConfirmPurpose::ShellExit { .. }) => {
            let mut effects = jobs::cancel_all(state);
            effects.push(Effect::Quit);
            effects
        }
        (Some(false), ConfirmPurpose::ShellExit { token }) => vec![Effect::CancelShellExit { token }],
        (answer, ConfirmPurpose::RemoteShellExit { token }) => {
            let close = answer == Some(true);
            if let (true, crate::Console::Remote { machine, session }) = (close, &state.console) {
                state.ending_remote = Some((machine.clone(), session.clone()));
            }
            let reply = if close { "yes" } else { "no" };
            vec![Effect::Write(format!("{token}:{reply}\n").into_bytes())]
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

pub(crate) fn toggle_setting(state: &mut AppState, selected: usize, now: Instant) -> Vec<Effect> {
    let Some(item) = crate::menu::checkbox_items(state).get(selected).cloned() else { return Vec::new() };
    let plugins = matches!(state.overlay, Some(Overlay::Plugins { .. }));
    let effects = actions::perform(state, item.action, now);
    state.overlay = Some(if plugins { Overlay::Plugins { selected } } else { Overlay::Settings { selected } });
    effects
}

fn on_settings(state: &mut AppState, selected: usize, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    let plugins = matches!(state.overlay, Some(Overlay::Plugins { .. }));
    let count = crate::menu::checkbox_items(state).len();
    let next = match key.code {
        KeyCode::Esc | KeyCode::F(10) => {
            state.overlay = None;
            return Vec::new();
        }
        KeyCode::Enter | KeyCode::Char(' ') => return toggle_setting(state, selected, now),
        KeyCode::Down | KeyCode::Tab => (selected + 1) % count,
        KeyCode::Up | KeyCode::BackTab => (selected + count - 1) % count,
        KeyCode::Home => 0,
        KeyCode::End => count - 1,
        _ => selected,
    };
    state.overlay =
        Some(if plugins { Overlay::Plugins { selected: next } } else { Overlay::Settings { selected: next } });
    Vec::new()
}

fn on_languages(state: &mut AppState, mut picker: Picker, key: &KeyEvent) -> Vec<Effect> {
    let last = borg_i18n::Locale::ALL.len() - 1;
    match key.code {
        KeyCode::Up => picker.selected = picker.selected.saturating_sub(1),
        KeyCode::Down => picker.selected = (picker.selected + 1).min(last),
        KeyCode::Home => picker.selected = 0,
        KeyCode::End => picker.selected = last,
        KeyCode::Enter => {
            let language = borg_i18n::Locale::ALL[picker.selected];
            state.settings.preferences.language = language;
            state.overlay = None;
            return vec![Effect::SetLanguage(language), Effect::SaveConfig];
        }
        KeyCode::Esc => {
            state.overlay = None;
            return Vec::new();
        }
        _ => {}
    }
    state.overlay = Some(Overlay::Languages(picker));
    Vec::new()
}
