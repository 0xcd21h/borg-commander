//! The monitor's keys: in its lists, its memory map and its forms.

use super::{
    Activity, ActivityPage, ActivityRequest, ActivitySort, CENSUS_LEGEND, MemorySearchForm, MemorySearchMode,
    RegionRow, RowKey, SNAPSHOT_LIMIT, census_grid, check_integrity, chrome_rows,
};
use crate::{AppState, Effect};
use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent};
use std::time::Instant;

pub(crate) fn on_key(state: &mut AppState, key: &KeyEvent) -> Vec<Effect> {
    let side = state.activity.side.unwrap_or(state.active);
    let rect =
        state.layout(state.epoch).panels[if state.activity.preview_focused { side.other() } else { side }.index()];
    let activity = &mut state.activity;
    if let Some(mut form) = activity.search_form.take() {
        let mut submit = false;
        match key.code {
            KeyCode::Esc => {
                activity.error = None;
                return Vec::new();
            }
            KeyCode::Tab | KeyCode::Down => form.selected = (form.selected + 1) % 4,
            KeyCode::BackTab | KeyCode::Up => form.selected = (form.selected + 3) % 4,
            KeyCode::Enter => submit = true,
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.selected == 1 => {
                let modes = MemorySearchMode::ALL;
                let at = modes.iter().position(|&mode| mode == form.mode).unwrap_or(0);
                let step = if key.code == KeyCode::Left { modes.len() - 1 } else { 1 };
                form.mode = modes[(at + step) % modes.len()];
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.selected == 2 => {
                form.match_case = !form.match_case
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.selected == 3 => {
                form.snapshot_only = !form.snapshot_only
            }
            _ if form.selected == 0 => {
                form.input.apply(key);
            }
            _ => {}
        }
        if submit {
            let valid = (form.mode.strings() || !form.input.text().is_empty())
                && form.input.text().len() <= 512
                && (form.mode != MemorySearchMode::Hex || {
                    let value = form.input.text().split_whitespace().collect::<String>();
                    !value.is_empty() && value.len() % 2 == 0 && value.chars().all(|c| c.is_ascii_hexdigit())
                });
            if !valid {
                activity.error = Some(tr!(memory_query_invalid).into());
            } else if let Some(process) = activity.selected {
                let snapshot =
                    if form.snapshot_only { activity.snapshot.clone().filter(|s| s.process == process) } else { None };
                if form.snapshot_only && snapshot.is_none() {
                    activity.error = Some(tr!(memory_no_snapshot).into());
                } else {
                    activity.search_progress = Some((0, 0, 0));
                    return activity.request(ActivityRequest::Search {
                        process,
                        query: form.input.text().to_owned(),
                        mode: form.mode,
                        match_case: form.match_case,
                        snapshot,
                        start: None,
                        admin: false,
                    });
                }
            }
        }
        activity.search_form = Some(form);
        return Vec::new();
    }
    // Typing edits the filter; the arrows and paging keys still move through the matches.
    let moving = matches!(
        key.code,
        KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End
    );
    if activity.filtering && !moving {
        match key.code {
            KeyCode::Enter => activity.filtering = false,
            KeyCode::Esc => {
                activity.filtering = false;
                activity.filter.clear();
            }
            KeyCode::Backspace => {
                activity.filter.pop();
            }
            KeyCode::Char(c) => activity.filter.push(c),
            _ => {}
        }
        activity.rebuild();
        return Vec::new();
    }

    let rows = usize::from(rect.height.saturating_sub(chrome_rows(activity.page))).max(1);
    let width = usize::from(rect.width.saturating_sub(2));
    if activity.page == ActivityPage::Snapshot
        && activity.map
        && let Some(effects) = map_key(activity, key.code, rows, width)
    {
        return effects;
    }
    match key.code {
        KeyCode::Down => activity.move_by(1, rows, width),
        KeyCode::Up => activity.move_by(-1, rows, width),
        KeyCode::PageDown => activity.move_by(rows as isize, rows, width),
        KeyCode::PageUp => activity.move_by(-(rows as isize), rows, width),
        KeyCode::Home => activity.move_by(isize::MIN, rows, width),
        KeyCode::End => activity.move_by(isize::MAX, rows, width),
        KeyCode::Char('/') if activity.page == ActivityPage::Processes => activity.filtering = true,
        KeyCode::Char('/')
            if !matches!(activity.page, ActivityPage::Processes | ActivityPage::Files | ActivityPage::FileDetails) =>
        {
            activity.search_form = Some(MemorySearchForm {
                input: crate::LineInput::default(),
                mode: MemorySearchMode::Text,
                match_case: true,
                snapshot_only: false,
                selected: 0,
            });
        }
        KeyCode::Char('c')
            if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
                && activity.pending.as_ref().is_some_and(|(_, r)| {
                    matches!(r, ActivityRequest::Search { .. } | ActivityRequest::Integrity { .. })
                }) =>
        {
            return vec![Effect::CancelMemorySearch];
        }
        KeyCode::Char('n' | 'N') if activity.page == ActivityPage::Search && activity.pending.is_none() => {
            if let (Some(report), Some(ActivityRequest::Search { process, query, mode, match_case, snapshot, .. })) =
                (&activity.search_report, &activity.last_search)
                && let Some(start) = report.next
            {
                return activity.request(ActivityRequest::Search {
                    process: *process,
                    query: query.clone(),
                    mode: *mode,
                    match_case: *match_case,
                    snapshot: snapshot.clone(),
                    start: Some(start),
                    admin: false,
                });
            }
        }
        KeyCode::Char(' ') => {
            activity.paused = !activity.paused;
            if !activity.paused && activity.pending.is_none() {
                return activity.request(ActivityRequest::Sample);
            }
        }
        KeyCode::Char('a' | 'A') if activity.pending.is_none() => {
            if let Some(request) = activity.retry.as_ref().and_then(ActivityRequest::elevated) {
                return activity.request(request);
            }
        }
        KeyCode::Char('r' | 'R') if activity.pending.is_none() => {
            if activity.page == ActivityPage::Integrity {
                if let Some(process) = activity.selected {
                    return activity.request(ActivityRequest::Integrity { process, admin: false });
                }
            } else if activity.page == ActivityPage::Snapshot {
                if let Some(request) = activity.recapture_request() {
                    return activity.request(request);
                }
            } else if activity.page == ActivityPage::Regions {
                if let Some(process) = activity.selected {
                    return activity.request(ActivityRequest::Regions { process, admin: false });
                }
            } else if matches!(activity.page, ActivityPage::Files | ActivityPage::FileDetails) {
                if let Some(process) = activity.selected {
                    return activity.request(ActivityRequest::Files { process, admin: false });
                }
            } else {
                return activity.request(ActivityRequest::Sample);
            }
        }
        KeyCode::Char('h' | 'H') if activity.page == ActivityPage::Snapshot => {
            activity.text = !activity.text;
            activity.offset = 0;
        }
        KeyCode::Char('d' | 'D') if activity.page == ActivityPage::Snapshot => activity.diff = !activity.diff,
        KeyCode::Char(c @ (']' | '[')) if activity.page == ActivityPage::Snapshot => {
            if let Some(row) = activity.data_block(activity.snapshot_columns(width), c == ']') {
                activity.offset = 0;
                activity.move_by(row as isize, rows, width);
            }
        }
        KeyCode::Char(c @ ('>' | '<')) if activity.page == ActivityPage::Snapshot && activity.pending.is_none() => {
            let start = activity.snapshot.as_ref().zip(activity.shown_region()).and_then(|(shown, region)| match c {
                '>' => Some(shown.address.saturating_add(shown.bytes.len() as u64)),
                _ => (shown.address > region.start)
                    .then(|| shown.address.saturating_sub(SNAPSHOT_LIMIT as u64).max(region.start)),
            });
            if let Some(request) = start.and_then(|start| activity.capture_at(start, c == '>')) {
                return activity.request(request);
            }
        }
        KeyCode::Char('e' | 'E') if activity.page == ActivityPage::Snapshot => {
            if let Some(snapshot) = &activity.snapshot {
                let path = state.panels[state.active.index()]
                    .dir()
                    .join(format!("memory-{}-{:x}-{}.bin", snapshot.process.pid, snapshot.address, snapshot.captured));
                return vec![Effect::ExportMemory { path, bytes: snapshot.bytes.clone() }];
            }
        }
        KeyCode::Char(c) if activity.page == ActivityPage::Processes => {
            let sort = match c.to_ascii_lowercase() {
                'm' => Some(ActivitySort::Memory),
                'c' => Some(ActivitySort::Cpu),
                'g' => Some(ActivitySort::Growth),
                'n' => Some(ActivitySort::Name),
                'p' => Some(ActivitySort::Pid),
                'i' => Some(ActivitySort::Read),
                'o' => Some(ActivitySort::Write),
                't' => {
                    activity.tree = !activity.tree;
                    None
                }
                _ => None,
            };
            if let Some(sort) = sort {
                if activity.sort == sort {
                    activity.reverse = !activity.reverse;
                } else {
                    activity.sort = sort;
                    activity.reverse = sort != ActivitySort::Name && sort != ActivitySort::Pid;
                }
            }
            activity.rebuild();
        }
        _ => {}
    }
    Vec::new()
}

/// Moves over the page map, or opens the page under the cursor in bytes with Enter.
pub(super) fn map_key(activity: &mut Activity, code: KeyCode, rows: usize, width: usize) -> Option<Vec<Effect>> {
    let census = activity.census.as_ref()?;
    if code == KeyCode::Enter {
        let address = census.address(activity.census_cursor);
        let start = address - address % census.page.max(1);
        activity.map = false;
        return Some(activity.capture_at(start, false).map(|request| activity.request(request)).unwrap_or_default());
    }
    let height = rows.saturating_sub(CENSUS_LEGEND);
    let (columns, _, span) = census_grid(census.pages.len(), width, height);
    let cell = (activity.census_cursor / span) as isize;
    let last = (census.pages.len().saturating_sub(1) / span) as isize;
    let step = match code {
        KeyCode::Left => -1,
        KeyCode::Right => 1,
        KeyCode::Up => -(columns as isize),
        KeyCode::Down => columns as isize,
        KeyCode::PageUp => -((columns * height) as isize),
        KeyCode::PageDown => (columns * height) as isize,
        KeyCode::Home => -cell,
        KeyCode::End => last - cell,
        _ => return None,
    };
    activity.census_cursor = ((cell + step).clamp(0, last) as usize) * span;
    Some(Vec::new())
}

pub(crate) fn route_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    if key.modifiers == crossterm::event::KeyModifiers::ALT.union(crossterm::event::KeyModifiers::SHIFT)
        && matches!(key.code, KeyCode::Left | KeyCode::Right)
    {
        return crate::actions::perform(
            state,
            crate::Action::NudgeSplit(if key.code == KeyCode::Right {
                crate::keymap::SPLIT_STEP
            } else {
                -crate::keymap::SPLIT_STEP
            }),
            now,
        );
    }

    if key.code == KeyCode::Char('w') && key.modifiers == crossterm::event::KeyModifiers::ALT {
        state.activity.expanded = !state.activity.expanded;
        return Vec::new();
    }
    if state.activity.search_form.is_some() {
        return on_key(state, key);
    }
    if !state.activity.filtering && key.modifiers.is_empty() {
        match key.code {
            KeyCode::Char('1') => {
                if state.activity.selected.is_none() {
                    return Vec::new();
                }
                state.activity.preview_focused = false;
                if state.activity.pending.as_ref().is_some_and(|(_, r)| !matches!(r, ActivityRequest::Sample)) {
                    state.activity.pending = None;
                    state.activity.next = None;
                }
                if state.activity.regions.is_empty() {
                    return enter_map(state);
                }
                state.activity.list = ActivityPage::Regions;
                state.activity.page = ActivityPage::Snapshot;
                state.activity.schedule_preview(now);
                return Vec::new();
            }
            KeyCode::Char('2') => {
                let activity = &mut state.activity;
                let Some(process) = activity.selected else {
                    return Vec::new();
                };
                activity.list = ActivityPage::Files;
                activity.page = ActivityPage::FileDetails;
                activity.preview_focused = false;
                activity.preview_due = None;
                if !activity.files_loaded {
                    return activity.request(ActivityRequest::Files { process, admin: false });
                }
                if activity.pending.as_ref().is_some_and(|(_, r)| !matches!(r, ActivityRequest::Sample)) {
                    activity.pending = None;
                    activity.next = None;
                }
                return Vec::new();
            }
            _ => {}
        }
    }
    if key.code == KeyCode::Char('f') && key.modifiers == crossterm::event::KeyModifiers::CONTROL {
        if state.activity.list == ActivityPage::Files {
            return Vec::new();
        }
        state.activity.preview_focused = true;
        if !matches!(state.activity.list, ActivityPage::Search | ActivityPage::Integrity) {
            state.activity.results_origin = state.activity.list;
        }
        if state.activity.page == ActivityPage::Processes {
            state.activity.page = ActivityPage::Memory;
        }
        return on_key(state, &KeyCode::Char('/').into());
    }

    if key.code == KeyCode::Char('o') && key.modifiers == crossterm::event::KeyModifiers::CONTROL {
        return crate::actions::perform(state, crate::Action::ToggleShell, now);
    }
    if key.code == KeyCode::F(9) {
        return crate::actions::perform(state, crate::Action::OpenMenuBar, now);
    }
    if key.code == KeyCode::F(1) {
        return crate::actions::perform(state, crate::Action::About, now);
    }
    if key.code == KeyCode::F(10) {
        return crate::actions::perform(state, crate::Action::Quit, now);
    }
    if key.code == KeyCode::Char('m') && key.modifiers == crossterm::event::KeyModifiers::ALT {
        state.activity.side = None;
        return Vec::new();
    }
    if key.code == KeyCode::Tab {
        state.activity.filtering = false;
        state.activity.preview_focused = !state.activity.preview_focused;
        return Vec::new();
    }
    if let Some(path) = state.activity.previewable_file() {
        if key.code == KeyCode::F(3) {
            return crate::viewer::open(state, path);
        }
        // The focused preview scrolls; the keys that act on the process keep working.
        if state.activity.preview_focused
            && matches!(
                key.code,
                KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::PageUp
                    | KeyCode::PageDown
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::Left
                    | KeyCode::Right
            )
        {
            if let Some(crate::Peeked { peek: crate::Peek::Text(pager), .. }) = state.activity.file_preview.as_mut()
                && let Err(error) = crate::viewer::scroll(pager, key.code)
            {
                state.activity.error = Some(tr!(preview_error, error = error));
            }
            return Vec::new();
        }
    }
    if key.code == KeyCode::Esc {
        let activity = &mut state.activity;
        activity.error = None;
        activity.retry = None;
        if activity.preview_focused {
            activity.preview_focused = false;
            return Vec::new();
        }
        if activity.list == ActivityPage::Processes {
            // Esc stops and clears a filter before it closes the monitor.
            if activity.filtering || !activity.filter.is_empty() {
                activity.filtering = false;
                activity.filter.clear();
                activity.rebuild();
            } else {
                activity.side = None;
            }
        } else {
            if activity.pending.as_ref().is_some_and(|(_, r)| !matches!(r, ActivityRequest::Sample)) {
                activity.pending = None;
                activity.next = None;
            }
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
        return Vec::new();
    }
    if key.code == KeyCode::Enter && !state.activity.filtering {
        if state.activity.list == ActivityPage::Processes {
            return enter_map(state);
        }
        if state.activity.list == ActivityPage::Regions
            && (state.activity.selected_group().is_some() || state.activity.selected_file().is_some())
        {
            state.activity.preview_focused = false;
            toggle(&mut state.activity, true, now);
            return Vec::new();
        }
        if state.activity.preview_focused && state.activity.map && state.activity.census.is_some() {
            return on_key(state, key);
        }
        state.activity.preview_focused = true;
        if state.activity.preview_due.is_some() {
            state.activity.preview_due = Some(now);
        }
        return Vec::new();
    }
    if key.code == KeyCode::Char('i')
        && key.modifiers.is_empty()
        && matches!(state.activity.list, ActivityPage::Regions | ActivityPage::Search | ActivityPage::Integrity)
        && let Some(process) = state.activity.selected
    {
        return check_integrity(&mut state.activity, process);
    }
    if key.code == KeyCode::Char('c')
        && key.modifiers.is_empty()
        && state.activity.list == ActivityPage::Search
        && state.activity.next_copy()
    {
        state.activity.schedule_preview(now);
        return Vec::new();
    }
    if state.activity.list == ActivityPage::Regions && key.code == KeyCode::Char('m') && key.modifiers.is_empty() {
        state.activity.map = !state.activity.map;
        state.activity.schedule_preview(now);
        return Vec::new();
    }
    if !state.activity.preview_focused && state.activity.list == ActivityPage::Regions {
        match key.code {
            KeyCode::Char('g' | 'G') => {
                let address = state.activity.selected_region().map(|r| r.start);
                state.activity.set_grouped(!state.activity.grouped);
                state.activity.region_cursor =
                    address.and_then(|start| state.activity.find_row(&RowKey::Region(start))).unwrap_or(0);
                state.activity.tops[1] = 0;
                state.activity.schedule_preview(now);
                return Vec::new();
            }
            KeyCode::Left | KeyCode::Right => {
                toggle(&mut state.activity, key.code == KeyCode::Right, now);
                return Vec::new();
            }
            _ => {}
        }
    }
    if matches!(key.code, KeyCode::Char('r' | 'R'))
        && state.activity.preview_focused
        && state.activity.page == ActivityPage::Snapshot
    {
        if let Some(request) = state.activity.recapture_request() {
            return state.activity.request(request);
        }
        return Vec::new();
    }
    let page = state.activity.page;
    let context = state.activity.focused_page();
    let cursor = state.activity.list_cursor(context);
    state.activity.page = context;
    let effects = on_key(state, key);
    state.activity.page = page;
    if state.activity.search_form.is_some() {
        if state.activity.list != ActivityPage::Search {
            state.activity.results_origin = state.activity.list;
        }
        state.activity.preview_focused = true;
    }
    if matches!(context, ActivityPage::Regions | ActivityPage::Search | ActivityPage::Integrity)
        && cursor != state.activity.list_cursor(context)
    {
        state.activity.schedule_preview(now);
    }
    effects
}

/// Right opens the selected group or file and steps into it; Left closes the selected
/// group or file, or the one around the selected row, and selects it.
pub(super) fn toggle(activity: &mut Activity, expand: bool, now: Instant) {
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

pub(super) fn enter_map(state: &mut AppState) -> Vec<Effect> {
    let activity = &mut state.activity;
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
