//! Opening the monitor, asking for fresh data on time, and taking it in.

use super::{
    Activity, ActivityData, ActivityPage, ActivityRequest, FILE_PREVIEW_TOP, HISTORY, MemoryHistory, PROCESS_LIMIT,
    REFRESH, chrome_rows, merge_strings,
};
use crate::{AppState, Effect};
use borg_i18n::tr;
use std::{collections::HashSet, time::Instant};

pub(crate) fn open(state: &mut AppState) -> Vec<Effect> {
    if !state.settings.preferences.system_plugin {
        state.notice = Some(tr!(activity_disabled).into());
        return Vec::new();
    }
    state.overlay = None;
    crate::docker::close(state);
    state.activity.side = Some(state.active);
    state.activity.preview_focused = false;
    state.activity.paused = false;
    state.activity.page = ActivityPage::Memory;
    state.activity.list = ActivityPage::Processes;
    state.activity.preview_due = None;
    state.activity.request(ActivityRequest::Sample)
}

pub(crate) fn wake_at(state: &AppState) -> Option<Instant> {
    if state.activity.side.is_none() || state.view != crate::View::Panels || state.overlay.is_some() {
        return None;
    }
    state
        .activity
        .preview_due
        .into_iter()
        .chain((!state.activity.paused).then_some(state.activity.next).flatten())
        .min()
}

/// Asks for a preview of the selected open file, when it is a regular file, sized to the pane
/// under its details.
pub(crate) fn sync_file_preview(state: &mut AppState) -> Option<Effect> {
    let Some(side) = state.activity.side.filter(|_| state.view == crate::View::Panels) else {
        state.activity.file_previewing = None;
        return None;
    };
    let Some(path) = state.activity.previewable_file() else {
        state.activity.file_preview = None;
        state.activity.file_previewing = None;
        return None;
    };
    if state.overlay.is_some() {
        return None;
    }
    let area = state.layout(state.epoch).panels[side.other().index()];
    let size = (
        usize::from(area.width.saturating_sub(2)),
        usize::from(area.height.saturating_sub(chrome_rows(ActivityPage::FileDetails) + FILE_PREVIEW_TOP)),
    );
    let activity = &mut state.activity;
    if let Some(preview) = activity.file_preview.as_mut().filter(|preview| preview.path == path) {
        if let crate::Peek::Text(pager) = &mut preview.peek
            && let Err(error) = pager.resize(size.0, size.1)
        {
            activity.error = Some(tr!(preview_error, error = error));
        }
        return None;
    }
    activity.file_preview = None;
    if activity.file_previewing.as_ref() == Some(&path) {
        return None;
    }
    activity.file_previewing = Some(path.clone());
    Some(Effect::Peek { aside: crate::Aside::OpenFile, path, at: None, size })
}

pub(crate) fn receive_file_preview(activity: &mut Activity, preview: crate::Peeked) {
    if activity.file_previewing.as_ref() == Some(&preview.path) {
        activity.file_previewing = None;
        activity.file_preview = Some(preview);
    }
}

pub(crate) fn sync(state: &mut AppState, now: Instant) -> Vec<Effect> {
    if state.activity.side.is_none() || state.view != crate::View::Panels {
        if state.activity.side.is_none() {
            state.activity.preview_due = None;
        } else if state.activity.pending.as_ref().is_some_and(|(_, r)| matches!(r, ActivityRequest::Capture { .. })) {
            state.activity.preview_due = Some(now);
        }
        state.activity.pending = None;
        state.activity.next = None;
        return Vec::new();
    }
    if state.overlay.is_some() {
        return Vec::new();
    }
    let layout = state.layout(now);
    let side = state.activity.side.unwrap();
    let rect = layout.panels[side.other().index()];
    let list_rect = layout.panels[side.index()];
    let activity = &mut state.activity;
    if !list_rect.is_empty() {
        activity.keep_visible(activity.list, usize::from(list_rect.height.saturating_sub(chrome_rows(activity.list))));
    }
    if activity.preview_due.is_some_and(|due| due <= now) {
        activity.preview_due = None;
        if activity.list == ActivityPage::Search
            && let Some(ActivityRequest::Search { snapshot: Some(snapshot), .. }) = &activity.last_search
        {
            if let Some(hit) = activity.search_report.as_ref().and_then(|r| r.hits.get(activity.search_cursor)) {
                let columns = activity.snapshot_columns(usize::from(rect.width.saturating_sub(2)));
                let at = hit.address.saturating_sub(snapshot.address) as usize;
                activity.snapshot = Some(snapshot.clone());
                let rows = activity.snapshot_rows(columns);
                let row = rows.iter().rposition(|row| row.start() <= at).unwrap_or(0);
                activity.offset = row.saturating_sub(1);
            }
        } else if let Some(request) = activity.capture_request() {
            return activity.request(request);
        }
    }
    if !activity.paused
        && activity.pending.is_none()
        && activity.preview_due.is_none()
        && activity.next.is_none_or(|next| now >= next)
    {
        return activity.request(ActivityRequest::Sample);
    }
    Vec::new()
}

pub(crate) fn receive(state: &mut AppState, id: u64, result: Result<ActivityData, String>, now: Instant) {
    let activity = &mut state.activity;
    let Some((pending, request)) = activity.pending.clone() else { return };
    if pending != id || activity.side.is_none() {
        return;
    }
    activity.pending = None;
    activity.next = Some(now + REFRESH);
    let reads_memory = request.reads_memory();
    match result {
        Ok(ActivityData::Search(mut report)) => {
            if matches!(&request, ActivityRequest::Search { start: Some(_), .. })
                && let Some(previous) = &activity.search_report
            {
                report.scanned = report.scanned.saturating_add(previous.scanned).min(report.total);
                report.skipped = report.skipped.saturating_add(previous.skipped);
                merge_strings(previous, &mut report);
            }
            activity.retry = (report.skipped > 0).then(|| request.clone());
            activity.search_report = Some(report);
            activity.search_cursor = 0;
            activity.page = ActivityPage::Snapshot;
            activity.list = ActivityPage::Search;
            activity.preview_focused = false;
            activity.search_progress = None;
            activity.last_search = Some(request);
            let retry = activity.retry.clone();
            activity.schedule_preview(now);
            activity.retry = retry;
        }
        Ok(ActivityData::Integrity(report)) => {
            // Unread memory leaves checks undone, which the administrator retry can finish.
            activity.retry = (report.unreadable > 0).then(|| request.clone());
            activity.integrity = Some(report);
            activity.finding_cursor = 0;
            activity.tops[4] = 0;
            activity.page = ActivityPage::Snapshot;
            activity.list = ActivityPage::Integrity;
            activity.preview_focused = false;
            activity.search_progress = None;
            let retry = activity.retry.clone();
            activity.schedule_preview(now);
            activity.retry = retry;
        }
        Ok(ActivityData::Sample(mut sample)) => {
            sample.processes.truncate(PROCESS_LIMIT);
            let keys: HashSet<_> = sample.processes.iter().map(|p| p.key).collect();
            activity.histories.retain(|key, _| keys.contains(key));
            for process in &sample.processes {
                if let Some(bytes) = process.resident {
                    let h = activity.histories.entry(process.key).or_insert_with(|| MemoryHistory {
                        values: Vec::new(),
                        first: bytes,
                        peak: bytes,
                    });
                    h.peak = h.peak.max(bytes);
                    h.values.push(bytes);
                    if h.values.len() > HISTORY {
                        h.values.remove(0);
                    }
                }
            }
            activity.sample = sample;
            activity.rebuild();
        }
        Ok(ActivityData::Regions(regions)) => {
            let selected = activity.selected_row_key();
            activity.set_regions(regions);
            activity.region_cursor = selected.and_then(|key| activity.find_row(&key)).unwrap_or(0);
            activity.retry = None;
            activity.schedule_preview(now);
        }
        Ok(ActivityData::Files(files)) => {
            let selected = activity.files.get(activity.file_cursor).map(|f| (f.descriptor.clone(), f.name.clone()));
            activity.file_cursor = selected
                .and_then(|key| files.entries.iter().position(|f| (f.descriptor.clone(), f.name.clone()) == key))
                .unwrap_or(0);
            activity.files = files.entries;
            activity.files_skipped = files.skipped;
            activity.files_loaded = true;
            activity.retry = None;
        }
        Ok(ActivityData::Census(census)) => {
            activity.census_cursor = activity.census_cursor.min(census.pages.len().saturating_sub(1));
            activity.census = Some(census);
            activity.retry = None;
        }
        Ok(ActivityData::Snapshot(snapshot)) => {
            activity.previous = activity
                .snapshot
                .take()
                .filter(|old| old.process == snapshot.process && old.address == snapshot.address);
            activity.snapshot = Some(snapshot);
            activity.page = ActivityPage::Snapshot;
            activity.offset = 0;
            activity.retry = None;
        }
        Err(error) => {
            activity.search_progress = None;
            activity.error = Some(error);
            if !matches!(request, ActivityRequest::Sample) {
                activity.retry = Some(request);
            }
        }
    }
    // Asking for administrator reads nothing more when Borg already is one, or from memory macOS
    // keeps from every program.
    if activity.sample.administrator || (reads_memory && activity.protection().is_some()) {
        activity.retry = None;
    }
}
