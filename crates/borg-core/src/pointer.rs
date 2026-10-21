use std::time::Instant;

use crate::{Action, AppState, Effect, MenuCursor, Overlay, Side, View, actions, menus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    ActivityExpand { preview: bool, expand: bool },
    ActivityHide,
    ActivitySearchField(usize),
    ActivityRow { index: usize, open: bool, preview: bool },
    ActivitySort(crate::ActivitySort),
    ActivityControl { preview: bool, code: crossterm::event::KeyCode },
    TreeNode { side: Side, index: usize },
    PromptField { option: bool },
    ResultControl { side: Side, action: Action },
    Collection { index: usize, open: bool },
    CollectionScroll(isize),
    NewCollection,
    SearchField(usize),
    SearchStart,
    Confirm(bool),
    ResultSort { side: Side, key: crate::SortKey },
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
        (Pointer::ActivityExpand { preview, expand }, None) if state.activity.side.is_some() => {
            state.activity.preview_focused = preview;
            state.activity.expanded = expand;
        }
        (Pointer::ActivityHide, None) if state.activity.side.is_some() => state.activity.side = None,

        (Pointer::ActivitySearchField(index), None) if state.activity.side.is_some() => {
            state.activity.preview_focused = true;
            if let Some(form) = state.activity.search_form.as_mut() {
                form.selected = index;
            }
            if index > 0 {
                return crate::activity::on_key(state, &crossterm::event::KeyCode::Char(' ').into());
            }
        }

        (Pointer::ActivityControl { preview, code }, None) if state.activity.side.is_some() => {
            state.activity.preview_focused = preview;
            return crate::activity::route_key(state, &code.into(), now);
        }
        (Pointer::ActivityRow { index, open, preview }, None) if state.activity.side.is_some() => {
            state.activity.preview_focused = preview;
            let page = state.activity.page;
            let context = state.activity.focused_page();
            let cursor = match context {
                crate::ActivityPage::Processes => state.activity.cursor,
                crate::ActivityPage::Search => state.activity.search_cursor,
                crate::ActivityPage::Files => state.activity.file_cursor,
                _ => state.activity.region_cursor,
            };
            state.activity.page = context;
            let side = state.activity.side.unwrap();
            let rect = state.layout(now).panels[if preview { side.other() } else { side }.index()];
            state.activity.move_by(
                index as isize - cursor as isize,
                usize::from(rect.height.saturating_sub(if context == crate::ActivityPage::Search { 7 } else { 6 })),
                usize::from(rect.width.saturating_sub(2)),
            );
            state.activity.page = page;
            if !preview
                && matches!(context, crate::ActivityPage::Regions | crate::ActivityPage::Search)
                && cursor != index
            {
                state.activity.schedule_preview(now);
            }
            if open {
                return crate::activity::route_key(state, &crossterm::event::KeyCode::Enter.into(), now);
            }
        }
        (Pointer::ActivitySort(sort), None) if state.activity.side.is_some() => {
            state.activity.preview_focused = false;
            let key = match sort {
                crate::ActivitySort::Pid => 'p',
                crate::ActivitySort::Name => 'n',
                crate::ActivitySort::Memory => 'm',
                crate::ActivitySort::Cpu => 'c',
                crate::ActivitySort::Growth => 'g',
                crate::ActivitySort::Read => 'i',
                crate::ActivitySort::Write => 'o',
            };
            return crate::activity::route_key(state, &crossterm::event::KeyCode::Char(key).into(), now);
        }
        (Pointer::TreeNode { side, index }, None) if state.panel(side).is_tree() => {
            state.active = side;
            let delta = index as isize - state.panel(side).cursor() as isize;
            state.panel_mut(side).move_by(delta);
            state.panel_mut(side).toggle_tree_node();
        }
        (Pointer::PromptField { option }, Some(Overlay::Prompt(mut prompt))) => {
            prompt.option_focused = option;
            if option && let crate::PromptPurpose::Copy { optimize_hdd, .. } = &mut prompt.purpose {
                *optimize_hdd = !*optimize_hdd;
            }
            state.overlay = Some(Overlay::Prompt(prompt));
        }
        (Pointer::ResultControl { side, action }, None)
            if state.panel(side).is_results() && matches!(action, Action::SetExpanded(_) | Action::BackResults) =>
        {
            state.active = side;
            state.preview_focused = false;
            return actions::perform(state, action, now);
        }
        (Pointer::Confirm(accept), Some(Overlay::Confirm(_))) => {
            return crate::overlays::on_key(
                state,
                &crossterm::event::KeyCode::Char(if accept { 'y' } else { 'n' }).into(),
                now,
            );
        }
        (Pointer::Collection { index, open }, Some(Overlay::Collections(mut picker))) if picker.creating.is_none() => {
            picker.selected = index.min(state.collections.len().saturating_sub(1));
            state.overlay = Some(Overlay::Collections(picker));
            if open {
                return crate::overlays::on_key(state, &crossterm::event::KeyCode::Enter.into(), now);
            }
        }
        (Pointer::CollectionScroll(delta), Some(Overlay::Collections(mut picker))) if picker.creating.is_none() => {
            picker.selected =
                picker.selected.saturating_add_signed(delta).min(state.collections.len().saturating_sub(1));
            state.overlay = Some(Overlay::Collections(picker));
        }
        (Pointer::NewCollection, Some(Overlay::Collections(mut picker))) => {
            picker.creating = Some(crate::LineInput::new(String::new()));
            state.overlay = Some(Overlay::Collections(picker));
        }
        (Pointer::SearchField(selected), Some(Overlay::Search(mut form))) => {
            form.selected = selected.min(crate::discovery::SEARCH_FIELDS - 1);
            let toggle = matches!(form.selected, 2 | 5);
            state.overlay = Some(Overlay::Search(form));
            if toggle {
                return crate::overlays::on_key(state, &crossterm::event::KeyCode::Right.into(), now);
            }
        }
        (Pointer::SearchStart, Some(Overlay::Search(_) | Overlay::Collections(_))) => {
            return crate::overlays::on_key(state, &crossterm::event::KeyCode::Enter.into(), now);
        }
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
        (Pointer::Setting(selected), Some(Overlay::Settings { .. } | Overlay::Plugins { .. })) => {
            return crate::overlays::toggle_setting(state, selected, now);
        }
        (Pointer::FocusPreview, None) if state.aside.is_some() => state.preview_focused = true,
        (Pointer::DismissMenu, Some(Overlay::Menu(_))) => state.overlay = None,
        (Pointer::ResultSort { side, key }, None) => {
            state.active = side;
            let previous = state.panel(side).sort();
            state.panel_mut(side).set_sort(crate::Sort {
                key,
                reverse: if previous.key == key {
                    !previous.reverse
                } else {
                    matches!(key, crate::SortKey::Size | crate::SortKey::Modified | crate::SortKey::Files)
                },
            });
        }
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
