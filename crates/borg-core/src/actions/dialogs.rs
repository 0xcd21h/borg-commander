//! The windows actions open over the panels: menus, prompts, pickers and the question before
//! quitting.

use borg_i18n::tr;

use crate::{
    AppState, Confirm, ConfirmPurpose, Effect, LineInput, MenuCursor, Overlay, Picker, Prompt, PromptPurpose, Side,
    menu::{RIGHT_MENU, menus},
};

pub(super) fn show(state: &mut AppState, overlay: Overlay) -> Vec<Effect> {
    state.overlay = Some(overlay);
    Vec::new()
}

pub(super) fn prompt(
    state: &mut AppState,
    title: &'static str,
    label: &str,
    value: &str,
    purpose: PromptPurpose,
) -> Vec<Effect> {
    show(
        state,
        Overlay::Prompt(Prompt {
            title,
            label: label.to_owned(),
            input: LineInput::new(value),
            purpose,
            option_focused: false,
        }),
    )
}

pub(super) fn open_menu(state: &mut AppState, menu: usize) -> Vec<Effect> {
    let item = menus(state).get(menu).and_then(crate::Menu::first_enabled);
    show(state, Overlay::Menu(MenuCursor { menu, item }))
}

/// F9: opens the menu last used, at its item when that can still be chosen, or the menu over
/// the active panel.
pub(super) fn open_menu_bar(state: &mut AppState, side: Side) -> Vec<Effect> {
    let all = menus(state);
    let resumed = state.last_menu.and_then(|last| {
        let menu = all.get(last.menu)?;
        let usable = |index: &usize| menu.item(*index).is_some_and(|item| item.enabled);
        Some(MenuCursor { item: last.item.filter(usable).or_else(|| menu.first_enabled()), ..last })
    });
    match resumed {
        Some(cursor) => show(state, Overlay::Menu(cursor)),
        None => open_menu(state, if side == Side::Left { 0 } else { RIGHT_MENU }),
    }
}

pub(super) fn pick_language(state: &mut AppState) -> Vec<Effect> {
    let current = state.settings.preferences.language as usize;
    show(state, Overlay::Languages(Picker { selected: current, original: current }))
}

pub(super) fn pick_theme(state: &mut AppState) -> Vec<Effect> {
    let current = state.appearance.current();
    show(state, Overlay::Themes(Picker { selected: current, original: current }))
}

/// F10: asks before quitting, naming the jobs or container shells that would end. Each panel
/// editor with unsaved changes asks first; quitting goes on once none does.
pub(super) fn confirm_quit(state: &mut AppState) -> Vec<Effect> {
    if crate::editor::ask_unsaved(state) {
        return Vec::new();
    }
    let message = if state.jobs.is_empty() && !state.docker.shells.is_empty() {
        match state.docker.shells.len() {
            1 => tr!(quit_shells_one).into(),
            count => tr!(quit_shells_many, count = count),
        }
    } else if state.jobs.is_empty() {
        tr!(quit_borg_commander).into()
    } else if state.jobs.len() == 1 {
        tr!(quit_jobs_one).into()
    } else {
        tr!(quit_jobs_many, count = state.jobs.len())
    };
    show(state, Overlay::Confirm(Confirm { title: tr!(quit), message, purpose: ConfirmPurpose::Quit, accept: false }))
}
