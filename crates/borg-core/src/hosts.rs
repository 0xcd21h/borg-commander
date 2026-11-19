//! The hosts screen: a card for each paired host with its load, memory, fullest disk, alerts and
//! containers, refreshed while the screen is open. The app asks each host over borg cloud; this
//! keeps what came back, orders the cards and moves the cursor among them.

use std::time::{Duration, Instant, SystemTime};

use crate::screens::{FILTER_LIMIT, PAGE};
use crate::{AppState, Effect, RemoteChoice, RemoteHostInfo, RemoteNames, Side, View};

mod figures;
mod keys;
mod report;

pub use figures::{
    ContainerFigures, DiskFigures, Figure, HostFigures, MemoryFigures, WHOLE, WatchFigures, fullest, holds_files,
    older_than, share, worse,
};
pub(crate) use keys::{click, paste, route_key};
pub use report::{
    ConcernLevel, ConcernReport, DiskReport, FiguresReport, HostReport, LoadReport, MemoryReport, ReachReport,
    SessionReport, WatcherReport, host_reports,
};

const EXCLUDE: char = '!';
/// How often the app asks each host while the screen is open.
pub const HOSTS_EVERY: Duration = Duration::from_secs(5);
/// Figures older than this show their age.
pub const STALE_AFTER: Duration = Duration::from_secs(15);
/// Cells a card takes at least, its border included, and rows it takes.
pub const CARD_WIDTH: u16 = 38;
pub const CARD_HEIGHT: u16 = 7;
/// Cells between two cards side by side.
pub const CARD_GAP: u16 = 1;
/// Rows the frame, the summary, the filter and the footer take around the cards.
pub const CARDS_CHROME: u16 = 5;
/// The rows view adds a row of column titles.
pub const ROWS_CHROME: u16 = 6;
/// Cells the pane's frame takes across.
const FRAME_CELLS: u16 = 2;

/// How the screen shows the hosts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HostsLayout {
    #[default]
    Cards,
    Rows,
}

/// Whether a host answered the last time Borg asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// Borg has not heard back yet since the screen opened.
    Waiting,
    Answering,
    /// The host is connected to the relay but did not answer in time, since then.
    Silent {
        since: Instant,
    },
    /// The relay says the host is not connected to it.
    Offline,
    /// Reaching the host failed, and why.
    Failed(String),
}

/// What the app heard back from a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostHeard {
    Answered(Box<HostReply>),
    Silent,
    Offline,
    Failed(String),
}

/// A host's answer: its version, platform and sessions as the remote hosts list shows them, and
/// its figures when its Borg sends them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostReply {
    pub info: RemoteHostInfo,
    pub sessions: Vec<RemoteChoice>,
    pub figures: Option<HostFigures>,
    pub seen: SystemTime,
}

/// A paired host, as the app lists them before it asks each one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedHost {
    pub identity: String,
    pub machine: String,
    pub relay: String,
    /// When the host last answered, as this computer remembers it.
    pub last_seen: Option<SystemTime>,
}

/// One host's card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCard {
    pub identity: String,
    pub machine: String,
    /// The name the user gave the host on this computer.
    pub label: Option<String>,
    pub reach: Reach,
    pub info: RemoteHostInfo,
    pub sessions: Vec<RemoteChoice>,
    pub figures: Option<HostFigures>,
    /// When the figures arrived.
    pub received: Option<Instant>,
    pub last_seen: Option<SystemTime>,
}

impl HostCard {
    fn new(host: PairedHost) -> Self {
        Self {
            identity: host.identity,
            machine: host.machine,
            label: None,
            reach: Reach::Waiting,
            info: RemoteHostInfo { relay: host.relay, ..Default::default() },
            sessions: Vec::new(),
            figures: None,
            received: None,
            last_seen: host.last_seen,
        }
    }

    pub fn name(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.machine)
    }

    pub fn offline(&self) -> bool {
        self.reach == Reach::Offline
    }

    /// How old the figures are, once they are older than a refresh or two.
    pub fn stale(&self, now: Instant) -> Option<Duration> {
        let age = now.saturating_duration_since(self.received?);
        (age >= STALE_AFTER).then_some(age)
    }

    /// Whether the host runs a Borg older than this one.
    pub fn older(&self) -> bool {
        self.info.version.as_deref().is_some_and(|version| older_than(version, env!("CARGO_PKG_VERSION")))
    }

    fn haystack(&self) -> String {
        let words = [
            self.name(),
            &self.machine,
            self.info.version.as_deref().unwrap_or_default(),
            self.info.platform.as_deref().unwrap_or_default(),
        ];
        words.join("\n").to_lowercase()
    }

    fn order(&self) -> (bool, String, String) {
        (self.offline(), self.name().to_lowercase(), self.identity.clone())
    }
}

/// The cards `filter` keeps: each word must appear, and none of those after `!`.
pub fn hosts_filtered(cards: &[HostCard], filter: &str) -> Vec<usize> {
    let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
    cards
        .iter()
        .enumerate()
        .filter(|(_, card)| {
            let haystack = card.haystack();
            words.iter().all(|word| match word.strip_prefix(EXCLUDE) {
                Some(excluded) => excluded.is_empty() || !haystack.contains(excluded),
                None => haystack.contains(word.as_str()),
            })
        })
        .map(|(index, _)| index)
        .collect()
}

/// Cards side by side in a pane `width` cells wide, inside its frame.
pub fn card_columns(width: u16) -> usize {
    let inner = width.saturating_sub(FRAME_CELLS);
    usize::from((inner + CARD_GAP) / (CARD_WIDTH + CARD_GAP)).max(1)
}

/// Whether a pane `width` cells wide fits a card.
pub fn fits_cards(width: u16) -> bool {
    width.saturating_sub(FRAME_CELLS) >= CARD_WIDTH
}

/// Whether a pane `height` rows tall fits a row of cards.
pub fn fits_card_rows(height: u16) -> bool {
    height >= CARDS_CHROME + CARD_HEIGHT
}

#[derive(Debug, Clone)]
pub struct Hosts {
    pub side: Option<Side>,
    pub expanded: bool,
    pub layout: HostsLayout,
    pub cards: Vec<HostCard>,
    /// The app listed the paired hosts.
    pub listed: bool,
    /// The cards the filter keeps, in order: those Borg reaches first, then those offline, each
    /// by name.
    pub rows: Vec<usize>,
    cursor: usize,
    /// The first row of cards, or of rows, in sight.
    top: usize,
    chosen: Option<String>,
    pub filter: String,
    pub filtering: bool,
    pub error: Option<String>,
    /// The app asks the hosts.
    pub watching: bool,
    /// Cards side by side, as the pane last fit them, or none in the rows view.
    columns: Option<usize>,
}

impl Default for Hosts {
    fn default() -> Self {
        Self {
            side: None,
            expanded: true,
            layout: HostsLayout::Cards,
            cards: Vec::new(),
            listed: false,
            rows: Vec::new(),
            cursor: 0,
            top: 0,
            chosen: None,
            filter: String::new(),
            filtering: false,
            error: None,
            watching: false,
            columns: None,
        }
    }
}

impl Hosts {
    pub fn is_open(&self) -> bool {
        self.side.is_some()
    }

    pub fn typing(&self) -> bool {
        self.filtering
    }

    pub fn card(&self, row: usize) -> Option<&HostCard> {
        self.rows.get(row).and_then(|index| self.cards.get(*index))
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Cards side by side, as the pane last fit them, or none in the rows view.
    pub fn columns(&self) -> Option<usize> {
        self.columns
    }

    pub fn selected(&self) -> Option<&HostCard> {
        self.card(self.cursor).filter(|_| self.cursor < self.reachable())
    }

    /// Rows the cursor can reach: every row in the rows view, and the cards of the hosts that are
    /// not offline in the cards view, which lists the others on one line.
    pub fn reachable(&self) -> usize {
        match self.columns {
            Some(_) => self.rows.iter().take_while(|&&index| !self.cards[index].offline()).count(),
            None => self.rows.len(),
        }
    }

    /// The hosts the cards view lists on one line below the cards.
    pub fn offline(&self) -> impl Iterator<Item = &HostCard> {
        self.rows.iter().map(|&index| &self.cards[index]).filter(|card| card.offline())
    }

    pub fn online(&self) -> usize {
        self.cards.iter().filter(|card| card.reach == Reach::Answering).count()
    }

    /// The first row, of cards or of hosts, in sight of `height` rows of them.
    pub fn list_start(&self, height: usize) -> usize {
        let line = self.line_of(self.cursor);
        self.top.min(line).max(line.saturating_sub(height.max(1) - 1))
    }

    fn line_of(&self, row: usize) -> usize {
        row / self.columns.unwrap_or(1)
    }

    fn keep_visible(&mut self, height: usize) {
        self.top = self.list_start(height);
    }

    pub(crate) fn select(&mut self, row: usize) {
        let row = row.min(self.reachable().saturating_sub(1));
        self.cursor = row;
        self.chosen = self.card(row).map(|card| card.identity.clone());
    }

    fn move_by(&mut self, delta: isize) {
        let last = self.reachable().saturating_sub(1) as isize;
        self.select((self.cursor as isize + delta).clamp(0, last.max(0)) as usize);
    }

    /// Up and Down move a row of cards, and Left and Right one card.
    fn step(&self) -> isize {
        self.columns.unwrap_or(1) as isize
    }

    /// Orders and filters the cards again, keeping the cursor on the host it was on.
    fn rebuild(&mut self) {
        let mut rows = hosts_filtered(&self.cards, &self.filter);
        rows.sort_by_key(|&index| self.cards[index].order());
        self.rows = rows;
        let chosen = self.chosen.clone();
        let found = chosen.and_then(|identity| {
            (0..self.rows.len()).find(|&row| self.card(row).is_some_and(|card| card.identity == identity))
        });
        self.cursor = found.unwrap_or(self.cursor).min(self.reachable().saturating_sub(1));
        self.chosen = self.card(self.cursor).map(|card| card.identity.clone());
    }

    /// The paired hosts as the app listed them: new ones wait for their first answer, and those
    /// no longer paired go.
    pub fn list(&mut self, hosts: Vec<PairedHost>) {
        let mut cards = Vec::with_capacity(hosts.len());
        for host in hosts {
            let card = match self.cards.iter().position(|card| card.identity == host.identity) {
                Some(index) => {
                    let mut card = self.cards.swap_remove(index);
                    card.machine = host.machine;
                    card.info.relay = host.relay;
                    card.last_seen = card.last_seen.max(host.last_seen);
                    card
                }
                None => HostCard::new(host),
            };
            cards.push(card);
        }
        self.cards = cards;
        self.listed = true;
        self.rebuild();
    }

    /// What a host answered, or that it did not.
    pub fn answer(&mut self, identity: &str, answer: HostHeard, now: Instant) {
        let Some(card) = self.cards.iter_mut().find(|card| card.identity == identity) else { return };
        card.reach = match answer {
            HostHeard::Answered(reply) => {
                let HostReply { info, sessions, figures, seen } = *reply;
                card.info = RemoteHostInfo { relay: card.info.relay.clone(), ..info };
                card.sessions = sessions;
                card.received = figures.is_some().then_some(now);
                card.figures = figures;
                card.last_seen = Some(seen);
                Reach::Answering
            }
            HostHeard::Silent => match card.reach {
                Reach::Silent { since } => Reach::Silent { since },
                _ => Reach::Silent { since: now },
            },
            HostHeard::Offline => Reach::Offline,
            HostHeard::Failed(error) => Reach::Failed(error),
        };
        self.rebuild();
    }

    /// Takes the names the user gave hosts on this computer.
    pub fn take_names(&mut self, names: &RemoteNames) {
        let mut renamed = false;
        for card in &mut self.cards {
            let label = names.0.get(&card.identity).and_then(|names| names.host.clone());
            if card.label != label {
                card.label = label;
                renamed = true;
            }
        }
        if renamed {
            self.rebuild();
        }
    }
}

pub(crate) fn open(state: &mut AppState) -> Vec<Effect> {
    state.overlay = None;
    crate::screens::close_others(state, crate::screens::Screen::Hosts);
    let hosts = &mut state.hosts;
    hosts.side = Some(state.active);
    hosts.error = None;
    Vec::new()
}

pub(crate) fn close(state: &mut AppState) {
    state.hosts.side = None;
    state.hosts.filtering = false;
}

/// The menus' entry opens the screen, and closes it while it is open.
pub(crate) fn toggle(state: &mut AppState) -> Vec<Effect> {
    if state.hosts.is_open() {
        close(state);
        return Vec::new();
    }
    open(state)
}

/// Starts and stops asking the hosts with the screen, keeps the cursor in sight, and takes the
/// names the user gave the hosts.
pub(crate) fn sync(state: &mut AppState, now: Instant) -> Vec<Effect> {
    let live = state.hosts.is_open() && state.view == View::Panels;
    let mut effects = Vec::new();
    if live != state.hosts.watching {
        state.hosts.watching = live;
        effects.push(Effect::WatchHosts(live));
    }
    let names = state.remote_names.clone();
    state.hosts.take_names(&names);
    if !live || state.overlay.is_some() {
        return effects;
    }
    let side = state.hosts.side.unwrap_or(state.active);
    let list = state.layout(now).panels[side.index()];
    if list.is_empty() {
        return effects;
    }
    let hosts = &mut state.hosts;
    let cards = hosts.layout == HostsLayout::Cards && fits_cards(list.width) && fits_card_rows(list.height);
    let columns = cards.then(|| card_columns(list.width));
    if columns != hosts.columns {
        hosts.columns = columns;
        hosts.rebuild();
    }
    let height = match columns {
        Some(_) => usize::from(list.height.saturating_sub(CARDS_CHROME) / CARD_HEIGHT),
        None => usize::from(list.height.saturating_sub(ROWS_CHROME)),
    };
    hosts.keep_visible(height);
    effects
}

pub(crate) fn listed(state: &mut AppState, result: Result<Vec<PairedHost>, String>) {
    match result {
        Ok(hosts) => {
            state.hosts.error = None;
            state.hosts.list(hosts);
        }
        Err(error) => state.hosts.error = Some(error),
    }
}

pub(crate) fn answered(state: &mut AppState, identity: &str, answer: HostHeard, now: Instant) {
    state.hosts.answer(identity, answer, now);
}

#[cfg(test)]
mod tests;
