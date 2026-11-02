//! The network screen: the machine's listening ports, the processes that use the network, every
//! connection and the interfaces, a page each in one panel, and the selected row's details and
//! traffic in the other.

use borg_i18n::tr;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    net::IpAddr,
    time::{Duration, Instant},
};

use crate::{AppState, Effect, Side, View};

mod filter;
mod keys;
mod model;
mod rows;
mod traffic;

use filter::Haystack;
pub use filter::NetworkFilter;
pub(crate) use keys::{click, paste, route_key, show_page, sort_by};
pub use model::{Interface, InterfaceKind, NetworkSample, PortKey, Socket, SocketKey, TcpState, Transport, service};
use rows::Source;
pub use rows::{ConnectionRow, Exposure, InterfaceRow, PortRow, ProcessRow};
pub use traffic::{Meter, Rate, SMOOTHING, Traffic};

const REFRESH: Duration = Duration::from_secs(1);
/// After an error, ask again less often.
const RETRY: Duration = Duration::from_secs(5);
const FILTER_LIMIT: usize = 128;
const PAGE: usize = 10;
/// Rows the list pane's frame, header and footer take.
pub const CHROME_ROWS: u16 = 6;
/// Addresses asked for their names at once.
pub const RESOLVE_BATCH: usize = 32;
/// Names kept for addresses; past this many they are forgotten and asked for again.
const HOSTS_KEPT: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPage {
    Ports,
    Processes,
    Connections,
    Interfaces,
}

impl NetworkPage {
    pub const ALL: [Self; 4] = [Self::Ports, Self::Processes, Self::Connections, Self::Interfaces];

    pub fn index(self) -> usize {
        self as usize
    }

    /// The digit that opens the page.
    pub fn key(self) -> char {
        char::from(b'1' + self as u8)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkSort {
    Port,
    Name,
    Connections,
    Traffic,
}

impl NetworkSort {
    fn from_key(key: char) -> Option<Self> {
        match key.to_ascii_lowercase() {
            'p' => Some(Self::Port),
            'n' => Some(Self::Name),
            'c' => Some(Self::Connections),
            't' => Some(Self::Traffic),
            _ => None,
        }
    }

    /// Whether `page` has a column this sorts by.
    pub fn fits(self, page: NetworkPage) -> bool {
        match self {
            Self::Port => matches!(page, NetworkPage::Ports | NetworkPage::Connections),
            Self::Connections => matches!(page, NetworkPage::Ports | NetworkPage::Processes),
            Self::Name | Self::Traffic => true,
        }
    }

    /// Counts and rates sort the largest first, names and ports from the start.
    fn descends(self) -> bool {
        matches!(self, Self::Connections | Self::Traffic)
    }
}

/// A page's sort column and its direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Order {
    pub sort: NetworkSort,
    pub descending: bool,
}

impl Order {
    const fn by(sort: NetworkSort, descending: bool) -> Self {
        Self { sort, descending }
    }

    fn apply(self, primary: Ordering) -> Ordering {
        if self.descending { primary.reverse() } else { primary }
    }
}

const ORDERS: [Order; 4] = [
    Order::by(NetworkSort::Port, false),
    Order::by(NetworkSort::Traffic, true),
    Order::by(NetworkSort::Traffic, true),
    Order::by(NetworkSort::Traffic, true),
];

/// What the cursor is on, kept across samples while rows come and go.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RowKey {
    Port(Transport, u16, Option<u32>),
    Process(Option<u32>),
    Connection(SocketKey),
    Interface(String),
}

#[derive(Debug, Clone)]
pub struct Network {
    pub side: Option<Side>,
    pub expanded: bool,
    pub details_focused: bool,
    pub page: NetworkPage,
    pub sample: NetworkSample,
    pub loaded: bool,
    pub traffic: Traffic,
    pub ports: Vec<PortRow>,
    pub processes: Vec<ProcessRow>,
    pub connections: Vec<ConnectionRow>,
    pub interfaces: Vec<InterfaceRow>,
    /// How many rows each page has before the filter.
    pub totals: [usize; 4],
    cursors: [usize; 4],
    tops: [usize; 4],
    chosen: [Option<RowKey>; 4],
    pub orders: [Order; 4],
    pub filter: String,
    pub filtering: bool,
    pub paused: bool,
    /// How far the details' list of connections is scrolled.
    pub details_top: usize,
    /// The names addresses resolve to; `None` for an address without one.
    pub hosts: HashMap<IpAddr, Option<String>>,
    resolving: HashSet<IpAddr>,
    pub error: Option<String>,
    pub pending: Option<u64>,
    next_id: u64,
    next: Option<Instant>,
    sampled: Option<Instant>,
}

impl Default for Network {
    fn default() -> Self {
        Self {
            side: None,
            expanded: false,
            details_focused: false,
            page: NetworkPage::Ports,
            sample: NetworkSample::default(),
            loaded: false,
            traffic: Traffic::default(),
            ports: Vec::new(),
            processes: Vec::new(),
            connections: Vec::new(),
            interfaces: Vec::new(),
            totals: [0; 4],
            cursors: [0; 4],
            tops: [0; 4],
            chosen: Default::default(),
            orders: ORDERS,
            filter: String::new(),
            filtering: false,
            paused: false,
            details_top: 0,
            hosts: HashMap::new(),
            resolving: HashSet::new(),
            error: None,
            pending: None,
            next_id: 0,
            next: None,
            sampled: None,
        }
    }
}

/// Who can reach a listening port, in words.
pub fn exposure_label(exposure: Exposure) -> &'static str {
    match exposure {
        Exposure::Everywhere => tr!(network_everywhere),
        Exposure::Addresses => tr!(network_some_addresses),
        Exposure::ThisMachine => tr!(network_this_machine),
    }
}

pub fn kind_label(kind: InterfaceKind) -> &'static str {
    match kind {
        InterfaceKind::Loopback => tr!(network_kind_loopback),
        InterfaceKind::WiFi => tr!(network_kind_wifi),
        InterfaceKind::Ethernet => tr!(network_kind_ethernet),
        InterfaceKind::Tunnel => tr!(network_kind_tunnel),
        InterfaceKind::Bridge => tr!(network_kind_bridge),
        InterfaceKind::Virtual => tr!(network_kind_virtual),
        InterfaceKind::Other => tr!(network_kind_other),
    }
}

impl Network {
    /// What the screen lists for two samples `seconds` apart, sorted as the screen first
    /// sorts each page and kept by `filter`, with the host names already known.
    pub fn measured(
        before: &NetworkSample,
        after: NetworkSample,
        seconds: f64,
        hosts: HashMap<IpAddr, Option<String>>,
        filter: &str,
    ) -> Self {
        let mut network = Self { hosts, filter: filter.to_owned(), loaded: true, ..Self::default() };
        network.traffic.take(before, 0.0);
        network.traffic.take(&after, seconds);
        network.sample = after;
        network.rebuild();
        network
    }

    pub fn is_open(&self) -> bool {
        self.side.is_some()
    }

    pub fn order(&self) -> Order {
        self.orders[self.page.index()]
    }

    pub fn len(&self, page: NetworkPage) -> usize {
        match page {
            NetworkPage::Ports => self.ports.len(),
            NetworkPage::Processes => self.processes.len(),
            NetworkPage::Connections => self.connections.len(),
            NetworkPage::Interfaces => self.interfaces.len(),
        }
    }

    pub fn cursor(&self) -> usize {
        self.cursors[self.page.index()]
    }

    pub fn list_start(&self, height: usize) -> usize {
        let cursor = self.cursor();
        self.tops[self.page.index()].min(cursor).max(cursor.saturating_sub(height.max(1) - 1))
    }

    pub(crate) fn keep_visible(&mut self, height: usize) {
        self.tops[self.page.index()] = self.list_start(height);
    }

    /// The name of the process with `pid`, or what the screen calls a process it cannot see.
    pub fn name(&self, pid: Option<u32>) -> String {
        pid.and_then(|pid| self.sample.names.get(&pid)).cloned().unwrap_or_else(|| tr!(network_unknown_process).into())
    }

    /// The name `address` resolves to, once it has.
    pub fn host(&self, address: IpAddr) -> Option<&str> {
        self.hosts.get(&address).and_then(Option::as_deref)
    }

    pub fn port(&self) -> Option<&PortRow> {
        self.ports.get(self.cursors[NetworkPage::Ports.index()]).filter(|_| self.page == NetworkPage::Ports)
    }

    pub fn process(&self) -> Option<&ProcessRow> {
        self.processes.get(self.cursors[NetworkPage::Processes.index()]).filter(|_| self.page == NetworkPage::Processes)
    }

    pub fn connection(&self) -> Option<(&Socket, Rate)> {
        let row = self.connections.get(self.cursors[NetworkPage::Connections.index()])?;
        (self.page == NetworkPage::Connections).then(|| (&self.sample.sockets[row.socket], row.rate))
    }

    pub fn interface(&self) -> Option<(&Interface, Rate)> {
        let row = self.interfaces.get(self.cursors[NetworkPage::Interfaces.index()])?;
        (self.page == NetworkPage::Interfaces).then(|| (&self.sample.interfaces[row.interface], row.rate))
    }

    /// The process the selected row belongs to, when Borg can see it.
    pub fn selected_pid(&self) -> Option<u32> {
        match self.page {
            NetworkPage::Ports => self.port()?.pid,
            NetworkPage::Processes => self.process()?.pid,
            NetworkPage::Connections => self.connection()?.0.pid,
            NetworkPage::Interfaces => None,
        }
    }

    /// The connections of the selected port or process, busiest first.
    pub fn related(&self) -> Vec<(&Socket, Rate)> {
        let port = self.port();
        let pid = self.process().map(|row| row.pid);
        let mut sockets: Vec<_> = self
            .sample
            .sockets
            .iter()
            .filter(|socket| socket.connects())
            .filter(|socket| match (port, pid) {
                (Some(port), _) => port.covers(socket),
                (None, Some(pid)) => socket.pid == pid,
                (None, None) => false,
            })
            .map(|socket| (socket, self.traffic.socket(&socket.key())))
            .collect();
        sockets.sort_by(|(a, x), (b, y)| y.total().cmp(&x.total()).then(a.remote.cmp(&b.remote)));
        sockets
    }

    /// The ports the selected process listens on, whatever the filter hides.
    pub fn listening(&self) -> Vec<PortRow> {
        let Some(pid) = self.process().map(|row| row.pid) else { return Vec::new() };
        let mut ports = self.source(&NetworkFilter::default()).ports();
        ports.retain(|row| row.pid == pid);
        ports
    }

    /// The machine's traffic: every interface but loopback.
    /// The machine's traffic: every interface but loopback, whatever the filter keeps.
    pub fn total(&self) -> Rate {
        self.sample
            .interfaces
            .iter()
            .filter(|interface| interface.kind != InterfaceKind::Loopback)
            .fold(Rate::default(), |sum, interface| sum.plus(self.traffic.interface(&interface.name)))
    }

    fn source<'a>(&'a self, filter: &'a NetworkFilter) -> Source<'a> {
        Source { sample: &self.sample, traffic: &self.traffic, hosts: &self.hosts, filter }
    }

    fn key_at(&self, page: NetworkPage, index: usize) -> Option<RowKey> {
        Some(match page {
            NetworkPage::Ports => {
                let row = self.ports.get(index)?;
                RowKey::Port(row.transport, row.port, row.pid)
            }
            NetworkPage::Processes => RowKey::Process(self.processes.get(index)?.pid),
            NetworkPage::Connections => {
                RowKey::Connection(self.sample.sockets[self.connections.get(index)?.socket].key())
            }
            NetworkPage::Interfaces => {
                RowKey::Interface(self.sample.interfaces[self.interfaces.get(index)?.interface].name.clone())
            }
        })
    }

    fn find(&self, page: NetworkPage, key: &RowKey) -> Option<usize> {
        (0..self.len(page)).find(|&index| self.key_at(page, index).as_ref() == Some(key))
    }

    pub(crate) fn select(&mut self, index: usize) {
        let page = self.page;
        let index = index.min(self.len(page).saturating_sub(1));
        if self.cursors[page.index()] != index {
            self.details_top = 0;
        }
        self.cursors[page.index()] = index;
        self.chosen[page.index()] = self.key_at(page, index);
    }

    pub(crate) fn move_by(&mut self, delta: isize) {
        let last = self.len(self.page).saturating_sub(1) as isize;
        self.select((self.cursor() as isize + delta).clamp(0, last.max(0)) as usize);
    }

    /// Builds every page's rows from the sample, filtered and sorted, keeping each cursor on the
    /// row it was on.
    pub(crate) fn rebuild(&mut self) {
        let filter = NetworkFilter::parse(&self.filter);
        let (rows, totals) = self.source(&filter).filtered();
        let rows::Rows { mut ports, mut processes, mut connections, mut interfaces } = rows;
        self.totals = totals;
        let [by_port, by_process, by_connection, by_interface] = self.orders;
        let names = &self.sample.names;
        let name = |pid: Option<u32>| pid.and_then(|pid| names.get(&pid)).map(|name| name.to_lowercase());
        ports.sort_by(|a, b| {
            by_port
                .apply(match by_port.sort {
                    NetworkSort::Name => name(a.pid).cmp(&name(b.pid)),
                    NetworkSort::Connections => a.connections.cmp(&b.connections),
                    NetworkSort::Traffic => a.rate.total().cmp(&b.rate.total()),
                    NetworkSort::Port => Ordering::Equal,
                })
                .then((a.port, a.transport, a.pid).cmp(&(b.port, b.transport, b.pid)))
        });
        processes.sort_by(|a, b| {
            by_process
                .apply(match by_process.sort {
                    NetworkSort::Name => name(a.pid).cmp(&name(b.pid)),
                    NetworkSort::Connections => a.connections.cmp(&b.connections),
                    NetworkSort::Traffic | NetworkSort::Port => {
                        a.rate.total().cmp(&b.rate.total()).then(a.connections.cmp(&b.connections))
                    }
                })
                .then((name(a.pid), a.pid).cmp(&(name(b.pid), b.pid)))
        });
        let sockets = &self.sample.sockets;
        connections.sort_by(|a, b| {
            let (x, y) = (&sockets[a.socket], &sockets[b.socket]);
            by_connection
                .apply(match by_connection.sort {
                    NetworkSort::Name => name(x.pid).cmp(&name(y.pid)),
                    NetworkSort::Port => x.remote.map(|r| r.port()).cmp(&y.remote.map(|r| r.port())),
                    NetworkSort::Traffic | NetworkSort::Connections => a.rate.total().cmp(&b.rate.total()),
                })
                .then((name(x.pid), x.remote, x.local).cmp(&(name(y.pid), y.remote, y.local)))
        });
        let all = &self.sample.interfaces;
        interfaces.sort_by(|a, b| {
            let (x, y) = (&all[a.interface], &all[b.interface]);
            by_interface
                .apply(match by_interface.sort {
                    NetworkSort::Name => x.name.cmp(&y.name),
                    _ => (a.rate.total(), x.up).cmp(&(b.rate.total(), y.up)),
                })
                .then(x.name.cmp(&y.name))
        });
        (self.ports, self.processes, self.connections, self.interfaces) = (ports, processes, connections, interfaces);
        for page in NetworkPage::ALL {
            let index = page.index();
            let found = self.chosen[index].clone().and_then(|key| self.find(page, &key));
            self.cursors[index] = found.unwrap_or(self.cursors[index].min(self.len(page).saturating_sub(1)));
            if found.is_none() && self.chosen[index].is_some() {
                self.chosen[index] = self.key_at(page, self.cursors[index]);
            }
        }
    }

    fn request(&mut self) -> Vec<Effect> {
        self.next_id += 1;
        self.pending = Some(self.next_id);
        self.next = None;
        vec![Effect::Network { id: self.next_id }]
    }

    /// Asks for the names of the remote addresses not yet asked about.
    fn resolve(&mut self) -> Option<Effect> {
        if self.hosts.len() > HOSTS_KEPT {
            self.hosts.clear();
        }
        let wanted: Vec<IpAddr> = self
            .sample
            .sockets
            .iter()
            .filter_map(|socket| socket.remote.map(|remote| remote.ip()))
            .filter(|address| !address.is_loopback() && !address.is_unspecified())
            .collect::<HashSet<_>>()
            .into_iter()
            .filter(|address| !self.hosts.contains_key(address) && !self.resolving.contains(address))
            .take(RESOLVE_BATCH)
            .collect();
        if wanted.is_empty() {
            return None;
        }
        self.resolving.extend(wanted.iter().copied());
        Some(Effect::ResolveHosts(wanted))
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
    let network = &mut state.network;
    network.side = Some(state.active);
    network.details_focused = false;
    network.paused = false;
    network.error = None;
    network.loaded = false;
    network.traffic = Traffic::default();
    network.sampled = None;
    network.request()
}

pub(crate) fn close(state: &mut AppState) {
    state.network.side = None;
    state.network.filtering = false;
}

/// Alt-N opens the screen, and closes it while it is open.
pub(crate) fn toggle(state: &mut AppState) -> Vec<Effect> {
    if state.network.is_open() {
        close(state);
        return Vec::new();
    }
    open(state)
}

pub(crate) fn wake_at(state: &AppState) -> Option<Instant> {
    let network = &state.network;
    if !network.is_open() || state.view != View::Panels || state.overlay.is_some() || network.paused {
        return None;
    }
    network.next
}

/// Samples once a second while the screen shows, and keeps the cursor in sight.
pub(crate) fn sync(state: &mut AppState, now: Instant) -> Vec<Effect> {
    if !state.network.is_open() || state.view != View::Panels {
        state.network.pending = None;
        state.network.next = None;
        return Vec::new();
    }
    if state.overlay.is_some() {
        return Vec::new();
    }
    let side = state.network.side.unwrap_or(state.active);
    let list = state.layout(now).panels[side.index()];
    let network = &mut state.network;
    if !list.is_empty() {
        network.keep_visible(usize::from(list.height.saturating_sub(CHROME_ROWS)));
    }
    if !network.paused && network.pending.is_none() && network.next.is_none_or(|next| now >= next) {
        return network.request();
    }
    Vec::new()
}

pub(crate) fn receive(
    state: &mut AppState,
    id: u64,
    result: Result<NetworkSample, String>,
    now: Instant,
) -> Vec<Effect> {
    let network = &mut state.network;
    if network.pending != Some(id) || !network.is_open() {
        return Vec::new();
    }
    network.pending = None;
    match result {
        Ok(sample) => {
            let seconds = network.sampled.map_or(0.0, |before| now.duration_since(before).as_secs_f64());
            network.traffic.take(&sample, seconds);
            network.sample = sample;
            network.sampled = Some(now);
            network.loaded = true;
            network.error = None;
            network.next = Some(now + REFRESH);
            network.rebuild();
            network.resolve().into_iter().collect()
        }
        Err(error) => {
            network.error = Some(error);
            network.next = Some(now + RETRY);
            Vec::new()
        }
    }
}

/// Names for addresses asked about; an address without one keeps showing as it is.
pub(crate) fn hosts_resolved(state: &mut AppState, names: Vec<(IpAddr, Option<String>)>) {
    let network = &mut state.network;
    for (address, name) in names {
        network.resolving.remove(&address);
        network.hosts.insert(address, name);
    }
    if network.is_open() && !network.filter.is_empty() {
        network.rebuild();
    }
}

#[cfg(test)]
mod tests;
