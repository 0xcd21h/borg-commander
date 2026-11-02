//! Clicks and the mouse wheel, once the screen has said what lies under them. Each goes to the
//! editor, the monitor beside the panels, the window open over them or the panels themselves.

use std::time::Instant;

use crate::{Action, AppState, Effect, MenuCursor, Overlay, Side, View, actions, menus};

mod dialogs;
mod monitors;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    ActivityExpand {
        preview: bool,
        expand: bool,
    },
    ActivityHide,
    ActivitySearchField(usize),
    ActivityRow {
        index: usize,
        open: bool,
        preview: bool,
    },
    ActivitySort(crate::ActivitySort),
    ActivityControl {
        preview: bool,
        code: crossterm::event::KeyCode,
    },
    DockerExpand {
        details: bool,
        expand: bool,
    },
    DockerHide,
    DockerRow {
        index: usize,
        open: bool,
    },
    DockerControl {
        details: bool,
        code: crossterm::event::KeyCode,
    },
    DockerShellScroll(isize),
    NetworkExpand {
        details: bool,
        expand: bool,
    },
    NetworkHide,
    NetworkRow {
        index: usize,
        open: bool,
    },
    NetworkSort(crate::NetworkSort),
    NetworkPage(crate::NetworkPage),
    NetworkControl {
        details: bool,
        code: crossterm::event::KeyCode,
    },
    TreeNode {
        side: Side,
        index: usize,
    },
    PromptField {
        option: bool,
    },
    ResultControl {
        side: Side,
        action: Action,
    },
    Collection {
        index: usize,
        open: bool,
    },
    CollectionScroll(isize),
    NewCollection,
    SearchField(usize),
    /// A recent search in the search dialog, by its place in the history.
    SearchRecent(usize),
    /// Opens the list of what searches leave out.
    EditExclusions,
    /// A click in the editor's text, at a line and screen column; `select` extends the
    /// selection, as a drag or a Shift-click does, and `word` selects the word, as a double click does.
    EditorClick {
        slot: crate::EditorSlot,
        line: usize,
        column: usize,
        select: bool,
        word: bool,
    },
    /// Scrolls the editor's text by lines.
    EditorScroll(crate::EditorSlot, isize),
    /// A field or button of the editor's dialog, by its place among them.
    EditorControl(crate::EditorSlot, usize),
    /// A row of the open list dialog: a theme, a language, a setting, a job, a host, a way
    /// to connect or an item Time Machine lists.
    DialogRow(usize),
    /// A right-click on a row of the Time Machine window, which marks it as in the panels.
    DialogMark(usize),
    /// A click beside a dialog that closes on one, as Esc does.
    DismissDialog,
    /// A row of that list: a switch, a pattern or the row for adding one.
    Exclusion(usize),
    SearchStart,
    Confirm(bool),
    /// The mouse wheel over the approval dialog or the agent log.
    DialogScroll(isize),
    ResultSort {
        side: Side,
        key: crate::SortKey,
    },
    Action(Action),
    Menu(usize),
    MenuItem(usize),
    DismissMenu,
    DismissNotice,
    FocusPreview,
    Setting(usize),
    ScrollDocument(isize),
    Panel {
        side: Side,
        index: usize,
        open: bool,
        mark: bool,
    },
    ScrollPanel {
        side: Side,
        delta: isize,
    },
    /// A folder in the picker above the command line, counted from the bottom.
    CdChoice(usize),
}

pub(crate) fn on_pointer(state: &mut AppState, pointer: Pointer, now: Instant) -> Vec<Effect> {
    state.idle_since = now;
    state.intro = None;
    let resting = state.screensaver;
    state.screensaver = false;
    state.escaped = None;
    if let Pointer::ScrollDocument(delta) = pointer {
        if !resting && state.overlay.is_none() && !state.shell.captured() {
            scroll_document(state, delta);
        }
        return Vec::new();
    }
    // An agent's dialogs show over the editor and the viewer, and take their clicks.
    let agent = matches!(state.overlay, Some(Overlay::Approval(_) | Overlay::AgentLog { .. }));
    if state.editor.is_some() && !agent {
        return if resting { Vec::new() } else { crate::editor::on_pointer(state, crate::EditorSlot::Screen, pointer) };
    }
    if let Pointer::EditorClick { slot, .. } | Pointer::EditorScroll(slot, _) | Pointer::EditorControl(slot, _) =
        pointer
        && let crate::EditorSlot::Panel(side) = slot
    {
        if resting || state.overlay.is_some() || state.effective_view() != View::Panels {
            return Vec::new();
        }
        // A click goes to the panel's editor; the wheel scrolls it where it is.
        if !matches!(pointer, Pointer::EditorScroll(..)) {
            state.active = side;
        }
        return crate::editor::on_pointer(state, slot, pointer);
    }
    if resting
        || state.shell.captured()
        || ((state.viewer.is_some() || state.effective_view() != View::Panels) && !agent)
    {
        return Vec::new();
    }
    state.notice = None;
    // Finding in a panel ends at a click, which then acts on the panels.
    if let Some(Overlay::Find(find)) = state.overlay.clone() {
        crate::find::on_key(state, find, &crossterm::event::KeyCode::Esc.into());
    }
    match (pointer, state.overlay.clone()) {
        (Pointer::Menu(index), None | Some(Overlay::Menu(_))) => open_menu(state, index),
        (_, Some(overlay)) => dialogs::on_pointer(state, pointer, overlay, now),
        (_, None) => on_panels(state, pointer, now),
    }
}

/// The wheel over the viewer's text, or over the preview's beside the panels.
fn scroll_document(state: &mut AppState, delta: isize) {
    let pager = if let Some(crate::Viewer { content: crate::Content::Text(pager), .. }) = state.viewer.as_mut() {
        Some(pager)
    } else if let Some(crate::Peeked { peek: crate::Peek::Text(pager), .. }) = state.peeked.as_mut() {
        Some(pager)
    } else {
        None
    };
    let Some(pager) = pager else { return };
    let result = if delta < 0 { pager.scroll_up(delta.unsigned_abs()) } else { pager.scroll_down(delta as usize) };
    if let Err(error) = result {
        state.notice = Some(error.to_string());
    }
}

/// Opens a menu of the menu bar at its first item that can be chosen.
fn open_menu(state: &mut AppState, index: usize) -> Vec<Effect> {
    if let Some(menu) = menus(state).get(index) {
        let cursor = MenuCursor { menu: index, item: menu.first_enabled() };
        state.last_menu = Some(cursor);
        state.overlay = Some(Overlay::Menu(cursor));
    }
    Vec::new()
}

/// A click or the wheel with no window open: on a monitor beside the panels, or on the panels,
/// their trees and their results.
fn on_panels(state: &mut AppState, pointer: Pointer, now: Instant) -> Vec<Effect> {
    match pointer {
        Pointer::CdChoice(row) => crate::folders::pick(state, row),
        Pointer::ActivityExpand { .. }
        | Pointer::ActivityHide
        | Pointer::ActivitySearchField(_)
        | Pointer::ActivityControl { .. }
        | Pointer::ActivityRow { .. }
        | Pointer::ActivitySort(_)
            if state.activity.side.is_some() =>
        {
            monitors::activity(state, pointer, now)
        }
        Pointer::DockerExpand { .. }
        | Pointer::DockerHide
        | Pointer::DockerRow { .. }
        | Pointer::DockerShellScroll(_)
        | Pointer::DockerControl { .. }
            if state.docker.is_open() =>
        {
            monitors::containers(state, pointer, now)
        }
        Pointer::NetworkExpand { .. }
        | Pointer::NetworkHide
        | Pointer::NetworkRow { .. }
        | Pointer::NetworkSort(_)
        | Pointer::NetworkPage(_)
        | Pointer::NetworkControl { .. }
            if state.network.is_open() =>
        {
            monitors::network(state, pointer, now)
        }
        Pointer::TreeNode { side, index } if state.panel(side).is_tree() => toggle_tree_node(state, side, index),
        Pointer::ResultControl { side, action }
            if state.panel(side).is_results() && matches!(action, Action::SetExpanded(_) | Action::BackResults) =>
        {
            focus(state, side);
            actions::perform(state, action, now)
        }
        Pointer::FocusPreview if state.aside.is_some() => {
            state.preview_focused = true;
            Vec::new()
        }
        Pointer::ResultSort { side, key } => sort_by(state, side, key),
        Pointer::Action(action) => actions::perform(state, action, now),
        Pointer::Panel { side, index, open, mark } if takes_clicks(state, side) => {
            click_row(state, side, index, open, mark, now)
        }
        Pointer::ScrollPanel { side, delta } if takes_clicks(state, side) => {
            focus(state, side);
            state.panel_mut(side).move_by(delta);
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// Whether the panel on `side` takes clicks: any does, unless a preview shows beside the active
/// one in its place.
fn takes_clicks(state: &AppState, side: Side) -> bool {
    state.aside.is_none() || side == state.active
}

/// Makes the panel on `side` the active one, taking the keys back from the preview.
fn focus(state: &mut AppState, side: Side) {
    state.preview_focused = false;
    state.active = side;
}

/// A click on a folder of a panel's tree opens or closes it.
fn toggle_tree_node(state: &mut AppState, side: Side, index: usize) -> Vec<Effect> {
    state.active = side;
    let delta = index as isize - state.panel(side).cursor() as isize;
    state.panel_mut(side).move_by(delta);
    state.panel_mut(side).toggle_tree_node();
    Vec::new()
}

/// A click on a column's title sorts by it, and a second one turns the order round. Sizes, times
/// and file counts start reversed.
fn sort_by(state: &mut AppState, side: Side, key: crate::SortKey) -> Vec<Effect> {
    state.active = side;
    let previous = state.panel(side).sort();
    let reverse = if previous.key == key {
        !previous.reverse
    } else {
        matches!(key, crate::SortKey::Size | crate::SortKey::Modified | crate::SortKey::Files)
    };
    state.panel_mut(side).set_sort(crate::Sort { key, reverse });
    Vec::new()
}

/// A click on a panel's row puts the cursor there; a right click also marks it and a double
/// click opens it.
fn click_row(state: &mut AppState, side: Side, index: usize, open: bool, mark: bool, now: Instant) -> Vec<Effect> {
    focus(state, side);
    let panel = state.panel_mut(side);
    if index >= panel.len() {
        return Vec::new();
    }
    let delta = index as isize - panel.cursor() as isize;
    panel.move_by(delta);
    if mark {
        panel.toggle_mark();
    }
    if open { actions::perform(state, Action::Open, now) } else { Vec::new() }
}
