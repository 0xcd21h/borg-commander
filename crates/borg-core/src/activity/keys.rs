//! The monitor's keys. The ones for the whole monitor are here: switching pages, moving the
//! focus, the preview, Esc and Enter, and the lists' own keys. The keys of the page with the
//! focus are in `pages`, and the memory search form's in `form`.

use std::time::Instant;

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{Activity, ActivityPage, ActivityRequest, RegionRow, RowKey, check_integrity};
use crate::{Action, AppState, Effect, keymap::SPLIT_STEP};

mod form;
mod pages;

pub(crate) use pages::on_key;

pub(crate) fn route_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    let (code, modifiers) = (key.code, key.modifiers);
    if modifiers == KeyModifiers::ALT.union(KeyModifiers::SHIFT) && matches!(code, KeyCode::Left | KeyCode::Right) {
        let step = if code == KeyCode::Right { SPLIT_STEP } else { -SPLIT_STEP };
        return crate::actions::perform(state, Action::NudgeSplit(step), now);
    }
    if code == KeyCode::Char('w') && modifiers == KeyModifiers::ALT {
        state.activity.expanded = !state.activity.expanded;
        return Vec::new();
    }
    if state.activity.search_form.is_some() {
        return on_key(state, key);
    }
    if !state.activity.filtering && modifiers.is_empty() {
        match code {
            KeyCode::Char('1') => return open_regions(&mut state.activity, now),
            KeyCode::Char('2') => return open_files(&mut state.activity),
            _ => {}
        }
    }
    if code == KeyCode::Char('f') && modifiers == KeyModifiers::CONTROL {
        return find_in_memory(state);
    }
    if let Some(action) = panels_action(key) {
        return crate::actions::perform(state, action, now);
    }
    if code == KeyCode::Char('m') && modifiers == KeyModifiers::ALT {
        state.activity.side = None;
        return Vec::new();
    }
    if code == KeyCode::Tab {
        state.activity.filtering = false;
        state.activity.preview_focused = !state.activity.preview_focused;
        return Vec::new();
    }
    if let Some(path) = state.activity.previewable_file() {
        if code == KeyCode::F(3) {
            return crate::viewer::open(state, path);
        }
        // The focused preview scrolls; the keys that act on the process keep working.
        if state.activity.preview_focused && scrolls_preview(code) {
            scroll_preview(&mut state.activity, code);
            return Vec::new();
        }
    }
    match code {
        KeyCode::Esc => escape(&mut state.activity, now),
        KeyCode::Enter if !state.activity.filtering => enter(state, key, now),
        _ => match list_key(&mut state.activity, key, now) {
            Some(effects) => effects,
            None => in_focused_page(state, key, now),
        },
    }
}

/// Pastes into the memory search form's query, or into the processes' filter while it is typed.
pub(crate) fn paste(activity: &mut Activity, text: &str) {
    if let Some(form) = activity.search_form.as_mut() {
        form::paste(form, text);
    } else if activity.filtering {
        activity.filter.extend(text.chars().take(128));
        activity.rebuild();
    }
}

/// The panels' keys that work in the monitor too: the shell, the menu bar, About and quitting.
fn panels_action(key: &KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('o') if key.modifiers == KeyModifiers::CONTROL => Some(Action::ToggleShell),
        KeyCode::F(9) => Some(Action::OpenMenuBar),
        KeyCode::F(1) => Some(Action::About),
        KeyCode::F(10) => Some(Action::Quit),
        _ => None,
    }
}

/// 1 opens the selected process's memory regions, reading them first if need be.
fn open_regions(activity: &mut Activity, now: Instant) -> Vec<Effect> {
    if activity.selected.is_none() {
        return Vec::new();
    }
    activity.preview_focused = false;
    forget_request(activity);
    if activity.regions.is_empty() {
        return enter_map(activity);
    }
    activity.list = ActivityPage::Regions;
    activity.page = ActivityPage::Snapshot;
    activity.schedule_preview(now);
    Vec::new()
}

/// 2 opens the files the selected process holds open, listing them first if need be.
fn open_files(activity: &mut Activity) -> Vec<Effect> {
    let Some(process) = activity.selected else { return Vec::new() };
    activity.list = ActivityPage::Files;
    activity.page = ActivityPage::FileDetails;
    activity.preview_focused = false;
    activity.preview_due = None;
    if !activity.files_loaded {
        return activity.request(ActivityRequest::Files { process, admin: false });
    }
    forget_request(activity);
    Vec::new()
}

/// Ctrl-F opens the memory search in the preview, from any list but the open files. Its hits go
/// back with Esc to the list the search began in.
fn find_in_memory(state: &mut AppState) -> Vec<Effect> {
    let activity = &mut state.activity;
    if activity.list == ActivityPage::Files {
        return Vec::new();
    }
    activity.preview_focused = true;
    if !matches!(activity.list, ActivityPage::Search | ActivityPage::Integrity) {
        activity.results_origin = activity.list;
    }
    if activity.page == ActivityPage::Processes {
        activity.page = ActivityPage::Memory;
    }
    on_key(state, &KeyCode::Char('/').into())
}

fn scrolls_preview(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::Up
            | KeyCode::Down
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Left
            | KeyCode::Right
    )
}

fn scroll_preview(activity: &mut Activity, code: KeyCode) {
    if let Some(crate::Peeked { peek: crate::Peek::Text(pager), .. }) = activity.file_preview.as_mut()
        && let Err(error) = crate::viewer::scroll(pager, code)
    {
        activity.error = Some(tr!(preview_error, error = error));
    }
}

/// Esc takes the focus back from the preview, or else steps back a page. On the processes it
/// stops and clears a filter before it closes the monitor.
fn escape(activity: &mut Activity, now: Instant) -> Vec<Effect> {
    activity.error = None;
    activity.retry = None;
    if activity.preview_focused {
        activity.preview_focused = false;
    } else if activity.list != ActivityPage::Processes {
        step_back(activity, now);
    } else if activity.filtering || !activity.filter.is_empty() {
        activity.filtering = false;
        activity.filter.clear();
        activity.rebuild();
    } else {
        activity.side = None;
    }
    Vec::new()
}

/// Goes back from a search's hits or a check's findings to the list they began in, and from
/// the regions or the open files to the processes.
fn step_back(activity: &mut Activity, now: Instant) {
    forget_request(activity);
    activity.preview_due = None;
    activity.search_progress = None;
    activity.list = if matches!(activity.list, ActivityPage::Search | ActivityPage::Integrity) {
        activity.results_origin
    } else {
        ActivityPage::Processes
    };
    activity.page =
        if activity.list == ActivityPage::Processes { ActivityPage::Memory } else { ActivityPage::Snapshot };
    if activity.list == ActivityPage::Regions {
        activity.schedule_preview(now);
    }
}

/// Enter on the processes opens the selected one's memory map, and on the regions opens a
/// group or file. In the page map it opens the page under the cursor; anywhere else it gives
/// the preview the focus, showing it at once if it was to come after a pause.
fn enter(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    let activity = &mut state.activity;
    if activity.list == ActivityPage::Processes {
        return enter_map(activity);
    }
    if activity.list == ActivityPage::Regions
        && (activity.selected_group().is_some() || activity.selected_file().is_some())
    {
        activity.preview_focused = false;
        toggle(activity, true, now);
        return Vec::new();
    }
    if activity.preview_focused && activity.map && activity.census.is_some() {
        return on_key(state, key);
    }
    activity.preview_focused = true;
    if activity.preview_due.is_some() {
        activity.preview_due = Some(now);
    }
    Vec::new()
}

/// The keys of the regions, a search's hits and a check's findings: checking the process, the
/// next copy of a hit, the page map, grouping the regions and opening their groups, and
/// capturing the focused snapshot again.
fn list_key(activity: &mut Activity, key: &KeyEvent, now: Instant) -> Option<Vec<Effect>> {
    let plain = key.modifiers.is_empty();
    let regions = activity.list == ActivityPage::Regions;
    let findings = matches!(activity.list, ActivityPage::Regions | ActivityPage::Search | ActivityPage::Integrity);
    match key.code {
        KeyCode::Char('i') if plain && findings => {
            let process = activity.selected?;
            Some(check_integrity(activity, process))
        }
        KeyCode::Char('c') if plain && activity.list == ActivityPage::Search && activity.next_copy() => {
            activity.schedule_preview(now);
            Some(Vec::new())
        }
        KeyCode::Char('m') if plain && regions => {
            activity.map = !activity.map;
            activity.schedule_preview(now);
            Some(Vec::new())
        }
        KeyCode::Char('g' | 'G') if regions && !activity.preview_focused => {
            regroup(activity, now);
            Some(Vec::new())
        }
        KeyCode::Left | KeyCode::Right if regions && !activity.preview_focused => {
            toggle(activity, key.code == KeyCode::Right, now);
            Some(Vec::new())
        }
        KeyCode::Char('r' | 'R') if activity.preview_focused && activity.page == ActivityPage::Snapshot => {
            Some(activity.recapture_request().map(|request| activity.request(request)).unwrap_or_default())
        }
        _ => None,
    }
}

/// G groups the regions, or lists them one by one, keeping the cursor on the region it was on.
fn regroup(activity: &mut Activity, now: Instant) {
    let address = activity.selected_region().map(|region| region.start);
    activity.set_grouped(!activity.grouped);
    activity.region_cursor = address.and_then(|start| activity.find_row(&RowKey::Region(start))).unwrap_or(0);
    activity.tops[1] = 0;
    activity.schedule_preview(now);
}

/// Any other key goes to the page with the focus, as if it were the monitor's page. Moving to
/// another region, hit or finding previews it after a pause.
fn in_focused_page(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    let page = state.activity.page;
    let context = state.activity.focused_page();
    let cursor = state.activity.list_cursor(context);
    state.activity.page = context;
    let effects = on_key(state, key);
    let activity = &mut state.activity;
    activity.page = page;
    if activity.search_form.is_some() {
        if activity.list != ActivityPage::Search {
            activity.results_origin = activity.list;
        }
        activity.preview_focused = true;
    }
    if matches!(context, ActivityPage::Regions | ActivityPage::Search | ActivityPage::Integrity)
        && cursor != activity.list_cursor(context)
    {
        activity.schedule_preview(now);
    }
    effects
}

/// Gives up the request under way, unless it is only a sample, and the time set for the next.
fn forget_request(activity: &mut Activity) {
    if activity.pending.as_ref().is_some_and(|(_, request)| !matches!(request, ActivityRequest::Sample)) {
        activity.pending = None;
        activity.next = None;
    }
}

/// Right opens the selected group or file and steps into it; Left closes the selected
/// group or file, or the one around the selected row, and selects it.
fn toggle(activity: &mut Activity, expand: bool, now: Instant) {
    if !activity.grouped {
        return;
    }
    let items = activity.region_tree();
    let Some(item) = items.get(activity.region_cursor) else { return };
    let parent = |depth: usize| items[..activity.region_cursor].iter().rposition(|i| i.depth < depth);
    let target = if expand || item.expanded { Some(activity.region_cursor) } else { parent(item.depth) };
    let Some((target, row)) = target.map(|t| (t, items[t].row)).filter(|(_, row)| !matches!(row, RegionRow::Region(_)))
    else {
        return;
    };
    let open = |activity: &mut Activity, row: RegionRow, open: bool| match row {
        RegionRow::Group(group) if open => _ = activity.expanded_groups.insert(group),
        RegionRow::Group(group) => _ = activity.expanded_groups.remove(&group),
        RegionRow::File(first) => {
            let key = activity.file_key(first).to_owned();
            if open {
                activity.expanded_files.insert(key)
            } else {
                activity.expanded_files.remove(&key)
            };
        }
        RegionRow::Region(_) => {}
    };
    open(activity, row, expand);
    activity.refresh_tree();
    activity.region_cursor = (target + usize::from(expand)).min(activity.visible.len().saturating_sub(1));
    activity.schedule_preview(now);
}

/// Opens the selected process's memory map, reading its regions again.
fn enter_map(activity: &mut Activity) -> Vec<Effect> {
    let Some(process) = activity.selected else {
        return Vec::new();
    };
    activity.list = ActivityPage::Regions;
    activity.page = ActivityPage::Snapshot;
    activity.preview_focused = false;
    activity.results_origin = ActivityPage::Regions;
    activity.snapshot = None;
    activity.previous = None;
    activity.offset = 0;
    activity.from_search = false;
    activity.retry = None;
    activity.request(ActivityRequest::Regions { process, admin: false })
}
