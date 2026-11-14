//! The network screen: the machine's listening ports, the processes that use the network, every
//! connection and the interfaces, a page each in one panel, and the selected row's details and
//! traffic in the other.

use crate::screens::{CHROME_ROWS, FILTER_LIMIT, PAGE};
use borg_i18n::tr;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr},
    time::{Duration, Instant},
};

use crate::{AppState, Capture, Effect, Side, View};

mod capture;
mod filter;
mod keys;
mod model;
mod neighbors;
mod rows;
mod shell_variables;
mod traffic;
mod viewer;

pub use capture::{
    Authority, CAPTURE_SIZE_LIMIT, CaptureAccess, CaptureDialog, CaptureFailure, CaptureField, CaptureLimit,
    CaptureProgress, CaptureRequest, CaptureScope, DURATIONS, HEADER_BYTES, KEEP_CHOICES, Keep, LONGEST_CAPTURE,
    Proxied, ProxyEndpoint, Recording, SHORTEST_CAPTURE, SeenSockets, TlsKeys, capture_ask, capture_extension,
    capture_interfaces, capture_no_room,
};
use filter::Haystack;
pub use filter::NetworkFilter;
pub(crate) use keys::{click, copied_text, paste, route_key, show_page, sort_by};
pub use model::{Interface, InterfaceKind, NetworkSample, PortKey, Socket, SocketKey, TcpState, Transport, service};
pub use neighbors::{
    Gateway, GatewayWatch, KnownHost, KnownNetwork, LocalNetwork, Mac, MacChange, NEIGHBORS_FORMAT, Neighbor,
    NeighborMemory, NeighborRow, NeighborState, Neighborhood, Noted, Presence, Probe, Probed, SCAN_PORTS,
    SavedNeighbors, Subnet, SubnetScan, places, subnet, vendor,
};
use rows::Source;
pub use rows::{ConnectionRow, Exposure, InterfaceRow, PortRow, ProcessRow, neighbor_notes};
pub use shell_variables::{
    Dialect, KEY_LOG_VARIABLE, PROXY_VARIABLES, ShellVariables, TRUST_VARIABLES, shell_variables,
};
pub use traffic::{Meter, Rate, SMOOTHING, Traffic};
pub use viewer::{Ends, HostRow, Talker, TrafficPage, TrafficView};

const REFRESH: Duration = Duration::from_secs(1);
/// After an error, ask again less often.
const RETRY: Duration = Duration::from_secs(5);
/// Addresses asked for their names at once.
pub const RESOLVE_BATCH: usize = 32;
/// Names kept for addresses; past this many they are forgotten and asked for again.
const HOSTS_KEPT: usize = 4096;
const NEIGHBOR_SAMPLES: u8 = 5;
const PAGES: usize = 5;
/// How often the remembered neighbors are saved when only the times they were seen changed.
const MEMORY_SAVE_EVERY: Duration = Duration::from_secs(60);
const PORT_SEPARATOR: &str = ", ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPage {
    Ports,
    Processes,
    Connections,
    Interfaces,
    Neighbors,
}

impl NetworkPage {
    pub const ALL: [Self; PAGES] = [Self::Ports, Self::Processes, Self::Connections, Self::Interfaces, Self::Neighbors];

    pub fn index(self) -> usize {
        self as usize
    }

    /// The digit that opens the page.
    pub fn key(self) -> char {
        char::from(b'1' + self as u8)
    }

    pub fn from_key(key: char) -> Option<Self> {
        Self::ALL.into_iter().find(|page| page.key() == key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkSort {
    Port,
    Name,
    Connections,
    Traffic,
    Address,
    Mac,
}

impl NetworkSort {
    fn from_key(key: char) -> Option<Self> {
        match key.to_ascii_lowercase() {
            'p' => Some(Self::Port),
            'n' => Some(Self::Name),
            'c' => Some(Self::Connections),
            't' => Some(Self::Traffic),
            'a' => Some(Self::Address),
            'm' => Some(Self::Mac),
            _ => None,
        }
    }

    /// Whether `page` has a column this sorts by.
    pub fn fits(self, page: NetworkPage) -> bool {
        match self {
            Self::Port => matches!(page, NetworkPage::Ports | NetworkPage::Connections),
            Self::Connections => matches!(page, NetworkPage::Ports | NetworkPage::Processes),
            Self::Traffic => page != NetworkPage::Neighbors,
            Self::Address | Self::Mac => page == NetworkPage::Neighbors,
            Self::Name => true,
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

const ORDERS: [Order; PAGES] = [
    Order::by(NetworkSort::Port, false),
    Order::by(NetworkSort::Traffic, true),
    Order::by(NetworkSort::Traffic, true),
    Order::by(NetworkSort::Traffic, true),
    Order::by(NetworkSort::Address, false),
];

/// What the cursor is on, kept across samples while rows come and go.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RowKey {
    Port(Transport, u16, Option<u32>),
    Process(Option<u32>),
    Connection(SocketKey),
    Interface(String),
    Neighbor(IpAddr, String),
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
    pub neighbors: Vec<NeighborRow>,
    /// The neighbor tables with what scans found and the remembered hosts that are gone.
    pub neighborhood: Neighborhood,
    table: Neighborhood,
    presence: Vec<Presence>,
    pub places: Vec<LocalNetwork>,
    pub gateway_watch: GatewayWatch,
    /// What scans found this session, by address.
    pub probes: HashMap<IpAddr, Probed>,
    /// Every address a scan asked this session.
    scanned: HashSet<IpAddr>,
    pub scan: Option<ScanRun>,
    pub memory: NeighborMemory,
    /// False when the saved neighbors could not be read, so they are left as they are.
    pub memory_writable: bool,
    memory_changed: bool,
    memory_saved: Option<Instant>,
    neighbors_due: u8,
    /// How many rows each page has before the filter.
    pub totals: [usize; PAGES],
    cursors: [usize; PAGES],
    tops: [usize; PAGES],
    chosen: [Option<RowKey>; PAGES],
    pub orders: [Order; PAGES],
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
    /// The screen was opened from the activity monitor on this process, and Esc goes back to it.
    back: Option<u32>,
    /// A process to select once a sample lists it.
    revealing: Option<u32>,
    /// The capture `tcpdump` is taking.
    pub recording: Option<Recording>,
    /// What capturing as administrator can do here.
    pub access: CaptureAccess,
    /// The variables Borg's shell gets around captures that keep TLS keys.
    pub shell_variables: ShellVariables,
    /// Who signs the certificates of Borg's proxy, from the `[network]` settings.
    pub authority: Authority,
    /// The MiB captures leave free on their folder's disk, from the `[network]` settings.
    pub keep_free_mib: Option<u64>,
    /// A capture's traffic, shown in place of the live pages.
    pub viewer: Option<Box<TrafficView>>,
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
            neighbors: Vec::new(),
            neighborhood: Neighborhood::default(),
            table: Neighborhood::default(),
            presence: Vec::new(),
            places: Vec::new(),
            gateway_watch: GatewayWatch::default(),
            probes: HashMap::new(),
            scanned: HashSet::new(),
            scan: None,
            memory: NeighborMemory::default(),
            memory_writable: true,
            memory_changed: false,
            memory_saved: None,
            neighbors_due: 0,
            totals: [0; PAGES],
            cursors: [0; PAGES],
            tops: [0; PAGES],
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
            back: None,
            revealing: None,
            recording: None,
            access: CaptureAccess::default(),
            shell_variables: ShellVariables::default(),
            authority: Authority::default(),
            keep_free_mib: None,
            viewer: None,
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

    /// Whether Esc on the list goes back to the activity monitor.
    pub fn goes_back(&self) -> bool {
        self.back.is_some()
    }

    /// Whether a filter takes the typing.
    pub fn typing(&self) -> bool {
        self.filtering || self.viewer.as_ref().is_some_and(|view| view.filtering)
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
            NetworkPage::Neighbors => self.neighbors.len(),
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

    pub fn neighbor(&self) -> Option<(&Neighbor, NeighborRow)> {
        let row = *self.neighbors.get(self.cursors[NetworkPage::Neighbors.index()])?;
        (self.page == NetworkPage::Neighbors).then(|| (&self.neighborhood.neighbors[row.neighbor], row))
    }

    pub fn neighbor_name(&self, neighbor: &Neighbor) -> Option<String> {
        self.host(neighbor.address).or_else(|| neighbor.mac.and_then(vendor)).map(str::to_owned)
    }

    pub fn same_mac(&self, neighbor: &Neighbor) -> Vec<IpAddr> {
        let Some(mac) = neighbor.mac else { return Vec::new() };
        self.neighbors
            .iter()
            .map(|row| &self.neighborhood.neighbors[row.neighbor])
            .filter(|other| other.mac == Some(mac) && other.address != neighbor.address)
            .map(|other| other.address)
            .collect()
    }

    pub fn scan_target(&self) -> Option<SubnetScan> {
        let interface = match self.neighbor() {
            Some((neighbor, _)) => neighbor.interface.clone(),
            None => self.neighborhood.first_gateway()?.interface.clone(),
        };
        self.sample.interfaces.iter().find(|known| known.name == interface).and_then(SubnetScan::of)
    }

    /// The scanned ports `address` took connections on, from this session's scan or else an
    /// earlier one.
    pub fn open_ports(&self, address: IpAddr) -> Option<&[u16]> {
        neighbors::open_ports(&self.probes, &self.memory, &self.places, address)
    }

    /// Puts the scanned and remembered hosts beside the neighbor tables and builds the rows again.
    fn merge(&mut self) {
        self.places = neighbors::places(&self.sample.interfaces, &self.table);
        (self.neighborhood, self.presence) = neighbors::merge(&self.table, &self.probes, &self.memory, &self.places);
        self.rebuild();
    }

    /// Whether to save the remembered neighbors now: at once for a new host, port or MAC
    /// address, and at most every minute for times alone.
    fn save_due(&mut self, noted: Noted, now: Instant) -> bool {
        self.memory_changed |= noted.learned || noted.refreshed;
        let due = noted.learned || self.memory_saved.is_none_or(|saved| now.duration_since(saved) >= MEMORY_SAVE_EVERY);
        self.flush(due, now)
    }

    fn flush(&mut self, due: bool, now: Instant) -> bool {
        if !(due && self.memory_changed && self.memory_writable) {
            return false;
        }
        self.memory_changed = false;
        self.memory_saved = Some(now);
        true
    }

    /// The process the selected row belongs to, when Borg can see it.
    pub fn selected_pid(&self) -> Option<u32> {
        match self.page {
            NetworkPage::Ports => self.port()?.pid,
            NetworkPage::Processes => self.process()?.pid,
            NetworkPage::Connections => self.connection()?.0.pid,
            NetworkPage::Interfaces | NetworkPage::Neighbors => None,
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
        Source {
            sample: &self.sample,
            traffic: &self.traffic,
            hosts: &self.hosts,
            neighborhood: &self.neighborhood,
            presence: &self.presence,
            watch: &self.gateway_watch,
            scanned: &self.scanned,
            probes: &self.probes,
            memory: &self.memory,
            places: &self.places,
            filter,
        }
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
            NetworkPage::Neighbors => {
                let neighbor = &self.neighborhood.neighbors[self.neighbors.get(index)?.neighbor];
                RowKey::Neighbor(neighbor.address, neighbor.interface.clone())
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

    /// Shows the Processes page on the process with `pid`, when a row has it.
    fn reveal(&mut self, pid: u32) -> bool {
        let Some(index) = self.processes.iter().position(|row| row.pid == Some(pid)) else { return false };
        keys::show_page(self, NetworkPage::Processes);
        self.select(index);
        true
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
        let rows::Rows { mut ports, mut processes, mut connections, mut interfaces, mut neighbors } = rows;
        self.totals = totals;
        let [by_port, by_process, by_connection, by_interface, by_neighbor] = self.orders;
        let names = &self.sample.names;
        let name = |pid: Option<u32>| pid.and_then(|pid| names.get(&pid)).map(|name| name.to_lowercase());
        ports.sort_by(|a, b| {
            by_port
                .apply(match by_port.sort {
                    NetworkSort::Name => name(a.pid).cmp(&name(b.pid)),
                    NetworkSort::Connections => a.connections.cmp(&b.connections),
                    NetworkSort::Traffic => a.rate.total().cmp(&b.rate.total()),
                    NetworkSort::Port | NetworkSort::Address | NetworkSort::Mac => Ordering::Equal,
                })
                .then((a.port, a.transport, a.pid).cmp(&(b.port, b.transport, b.pid)))
        });
        processes.sort_by(|a, b| {
            by_process
                .apply(match by_process.sort {
                    NetworkSort::Name => name(a.pid).cmp(&name(b.pid)),
                    NetworkSort::Connections => a.connections.cmp(&b.connections),
                    NetworkSort::Traffic | NetworkSort::Port | NetworkSort::Address | NetworkSort::Mac => {
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
                    NetworkSort::Traffic | NetworkSort::Connections | NetworkSort::Address | NetworkSort::Mac => {
                        a.rate.total().cmp(&b.rate.total())
                    }
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
        let known = &self.neighborhood.neighbors;
        let labels: HashMap<usize, Option<String>> = neighbors
            .iter()
            .map(|row| (row.neighbor, self.neighbor_name(&known[row.neighbor]).map(|name| name.to_lowercase())))
            .collect();
        neighbors.sort_by(|a, b| {
            let (x, y) = (&known[a.neighbor], &known[b.neighbor]);
            let (p, q) = (&labels[&a.neighbor], &labels[&b.neighbor]);
            let rank = |row: &NeighborRow, neighbor: &Neighbor| match row.presence {
                Presence::Listed if neighbor.mac.is_some() => 0,
                Presence::Answered => 1,
                Presence::Offline(_) => 2,
                Presence::Listed => 3,
            };
            rank(a, x).cmp(&rank(b, y)).then_with(|| {
                by_neighbor
                    .apply(match by_neighbor.sort {
                        NetworkSort::Mac => x.mac.cmp(&y.mac),
                        NetworkSort::Name => p.is_none().cmp(&q.is_none()).then_with(|| p.cmp(q)),
                        _ => Ordering::Equal,
                    })
                    .then((x.address, &x.interface).cmp(&(y.address, &y.interface)))
            })
        });
        (self.ports, self.processes, self.connections, self.interfaces, self.neighbors) =
            (ports, processes, connections, interfaces, neighbors);
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
        let neighbors = self.page == NetworkPage::Neighbors || self.neighbors_due == 0;
        self.neighbors_due = if neighbors { NEIGHBOR_SAMPLES - 1 } else { self.neighbors_due - 1 };
        vec![Effect::Network { id: self.next_id, neighbors }]
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
            .chain(
                self.neighbors
                    .iter()
                    .map(|row| &self.neighborhood.neighbors[row.neighbor])
                    .filter(|neighbor| neighbor.mac.is_some())
                    .map(|neighbor| neighbor.address),
            )
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
    crate::screens::close_others(state, crate::screens::Screen::Network);
    let network = &mut state.network;
    network.side = Some(state.active);
    network.details_focused = false;
    network.paused = false;
    network.error = None;
    network.back = None;
    network.revealing = None;
    network.viewer = None;
    network.loaded = false;
    network.traffic = Traffic::default();
    network.sampled = None;
    network.neighbors_due = 0;
    network.request()
}

/// Shows the screen again as it was left, with its rows and charts, when coming back to it from
/// the activity monitor.
pub(crate) fn resume(state: &mut AppState) -> Vec<Effect> {
    if !state.network.loaded {
        return open(state);
    }
    state.overlay = None;
    crate::screens::close_others(state, crate::screens::Screen::Network);
    let network = &mut state.network;
    network.side = Some(state.active);
    network.details_focused = false;
    network.paused = false;
    network.error = None;
    network.back = None;
    network.revealing = None;
    network.sampled = None;
    network.request()
}

/// Alt-N in the activity monitor: the screen on the selected process's traffic, and Esc goes
/// back to the monitor.
pub(crate) fn show_traffic(state: &mut AppState, pid: u32) -> Vec<Effect> {
    let effects = if state.activity.from_network { resume(state) } else { open(state) };
    let network = &mut state.network;
    if !network.is_open() {
        return effects;
    }
    network.back = Some(pid);
    network.filter.clear();
    network.filtering = false;
    network.rebuild();
    keys::show_page(network, NetworkPage::Processes);
    if !network.reveal(pid) {
        network.revealing = Some(pid);
    }
    effects
}

/// Esc on the screen's list: back to the activity monitor it was opened from, or closed.
fn leave(state: &mut AppState) -> Vec<Effect> {
    let back = state.network.back.take();
    close(state);
    let Some(pid) = back else { return Vec::new() };
    let effects = crate::activity::open(state);
    if state.activity.side.is_some() {
        state.activity.reveal(pid, false);
    }
    effects
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

/// Samples once a second while the screen shows the live pages, keeps the cursor in sight, and
/// stops a capture once the screen closes.
pub(crate) fn sync(state: &mut AppState, now: Instant) -> Vec<Effect> {
    if let Some(recording) = state.network.recording.as_mut().filter(|recording| !recording.stopping)
        && state.network.side.is_none()
    {
        recording.stopping = true;
        return vec![Effect::StopCapture { id: recording.id }];
    }
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
    let height = usize::from(list.height.saturating_sub(CHROME_ROWS));
    if let Some(view) = network.viewer.as_deref_mut() {
        view.keep_visible(height);
        return Vec::new();
    }
    if !list.is_empty() {
        network.keep_visible(height);
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
        Ok(mut sample) => {
            let seconds = network.sampled.map_or(0.0, |before| now.duration_since(before).as_secs_f64());
            network.traffic.take(&sample, seconds);
            let table = sample.neighborhood.take();
            let changes = table.as_ref().map(|table| network.gateway_watch.watch(table));
            network.sample = sample;
            capture::sampled(network);
            network.sampled = Some(now);
            network.loaded = true;
            network.error = None;
            network.next = Some(now + REFRESH);
            let mut save = false;
            if let Some(table) = table {
                network.table = table;
                network.places = neighbors::places(&network.sample.interfaces, &network.table);
                let noted =
                    network.memory.note_listed(&network.places, &network.table.neighbors, network.table.read_at);
                save = network.save_due(noted, now);
            }
            network.merge();
            if let Some(pid) = network.revealing.take()
                && !network.reveal(pid)
            {
                let process = state.activity.sample.processes.iter().find(|process| process.key.pid == pid);
                let name = process.map_or_else(|| pid.to_string(), |process| process.name.clone());
                state.notice =
                    Some(tr!(network_process_idle, process = tr!(process_control_subject, name = name, pid = pid)));
            }
            if let Some((_, change)) = changes.into_iter().flatten().next() {
                state.notice = Some(tr!(network_gateway_changed_notice, from = change.from, to = change.to));
            }
            let resolve = state.network.resolve();
            save.then_some(Effect::SaveNeighbors).into_iter().chain(resolve).collect()
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

/// A subnet scan under way: how many of its addresses were probed and answered so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanRun {
    pub id: u64,
    pub subnet: String,
    pub interface: String,
    pub total: usize,
    pub done: usize,
    pub answered: usize,
    pub stopping: bool,
}

/// The ports a subnet scan tries, as the scan question and the details list them.
pub fn scanned_ports() -> String {
    SCAN_PORTS.map(|port| port.to_string()).join(PORT_SEPARATOR)
}

/// S asks before scanning the selected neighbor's subnet.
pub(crate) fn scan(state: &mut AppState) -> Vec<Effect> {
    if state.network.scan.is_some() {
        return Vec::new();
    }
    let Some(scan) = state.network.scan_target() else {
        state.notice = Some(tr!(network_scan_nothing).into());
        return Vec::new();
    };
    state.overlay = Some(crate::Overlay::Confirm(crate::Confirm {
        title: tr!(network_scan_title),
        message: tr!(
            network_scan_question,
            subnet = scan.subnet(),
            interface = scan.interface,
            count = scan.addresses.len(),
            ports = scanned_ports()
        ),
        purpose: crate::ConfirmPurpose::Scan(Box::new(scan)),
        accept: true,
    }));
    Vec::new()
}

pub(crate) fn start_scan(state: &mut AppState, scan: SubnetScan) -> Vec<Effect> {
    state.notice = None;
    let network = &mut state.network;
    network.next_id += 1;
    let id = network.next_id;
    for address in &scan.addresses {
        network.probes.remove(&IpAddr::V4(*address));
        network.scanned.insert(IpAddr::V4(*address));
    }
    network.scan = Some(ScanRun {
        id,
        subnet: scan.subnet(),
        interface: scan.interface.clone(),
        total: scan.addresses.len(),
        done: 0,
        answered: 0,
        stopping: false,
    });
    network.merge();
    vec![Effect::ScanSubnet { id, addresses: scan.addresses }]
}

/// Esc stops a scan under way; the probes already out still report.
pub(crate) fn stop_scan(state: &mut AppState) -> Vec<Effect> {
    let Some(scan) = state.network.scan.as_mut().filter(|scan| !scan.stopping) else { return Vec::new() };
    scan.stopping = true;
    vec![Effect::StopScan { id: scan.id }]
}

/// One scanned address's answers, at `at` in seconds since the epoch.
pub(crate) fn probed(state: &mut AppState, id: u64, probe: Probe, at: u64, now: Instant) -> Vec<Effect> {
    let network = &mut state.network;
    let Some(scan) = network.scan.as_mut().filter(|scan| scan.id == id) else { return Vec::new() };
    scan.done += 1;
    scan.answered += usize::from(probe.answered());
    let interface = scan.interface.clone();
    let noted = network.memory.note_probe(&network.places, &probe, at);
    network.probes.insert(IpAddr::V4(probe.address), Probed { interface, probe });
    let save = network.save_due(noted, now);
    network.merge();
    let resolve = network.resolve();
    save.then_some(Effect::SaveNeighbors).into_iter().chain(resolve).collect()
}

pub(crate) fn scan_finished(state: &mut AppState, id: u64, result: Result<(), String>, now: Instant) -> Vec<Effect> {
    let network = &mut state.network;
    let Some(scan) = network.scan.take_if(|scan| scan.id == id) else { return Vec::new() };
    state.notice = Some(match result {
        Ok(()) if scan.stopping => {
            tr!(network_scan_stopped, subnet = scan.subnet, answered = scan.answered, done = scan.done)
        }
        Ok(()) => tr!(network_scanned, subnet = scan.subnet, answered = scan.answered, total = scan.total),
        Err(error) => tr!(network_scan_failed, subnet = scan.subnet, error = error),
    });
    network.merge();
    network.flush(true, now).then_some(Effect::SaveNeighbors).into_iter().collect()
}

/// F8 on a host that is gone asks before Borg forgets it.
/// P: asks which ports of the selected neighbor to scan, or of a host typed in.
pub(crate) fn scan_ports(state: &mut AppState) -> Vec<Effect> {
    let target = state.network.neighbor().map(|(neighbor, _)| neighbor.address);
    crate::port_scan::open_form(state, target)
}

pub(crate) fn forget(state: &mut AppState) -> Vec<Effect> {
    let Some((neighbor, row)) = state.network.neighbor() else { return Vec::new() };
    let (IpAddr::V4(address), Presence::Offline(_)) = (neighbor.address, row.presence) else {
        state.notice = Some(tr!(network_forget_present).into());
        return Vec::new();
    };
    state.overlay = Some(crate::Overlay::Confirm(crate::Confirm {
        title: tr!(network_forget_title),
        message: tr!(network_forget_question, address = address),
        purpose: crate::ConfirmPurpose::ForgetNeighbor(address),
        accept: false,
    }));
    Vec::new()
}

pub(crate) fn forget_host(state: &mut AppState, address: Ipv4Addr, now: Instant) -> Vec<Effect> {
    let network = &mut state.network;
    let Some(place) = network.places.iter().find(|place| place.contains(address)).cloned() else { return Vec::new() };
    if !network.memory.forget(&place, address) {
        return Vec::new();
    }
    network.memory_changed = true;
    network.merge();
    state.notice = Some(tr!(network_forgot, address = address));
    network.flush(true, now).then_some(Effect::SaveNeighbors).into_iter().collect()
}

/// The file names the traffic viewer opens.
const CAPTURE_EXTENSIONS: [&str; 3] = ["pcap", "pcapng", "cap"];

/// Whether `path` names a capture file the traffic viewer opens.
pub fn is_capture(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| CAPTURE_EXTENSIONS.iter().any(|known| known.eq_ignore_ascii_case(extension)))
}

pub(crate) fn on_capture_key(
    state: &mut AppState,
    dialog: CaptureDialog,
    key: &crossterm::event::KeyEvent,
    now: Instant,
) -> Vec<Effect> {
    capture::on_dialog_key(state, dialog, key, now)
}

pub(crate) fn pick_capture_field(state: &mut AppState, dialog: CaptureDialog, row: usize) {
    capture::pick(state, dialog, row);
}

pub(crate) fn capture_progress(state: &mut AppState, id: u64, progress: CaptureProgress, now: Instant) {
    capture::progress(state, id, progress, now);
}

pub(crate) fn capture_limited(state: &mut AppState, id: u64, limit: CaptureLimit) {
    capture::limited(state, id, limit);
}

pub(crate) fn capture_keys(
    state: &mut AppState,
    id: u64,
    path: std::path::PathBuf,
    proxy: Option<ProxyEndpoint>,
) -> Vec<Effect> {
    capture::keys_ready(state, id, path, proxy)
}

pub(crate) fn capture_proxied(state: &mut AppState, id: u64, proxied: Proxied) {
    capture::proxied(state, id, proxied);
}

/// Puts the variables of a capture that kept TLS keys back in Borg's shell once the shell that
/// was busy when it ended is free.
pub(crate) fn sync_shell_variables(state: &mut AppState) -> Vec<Effect> {
    shell_variables::sync(state)
}

pub(crate) fn capture_done(
    state: &mut AppState,
    id: u64,
    result: Result<(std::path::PathBuf, Capture), CaptureFailure>,
) -> Vec<Effect> {
    capture::done(state, id, result)
}

pub(crate) fn capture_as_administrator(
    state: &mut AppState,
    request: CaptureRequest,
    view_filter: String,
    now: Instant,
) -> Vec<Effect> {
    capture::elevate(state, request, view_filter, now)
}

pub(crate) fn capture_opened(
    state: &mut AppState,
    path: std::path::PathBuf,
    result: Result<Capture, String>,
) -> Vec<Effect> {
    capture::opened(state, path, result)
}

pub(crate) fn show_traffic_page(network: &mut Network, page: TrafficPage) {
    viewer::show_page(network, page);
}

#[cfg(test)]
mod tests;
