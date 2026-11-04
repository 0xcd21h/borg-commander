//! The entropy scan of every process: the regions whose sampled pages are as dense as packed,
//! compressed or encrypted bytes, ranked with code first, and the way from a hit to its pages.

use super::{Activity, ActivityPage, ActivityRequest, MemoryRegion, ProcessKey, RowKey};
use crate::{AppState, Effect, Overlay, looks_packed};
use borg_i18n::tr;
use borg_view::entropy::Tally;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Hits a scan keeps, the best ones; the rest are only counted.
pub const HIT_LIMIT: usize = 2000;

/// What a scan reads of every process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntropyScope {
    /// The executable regions, where dense bytes mean packed or encrypted code.
    Code,
    /// Every readable region.
    Memory,
}

impl EntropyScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Code => tr!(entropy_scope_code),
            Self::Memory => tr!(entropy_scope_memory),
        }
    }

    /// Whether a scan in this scope reads `region`.
    pub fn reads(self, region: &MemoryRegion) -> bool {
        region.readable() && (self == Self::Memory || region.permissions.contains('x'))
    }
}

/// What a hit's region holds, in the order hits rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum HitKind {
    /// Executable memory.
    Code,
    /// Memory the process filled itself.
    Own,
    /// A mapped file as the process found it.
    File,
}

impl HitKind {
    pub fn of(region: &MemoryRegion) -> Self {
        if region.permissions.contains('x') {
            Self::Code
        } else if region.own() {
            Self::Own
        } else {
            Self::File
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Code => tr!(entropy_kind_code),
            Self::Own => tr!(entropy_kind_own),
            Self::File => tr!(entropy_kind_file),
        }
    }
}

/// A region with at least one sampled page of high entropy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntropyHit {
    pub process: ProcessKey,
    pub name: String,
    pub region: MemoryRegion,
    pub kind: HitKind,
    /// The region's sampled pages, with the address of the first dense one.
    pub tally: Tally,
}

impl EntropyHit {
    /// Code mostly as dense as packed code, which a program's own code rarely is.
    pub fn packed(&self) -> bool {
        self.kind == HitKind::Code && looks_packed(&self.tally)
    }
}

/// What a scan found, or one part of it: the hits, and the processes it read, could not read
/// or did not reach.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntropyReport {
    pub hits: Vec<EntropyHit>,
    /// Hits left out past `HIT_LIMIT`.
    pub dropped: usize,
    /// Processes read.
    pub scanned: usize,
    /// Processes Borg may not read, which an administrator may.
    pub unreadable: Vec<ProcessKey>,
    /// Processes macOS keeps from every program, administrators too.
    pub protected: usize,
    /// Processes the scan did not reach before it stopped.
    pub remaining: Vec<ProcessKey>,
}

impl EntropyReport {
    /// Adds what a later part of the scan found.
    pub fn merge(&mut self, more: Self) {
        self.hits.extend(more.hits);
        self.dropped += more.dropped;
        self.scanned += more.scanned;
        self.unreadable.extend(more.unreadable);
        self.protected += more.protected;
        self.remaining.extend(more.remaining);
        self.rank();
    }

    /// Orders the hits: code first, then memory the process filled, then mapped files; within
    /// each, the larger share of dense pages first, then the densest page. Keeps the best
    /// `HIT_LIMIT`.
    pub fn rank(&mut self) {
        self.hits.sort_by(|a, b| {
            a.kind
                .cmp(&b.kind)
                .then(b.tally.share().total_cmp(&a.tally.share()))
                .then(b.tally.highest.cmp(&a.tally.highest))
                .then(a.process.pid.cmp(&b.process.pid))
                .then(a.region.start.cmp(&b.region.start))
        });
        if self.hits.len() > HIT_LIMIT {
            self.dropped += self.hits.len() - HIT_LIMIT;
            self.hits.truncate(HIT_LIMIT);
        }
    }
}

/// A scan of every process, under way or done, and the hit the list's cursor is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntropyScan {
    pub id: u64,
    pub scope: EntropyScope,
    pub report: EntropyReport,
    /// Processes the scan set out to read.
    pub total: usize,
    pub running: bool,
    /// Whether the run under way, or the last one, read as administrator.
    pub admin: bool,
    /// What the run under way reads as administrator, which goes back to the unreadable
    /// processes when the helper fails.
    targets: Vec<ProcessKey>,
    pub cursor: usize,
}

impl EntropyScan {
    pub fn selected(&self) -> Option<&EntropyHit> {
        self.report.hits.get(self.cursor)
    }

    /// Whether N can go on with processes the scan did not reach.
    pub fn continues(&self) -> bool {
        !self.running && !self.report.remaining.is_empty()
    }

    /// Whether A can read the processes Borg may not, as administrator.
    pub fn elevates(&self, administrator: bool) -> bool {
        !self.running && !administrator && !self.report.unreadable.is_empty()
    }
}

/// What the dialog E opens offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntropyChoice {
    Scan(EntropyScope),
    /// The hits of the last scan, which may still run.
    Results,
}

impl EntropyChoice {
    /// The letter that picks this choice in the dialog.
    pub fn key(self) -> char {
        match self {
            Self::Scan(EntropyScope::Code) => 'c',
            Self::Scan(EntropyScope::Memory) => 'm',
            Self::Results => 'l',
        }
    }
}

/// The dialog E opens on the process list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntropyDialog {
    pub selected: usize,
    /// Whether there is a last scan to go back to.
    pub results: bool,
}

impl EntropyDialog {
    pub fn choices(&self) -> Vec<EntropyChoice> {
        let mut choices = vec![EntropyChoice::Scan(EntropyScope::Code), EntropyChoice::Scan(EntropyScope::Memory)];
        if self.results {
            choices.push(EntropyChoice::Results);
        }
        choices
    }
}

/// Where Enter on a hit lands once the process's memory map and the region's page map arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Landing {
    pub region: u64,
    pub address: Option<u64>,
}

/// E opens the dialog that picks what to scan.
pub(crate) fn open(state: &mut AppState) -> Vec<Effect> {
    let results = state.activity.entropy.is_some();
    state.overlay = Some(Overlay::EntropyScan(EntropyDialog { selected: 0, results }));
    Vec::new()
}

pub(crate) fn on_dialog_key(state: &mut AppState, mut dialog: EntropyDialog, key: &KeyEvent) -> Vec<Effect> {
    let choices = dialog.choices();
    let last = choices.len() - 1;
    let choice = match crate::keyboard::latin(key).code {
        KeyCode::Esc | KeyCode::F(10) => {
            state.overlay = None;
            return Vec::new();
        }
        KeyCode::Up => {
            dialog.selected = dialog.selected.saturating_sub(1);
            None
        }
        KeyCode::Down => {
            dialog.selected = (dialog.selected + 1).min(last);
            None
        }
        KeyCode::Home => {
            dialog.selected = 0;
            None
        }
        KeyCode::End => {
            dialog.selected = last;
            None
        }
        KeyCode::Enter => choices.get(dialog.selected).copied(),
        KeyCode::Char(c) => choices.iter().copied().find(|choice| choice.key() == c.to_ascii_lowercase()),
        _ => None,
    };
    let Some(choice) = choice else {
        state.overlay = Some(Overlay::EntropyScan(dialog));
        return Vec::new();
    };
    state.overlay = None;
    match choice {
        EntropyChoice::Scan(scope) => start(&mut state.activity, scope),
        EntropyChoice::Results => {
            show(&mut state.activity);
            Vec::new()
        }
    }
}

/// Scans every process the last sample listed.
fn start(activity: &mut Activity, scope: EntropyScope) -> Vec<Effect> {
    let processes: Vec<_> = activity.sample.processes.iter().map(|process| process.key).collect();
    activity.next_id += 1;
    let id = activity.next_id;
    activity.entropy = Some(EntropyScan {
        id,
        scope,
        report: EntropyReport::default(),
        total: processes.len(),
        running: true,
        admin: false,
        targets: Vec::new(),
        cursor: 0,
    });
    activity.tops[5] = 0;
    show(activity);
    vec![Effect::EntropyScan { id, scope, processes, admin: false }]
}

/// Goes on with `processes` in another run of the same scan.
fn resume(activity: &mut Activity, processes: Vec<ProcessKey>, admin: bool) -> Vec<Effect> {
    activity.next_id += 1;
    let id = activity.next_id;
    let Some(scan) = activity.entropy.as_mut() else { return Vec::new() };
    scan.id = id;
    scan.running = true;
    scan.admin = admin;
    scan.targets = if admin { processes.clone() } else { Vec::new() };
    activity.error = None;
    vec![Effect::EntropyScan { id, scope: scan.scope, processes, admin }]
}

/// Shows the hits in place of the list, with the selected one's details beside them.
fn show(activity: &mut Activity) {
    forget(activity);
    activity.list = ActivityPage::Entropy;
    activity.page = ActivityPage::EntropyDetails;
    activity.preview_focused = false;
    activity.error = None;
    activity.retry = None;
    activity.landing = None;
}

/// Gives up the request under way, unless it is only a sample.
fn forget(activity: &mut Activity) {
    activity.preview_due = None;
    if activity.pending.as_ref().is_some_and(|(_, request)| !matches!(request, ActivityRequest::Sample)) {
        activity.pending = None;
        activity.next = None;
    }
}

/// Takes in part of a scan. The cursor stays on its hit as new ones rank in around it.
pub(crate) fn receive(activity: &mut Activity, id: u64, found: Result<EntropyReport, String>, done: bool) {
    let Some(scan) = activity.entropy.as_mut().filter(|scan| scan.id == id) else { return };
    let selected = scan.selected().map(|hit| (hit.process, hit.region.start));
    match found {
        Ok(report) => scan.report.merge(report),
        Err(error) => {
            scan.report.unreadable.append(&mut scan.targets);
            activity.error = Some(error);
        }
    }
    if let Some(at) = selected
        .and_then(|selected| scan.report.hits.iter().position(|hit| (hit.process, hit.region.start) == selected))
    {
        scan.cursor = at;
    }
    scan.cursor = scan.cursor.min(scan.report.hits.len().saturating_sub(1));
    if done {
        scan.running = false;
        scan.targets.clear();
    }
}

/// The keys of the hits: Ctrl-C stops the scan, N goes on with the processes it did not reach,
/// A reads the ones Borg may not as administrator, and R scans again.
pub(super) fn list_key(state: &mut AppState, key: &KeyEvent) -> Option<Vec<Effect>> {
    let activity = &mut state.activity;
    let scan = activity.entropy.as_mut()?;
    let plain = key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT;
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) && scan.running => {
            Some(vec![Effect::StopEntropyScan])
        }
        KeyCode::Char('n' | 'N') if plain && scan.continues() => {
            let processes = std::mem::take(&mut scan.report.remaining);
            let admin = scan.admin;
            Some(resume(activity, processes, admin))
        }
        KeyCode::Char('a' | 'A') if plain && scan.elevates(activity.sample.administrator) => {
            let processes = std::mem::take(&mut scan.report.unreadable);
            Some(resume(activity, processes, true))
        }
        KeyCode::Char('r' | 'R') if plain && !scan.running => {
            let scope = scan.scope;
            Some(start(activity, scope))
        }
        _ => None,
    }
}

/// Enter on a hit opens its process's memory map on the hit's region, with the page map's
/// cursor on the region's first dense page. Esc goes back to the hits.
pub(super) fn open_hit(activity: &mut Activity) -> Vec<Effect> {
    let Some(hit) = activity.entropy.as_ref().and_then(EntropyScan::selected).cloned() else { return Vec::new() };
    activity.filter.clear();
    activity.filtering = false;
    activity.rebuild();
    let Some(cursor) = activity.rows.iter().position(|&(i, _)| activity.sample.processes[i].key == hit.process) else {
        activity.error = Some(tr!(entropy_process_gone, name = hit.name, pid = hit.process.pid));
        return Vec::new();
    };
    activity.cursor = cursor;
    activity.select();
    activity.list = ActivityPage::Regions;
    activity.page = ActivityPage::Snapshot;
    activity.preview_focused = false;
    activity.results_origin = ActivityPage::Regions;
    activity.snapshot = None;
    activity.previous = None;
    activity.offset = 0;
    activity.from_search = false;
    activity.from_entropy = true;
    activity.map = true;
    activity.landing = Some(Landing { region: hit.region.start, address: hit.tally.first_high });
    activity.request(ActivityRequest::Regions { process: hit.process, admin: false })
}

/// Puts the cursor on the region a hit landed on, opening its group and file.
pub(super) fn land_on_region(activity: &mut Activity, now: Instant) {
    let Some(landing) = activity.landing else { return };
    let Some(index) = activity.regions.iter().position(|region| region.start == landing.region) else {
        activity.landing = None;
        return;
    };
    let group = activity.region_group(&activity.regions[index]);
    activity.expanded_groups.insert(group);
    if group.by_file() {
        let file = activity.file_key(index).to_owned();
        activity.expanded_files.insert(file);
    }
    activity.refresh_tree();
    activity.region_cursor = activity.find_row(&RowKey::Region(landing.region)).unwrap_or(0);
    activity.schedule_preview(now);
    activity.preview_focused = true;
    if landing.address.is_none() {
        activity.landing = None;
    }
}

/// Puts the page map's cursor on the first dense page the scan sampled.
pub(super) fn land_on_page(activity: &mut Activity) {
    let (Some(census), Some(Landing { region, address: Some(address) })) = (&activity.census, activity.landing) else {
        return;
    };
    if activity.selected_region().is_none_or(|selected| selected.start != region) {
        return;
    }
    activity.landing = None;
    let span = census.page.saturating_mul(census.stride).max(1);
    if let Some(index) = address.checked_sub(census.start).map(|offset| (offset / span) as usize) {
        activity.census_cursor = index.min(census.pages.len().saturating_sub(1));
    }
}
