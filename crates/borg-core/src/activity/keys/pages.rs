//! The keys of the page with the focus: moving through its list, filtering and sorting the
//! processes, searching memory and paging through the hits, reading the page again, and the
//! snapshot's bytes and page map.

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::form;
use crate::{
    AppState, Effect,
    activity::{
        Activity, ActivityPage, ActivityRequest, ActivitySort, CENSUS_LEGEND, SNAPSHOT_LIMIT, census_grid, chrome_rows,
    },
};

pub(crate) fn on_key(state: &mut AppState, key: &KeyEvent) -> Vec<Effect> {
    let side = state.activity.side.unwrap_or(state.active);
    let rect =
        state.layout(state.epoch).panels[if state.activity.preview_focused { side.other() } else { side }.index()];
    let activity = &mut state.activity;
    if let Some(form) = activity.search_form.take() {
        return form::on_key(activity, form, key);
    }
    // Typing edits the filter; the arrows and paging keys still move through the matches.
    let moving = matches!(
        key.code,
        KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End
    );
    if activity.filtering && !moving {
        filter_key(activity, key.code);
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
            activity.search_form = Some(form::new());
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) && scanning(activity) => {
            return vec![Effect::CancelMemorySearch];
        }
        KeyCode::Char('n' | 'N') if activity.page == ActivityPage::Search && activity.pending.is_none() => {
            return next_hits(activity);
        }
        KeyCode::Char(' ') => return pause(activity),
        KeyCode::Char('a' | 'A') if activity.pending.is_none() => {
            if let Some(request) = activity.retry.as_ref().and_then(ActivityRequest::elevated) {
                return activity.request(request);
            }
        }
        KeyCode::Char('r' | 'R') if activity.pending.is_none() => return reload(activity),
        KeyCode::Char(c) if activity.page == ActivityPage::Snapshot => {
            return snapshot_key(activity, c, rows, width, state.panels[state.active.index()].dir());
        }
        KeyCode::Char(c) if activity.page == ActivityPage::Processes => sort_key(activity, c),
        _ => {}
    }
    Vec::new()
}

/// Typing into the processes' filter: Enter keeps it and Esc clears it.
fn filter_key(activity: &mut Activity, code: KeyCode) {
    match code {
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
}

/// Whether a memory search or an integrity check is under way, which Ctrl-C stops.
fn scanning(activity: &Activity) -> bool {
    activity.pending.as_ref().is_some_and(|(_, request)| {
        matches!(request, ActivityRequest::Search { .. } | ActivityRequest::Integrity { .. })
    })
}

/// N goes on to the last search's next hits, when there are more.
fn next_hits(activity: &mut Activity) -> Vec<Effect> {
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
    Vec::new()
}

/// Space pauses the sampling, or goes on with it, taking a sample at once.
fn pause(activity: &mut Activity) -> Vec<Effect> {
    activity.paused = !activity.paused;
    if !activity.paused && activity.pending.is_none() { activity.request(ActivityRequest::Sample) } else { Vec::new() }
}

/// R reads again what the page shows: the integrity check, the snapshot, the regions or the
/// open files, or else takes a sample.
fn reload(activity: &mut Activity) -> Vec<Effect> {
    let process = activity.selected;
    let request = match activity.page {
        ActivityPage::Integrity => process.map(|process| ActivityRequest::Integrity { process, admin: false }),
        ActivityPage::Snapshot => activity.recapture_request(),
        ActivityPage::Regions => process.map(|process| ActivityRequest::Regions { process, admin: false }),
        ActivityPage::Files | ActivityPage::FileDetails => {
            process.map(|process| ActivityRequest::Files { process, admin: false })
        }
        _ => Some(ActivityRequest::Sample),
    };
    request.map(|request| activity.request(request)).unwrap_or_default()
}

/// The snapshot's keys: text or bytes, the change since the last capture, the next or previous
/// block of data, the bytes after or before the captured ones, and exporting them into the
/// active panel's folder.
fn snapshot_key(activity: &mut Activity, c: char, rows: usize, width: usize, folder: &Path) -> Vec<Effect> {
    match c {
        'h' | 'H' => {
            activity.text = !activity.text;
            activity.offset = 0;
        }
        'd' | 'D' => activity.diff = !activity.diff,
        ']' | '[' => {
            if let Some(row) = activity.data_block(activity.snapshot_columns(width), c == ']') {
                activity.offset = 0;
                activity.move_by(row as isize, rows, width);
            }
        }
        '>' | '<' if activity.pending.is_none() => return capture_beside(activity, c == '>'),
        'e' | 'E' => return export(activity, folder),
        _ => {}
    }
    Vec::new()
}

/// Captures the bytes right after the snapshot, or those before it within its region.
fn capture_beside(activity: &mut Activity, after: bool) -> Vec<Effect> {
    let start = activity.snapshot.as_ref().zip(activity.shown_region()).and_then(|(shown, region)| {
        if after {
            Some(shown.address.saturating_add(shown.bytes.len() as u64))
        } else {
            (shown.address > region.start)
                .then(|| shown.address.saturating_sub(SNAPSHOT_LIMIT as u64).max(region.start))
        }
    });
    if let Some(request) = start.and_then(|start| activity.capture_at(start, after)) {
        return activity.request(request);
    }
    Vec::new()
}

/// Saves the snapshot's bytes into `folder`, named for the process, the address and the capture.
fn export(activity: &Activity, folder: &Path) -> Vec<Effect> {
    let Some(snapshot) = &activity.snapshot else { return Vec::new() };
    let name = format!("memory-{}-{:x}-{}.bin", snapshot.process.pid, snapshot.address, snapshot.captured);
    vec![Effect::ExportMemory { path: folder.join(name), bytes: snapshot.bytes.clone() }]
}

/// The processes' keys: a column's key sorts by it, again turns the order round, and T shows
/// the processes as a tree.
fn sort_key(activity: &mut Activity, c: char) {
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

/// Moves over the page map, or opens the page under the cursor in bytes with Enter.
fn map_key(activity: &mut Activity, code: KeyCode, rows: usize, width: usize) -> Option<Vec<Effect>> {
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
