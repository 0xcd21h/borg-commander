use std::time::Instant;

use crate::{Action, AppState, Effect, MenuCursor, Overlay, Side, View, actions, menus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    Action(Action),
    Menu(usize),
    MenuItem(usize),
    DismissMenu,
    DismissNotice,
    FocusPreview,
    Setting(usize),
    ScrollDocument(isize),
    Panel { side: Side, index: usize, open: bool, mark: bool },
    ScrollPanel { side: Side, delta: isize },
}

pub(crate) fn on_pointer(state: &mut AppState, pointer: Pointer, now: Instant) -> Vec<Effect> {
    state.idle_since = now;
    let resting = state.screensaver;
    state.screensaver = false;
    state.escaped = None;
    if let Pointer::ScrollDocument(delta) = pointer {
        if !resting && state.overlay.is_none() && !state.shell.captured() {
            let pager = if let Some(crate::Viewer { content: crate::Content::Text(pager), .. }) = state.viewer.as_mut()
            {
                Some(pager)
            } else if let Some(crate::Peeked { peek: crate::Peek::Text(pager), .. }) = state.peeked.as_mut() {
                Some(pager)
            } else {
                None
            };
            if let Some(pager) = pager {
                let result =
                    if delta < 0 { pager.scroll_up(delta.unsigned_abs()) } else { pager.scroll_down(delta as usize) };
                if let Err(error) = result {
                    state.notice = Some(error.to_string());
                }
            }
        }
        return Vec::new();
    }
    if resting || state.shell.captured() || state.viewer.is_some() || state.effective_view() != View::Panels {
        return Vec::new();
    }
    state.notice = None;
    match (pointer, state.overlay.clone()) {
        (Pointer::Menu(index), None | Some(Overlay::Menu(_))) => {
            if let Some(menu) = menus(state).get(index) {
                let cursor = MenuCursor { menu: index, item: menu.first_enabled() };
                state.last_menu = Some(cursor);
                state.overlay = Some(Overlay::Menu(cursor));
            }
        }
        (Pointer::MenuItem(index), Some(Overlay::Menu(cursor))) => {
            if let Some(item) =
                menus(state).get(cursor.menu).and_then(|menu| menu.item(index)).filter(|item| item.enabled)
            {
                state.last_menu = Some(MenuCursor { item: Some(index), ..cursor });
                state.overlay = None;
                return actions::perform(state, item.action, now);
            }
        }
        (Pointer::Setting(selected), Some(Overlay::Settings { .. })) => {
            return crate::overlays::toggle_setting(state, selected, now);
        }
        (Pointer::FocusPreview, None) if state.aside.is_some() => state.preview_focused = true,
        (Pointer::DismissMenu, Some(Overlay::Menu(_))) => state.overlay = None,
        (Pointer::Action(action), None) => return actions::perform(state, action, now),
        (Pointer::Panel { side, index, open, mark }, None) if state.aside.is_none() || side == state.active => {
            state.preview_focused = false;
            state.active = side;
            let panel = state.panel_mut(side);
            if index >= panel.len() {
                return Vec::new();
            }
            let delta = index as isize - panel.cursor() as isize;
            panel.move_by(delta);
            if mark {
                panel.toggle_mark();
            }
            if open {
                return actions::perform(state, Action::Open, now);
            }
        }
        (Pointer::ScrollPanel { side, delta }, None) if state.aside.is_none() || side == state.active => {
            state.preview_focused = false;
            state.active = side;
            state.panel_mut(side).move_by(delta);
        }
        _ => {}
    }
    Vec::new()
}
