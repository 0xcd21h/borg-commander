use std::time::Instant;

use crate::{Action, AppState, Effect, MenuCursor, Overlay, Side, View, actions, menus};

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
        line: usize,
        column: usize,
        select: bool,
        word: bool,
    },
    /// Scrolls the editor's text by lines.
    EditorScroll(isize),
    /// A field or button of the editor's dialog, by its place among them.
    EditorControl(usize),
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
}

pub(crate) fn on_pointer(state: &mut AppState, pointer: Pointer, now: Instant) -> Vec<Effect> {
    state.idle_since = now;
    state.intro = None;
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
    // An agent's dialogs show over the editor and the viewer, and take their clicks.
    let agent = matches!(state.overlay, Some(Overlay::Approval(_) | Overlay::AgentLog { .. }));
    if state.editor.is_some() && !agent {
        return if resting { Vec::new() } else { crate::editor::on_pointer(state, pointer) };
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
        (Pointer::ActivityExpand { preview, expand }, None) if state.activity.side.is_some() => {
            state.activity.preview_focused = preview;
            state.activity.expanded = expand;
        }
        (Pointer::ActivityHide, None) if state.activity.side.is_some() => state.activity.side = None,
        (Pointer::DockerExpand { details, expand }, None) if state.docker.is_open() => {
            state.docker.details_focused = details;
            state.docker.expanded = expand;
        }
        (Pointer::DockerHide, None) if state.docker.is_open() => crate::docker::close(state),
        (Pointer::DockerRow { index, open }, None) if state.docker.is_open() => {
            return crate::docker::click(state, index, open, now);
        }
        (Pointer::DockerShellScroll(lines), None) if state.docker.is_open() => {
            let scroll = borg_term::Scroll::Delta(-(lines as i32));
            return crate::docker::scroll_shell(state, scroll);
        }
        (Pointer::DockerControl { details, code }, None) if state.docker.is_open() => {
            state.docker.details_focused = details;
            return crate::docker::route_key(state, &code.into(), now);
        }

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
            let cursor = state.activity.list_cursor(context);
            state.activity.page = context;
            let side = state.activity.side.unwrap();
            let rect = state.layout(now).panels[if preview { side.other() } else { side }.index()];
            state.activity.move_by(
                index as isize - cursor as isize,
                usize::from(rect.height.saturating_sub(crate::activity::chrome_rows(context))),
                usize::from(rect.width.saturating_sub(2)),
            );
            state.activity.page = page;
            if !preview
                && matches!(
                    context,
                    crate::ActivityPage::Regions | crate::ActivityPage::Search | crate::ActivityPage::Integrity
                )
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
            if option {
                prompt.purpose.toggle_option();
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
        (Pointer::DialogScroll(delta), Some(Overlay::Approval(_) | Overlay::AgentLog { .. })) => {
            crate::agent::scroll_dialog(state, delta);
            return Vec::new();
        }
        (Pointer::DialogScroll(delta), Some(overlay)) => {
            if let Some((selected, count)) = list_position(state, &overlay) {
                let row = selected.saturating_add_signed(delta).min(count.saturating_sub(1));
                select_row(state, overlay, row, now);
            }
        }
        (Pointer::DialogRow(row), Some(overlay)) => select_row(state, overlay, row, now),
        (Pointer::DialogMark(row), Some(Overlay::TimeMachine)) => state.backup.window.point(row, true),
        (Pointer::DismissDialog, Some(_)) => {
            return crate::overlays::on_key(state, &crossterm::event::KeyCode::Esc.into(), now);
        }
        (Pointer::Confirm(accept), Some(Overlay::Confirm(_) | Overlay::Approval(_))) => {
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
            form.selected = selected.min(form.fields() - 1);
            let toggle = matches!(form.selected, 2 | 5 | 6);
            state.overlay = Some(Overlay::Search(form));
            if toggle {
                return crate::overlays::on_key(state, &crossterm::event::KeyCode::Right.into(), now);
            }
        }
        (Pointer::SearchRecent(at), Some(Overlay::Search(mut form))) => {
            form.take_recent(state, at);
            state.overlay = Some(Overlay::Search(form));
        }
        (Pointer::EditExclusions, Some(Overlay::Search(form))) => return crate::exclusions::open(state, form),
        (Pointer::Exclusion(row), Some(Overlay::Exclusions(mut editor))) if editor.editing.is_none() => {
            let effects = editor.activate(state, row);
            state.overlay = Some(Overlay::Exclusions(editor));
            return effects;
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

/// The chosen row of a list dialog and how many rows it has.
fn list_position(state: &AppState, overlay: &Overlay) -> Option<(usize, usize)> {
    Some(match overlay {
        Overlay::Themes(picker) => (picker.selected, state.appearance.choices().len()),
        Overlay::Languages(picker) => (picker.selected, borg_i18n::Locale::ALL.len()),
        Overlay::Settings { selected } | Overlay::Plugins { selected } => {
            (*selected, crate::menu::checkbox_items(state).len())
        }
        Overlay::Jobs(list) => (list.selected, state.jobs.len()),
        Overlay::Remotes(browser) if browser.show_clients => {
            (browser.client_selected, browser.clients.as_ref().map_or(0, Vec::len))
        }
        Overlay::Remotes(browser) => (browser.selected, browser.choices.len()),
        Overlay::Exclusions(editor) if editor.editing.is_none() => {
            (editor.selected, crate::ExclusionEditor::rows(state))
        }
        Overlay::TimeMachine => {
            let list = state.backup.window.list();
            (list.cursor, list.items.len())
        }
        _ => return None,
    })
}

/// Chooses `row` in a list dialog as the arrow keys would, previewing a theme as they do.
fn select_row(state: &mut AppState, overlay: Overlay, row: usize, now: Instant) {
    let Some((_, count)) = list_position(state, &overlay).or(matches!(overlay, Overlay::Connect(_)).then_some((0, 3)))
    else {
        return;
    };
    let row = row.min(count.saturating_sub(1));
    state.overlay = Some(match overlay {
        Overlay::Themes(picker) => {
            let fade = state.motion().fade;
            state.appearance.select(row, now, fade);
            Overlay::Themes(crate::Picker { selected: row, ..picker })
        }
        Overlay::Languages(picker) => Overlay::Languages(crate::Picker { selected: row, ..picker }),
        Overlay::Settings { .. } => Overlay::Settings { selected: row },
        Overlay::Plugins { .. } => Overlay::Plugins { selected: row },
        Overlay::Jobs(_) => Overlay::Jobs(crate::JobList { selected: row }),
        Overlay::Remotes(mut browser) => {
            if browser.show_clients {
                browser.client_selected = row;
            } else {
                browser.selected = row;
                browser.error = None;
            }
            Overlay::Remotes(browser)
        }
        Overlay::Connect(mut connect) => {
            connect.pick = row;
            Overlay::Connect(connect)
        }
        Overlay::Exclusions(mut editor) => {
            editor.selected = row;
            Overlay::Exclusions(editor)
        }
        Overlay::TimeMachine => {
            state.backup.window.point(row, false);
            Overlay::TimeMachine
        }
        overlay => overlay,
    });
}
