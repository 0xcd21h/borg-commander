//! The process watcher: what a process starting looks like, the rules that find an unusual
//! start, the baseline Borg learns before it acts, what it does about a finding, and the
//! Alerts page that shows the lot. Everything here is data in and data out; the app watches
//! the machine, keeps the log and carries out the freeze.

use borg_i18n::tr;
use std::time::Instant;

use crate::{AppState, Effect, Side, View};

mod alerts;
mod baseline;
mod event;
mod keys;
mod response;
mod rules;

pub use alerts::{
    ALERTS_KEPT, Alert, AlertLog, AlertState, AlertsReply, AlertsRequest, Evidence, WatchSource, WatchStatus,
    filtered as alerts_filtered,
};
pub use baseline::{Allowance, Baseline, LEARNING_DAYS, PAIRS_LIMIT, ParentChild, review_ends};
pub use event::{Ancestor, COMMAND_CUT, COMMAND_KEPT, StartedProcess, StreamKind, kept_within};
pub(crate) use keys::{click, kill, paste, route_key};
pub use response::{
    ALERTS_PER_MINUTE, FIRST_PERSON, FREEZES_PER_MINUTE, Guard, Limits, NEVER_FROZEN, Outcome, Protection, Response,
    Watcher, decide,
};
pub use rules::{Finding, History, PRELOAD_VARIABLES, WatchRule, examine};

/// Rows the list pane's frame, summary, filter, column titles and footer take.
pub const ALERTS_CHROME_ROWS: u16 = 6;
const FILTER_LIMIT: usize = 128;
const PAGE: usize = 10;

/// The Alerts page: the log, what the filter keeps of it, and where the cursor is.
#[derive(Debug, Clone, Default)]
pub struct Alerts {
    pub side: Option<Side>,
    pub expanded: bool,
    pub log: AlertLog,
    pub loaded: bool,
    /// The alerts the filter keeps, by their place in the log.
    pub rows: Vec<usize>,
    cursor: usize,
    top: usize,
    chosen: Option<u64>,
    pub filter: String,
    pub filtering: bool,
    pub error: Option<String>,
    pub pending: Option<u64>,
    next_id: u64,
    /// Alerts nobody has read, which the key bar counts even while the page is closed.
    pub unread: usize,
}

impl Alerts {
    pub fn is_open(&self) -> bool {
        self.side.is_some()
    }

    pub fn typing(&self) -> bool {
        self.filtering
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn alert(&self, row: usize) -> Option<&Alert> {
        self.rows.get(row).and_then(|index| self.log.alerts.get(*index))
    }

    pub fn selected(&self) -> Option<&Alert> {
        self.alert(self.cursor)
    }

    pub fn list_start(&self, height: usize) -> usize {
        self.top.min(self.cursor).max(self.cursor.saturating_sub(height.max(1) - 1))
    }

    fn keep_visible(&mut self, height: usize) {
        self.top = self.list_start(height);
    }

    pub(crate) fn select(&mut self, row: usize) {
        let row = row.min(self.len().saturating_sub(1));
        self.cursor = row;
        self.chosen = self.alert(row).map(|alert| alert.id);
    }

    fn move_by(&mut self, delta: isize) {
        let last = self.len().saturating_sub(1) as isize;
        self.select((self.cursor as isize + delta).clamp(0, last.max(0)) as usize);
    }

    /// Filters the log again, keeping the cursor on the alert it was on.
    fn rebuild(&mut self) {
        self.rows = alerts_filtered(&self.log.alerts, &self.filter);
        let chosen = self.chosen;
        let found = chosen.and_then(|id| (0..self.len()).find(|&row| self.alert(row).map(|a| a.id) == Some(id)));
        self.cursor = found.unwrap_or(self.cursor.min(self.len().saturating_sub(1)));
        self.chosen = self.alert(self.cursor).map(|alert| alert.id);
    }

    fn request(&mut self, request: AlertsRequest) -> Vec<Effect> {
        self.next_id += 1;
        self.pending = Some(self.next_id);
        vec![Effect::Alerts { id: self.next_id, request }]
    }
}

pub(crate) fn open(state: &mut AppState) -> Vec<Effect> {
    if !state.settings.preferences.system_plugin {
        state.notice = Some(tr!(activity_disabled).into());
        return Vec::new();
    }
    state.overlay = None;
    state.activity.side = None;
    crate::docker::close(state);
    crate::network::close(state);
    crate::startup_items::close(state);
    let alerts = &mut state.alerts;
    alerts.side = Some(state.active);
    alerts.error = None;
    alerts.request(AlertsRequest::Open)
}

pub(crate) fn close(state: &mut AppState) {
    state.alerts.side = None;
    state.alerts.filtering = false;
}

/// Alt-L opens the page, and closes it while it is open.
pub(crate) fn toggle(state: &mut AppState) -> Vec<Effect> {
    if state.alerts.is_open() {
        close(state);
        return Vec::new();
    }
    open(state)
}

/// Reads the log again.
fn reread(state: &mut AppState) -> Vec<Effect> {
    state.alerts.error = None;
    state.alerts.request(AlertsRequest::Read)
}

/// Keeps the cursor in sight of the list pane.
pub(crate) fn sync(state: &mut AppState, now: Instant) {
    if !state.alerts.is_open() || state.view != View::Panels || state.overlay.is_some() {
        return;
    }
    let side = state.alerts.side.unwrap_or(state.active);
    let list = state.layout(now).panels[side.index()];
    if !list.is_empty() {
        state.alerts.keep_visible(usize::from(list.height.saturating_sub(ALERTS_CHROME_ROWS)));
    }
}

pub(crate) fn receive(state: &mut AppState, id: u64, result: Result<AlertsReply, String>) {
    let alerts = &mut state.alerts;
    if alerts.pending != Some(id) {
        return;
    }
    alerts.pending = None;
    match result {
        Ok(reply) => {
            alerts.log = reply.log;
            alerts.unread = alerts.log.unread();
            alerts.loaded = true;
            alerts.error = None;
            alerts.rebuild();
            if let Some(notice) = reply.notice {
                state.notice = Some(notice);
            }
        }
        Err(error) => alerts.error = Some(error),
    }
}

/// The log changed under Borg: the watcher wrote an alert, or another Borg marked them read.
/// The count of unread alerts follows even while the page is closed.
pub(crate) fn changed(state: &mut AppState, log: AlertLog) {
    let alerts = &mut state.alerts;
    alerts.log = log;
    alerts.unread = alerts.log.unread();
    alerts.loaded = true;
    if alerts.is_open() {
        alerts.rebuild();
    }
}

#[cfg(test)]
mod tests;
