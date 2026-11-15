//! Each page's rows, built from a sample and its traffic.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    net::IpAddr,
};

const CIDR_SEPARATOR: char = '/';

use borg_i18n::tr;

use super::{
    GatewayWatch, Haystack, Interface, InternetRoute, LocalNetwork, Neighbor, NeighborEntry, NeighborMemory,
    NeighborRow, NeighborState, Neighborhood, NetworkSample, PAGES, PortKey, Presence, Probed, Rate, Routed, Socket,
    TcpState, Traffic, Transport, filter::NetworkFilter, model::service, vendor,
};

/// Who can reach a listening port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Exposure {
    /// Bound to every address: other machines on any network the computer joins.
    Everywhere,
    /// Bound to specific addresses only.
    Addresses,
    /// Bound to loopback only: programs on this machine.
    ThisMachine,
}

impl Exposure {
    fn of(addresses: &[IpAddr]) -> Self {
        if addresses.iter().any(IpAddr::is_unspecified) {
            Self::Everywhere
        } else if addresses.iter().all(IpAddr::is_loopback) {
            Self::ThisMachine
        } else {
            Self::Addresses
        }
    }
}

/// A port a process listens on, with the addresses it is bound to and the connections to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortRow {
    pub transport: Transport,
    pub port: u16,
    pub pid: Option<u32>,
    pub addresses: Vec<IpAddr>,
    pub exposure: Exposure,
    pub connections: usize,
    pub rate: Rate,
}

impl PortRow {
    pub fn key(&self) -> PortKey {
        PortKey { transport: self.transport, port: self.port, pid: self.pid }
    }

    /// Whether `socket` came in through this port.
    pub(crate) fn covers(&self, socket: &Socket) -> bool {
        self.addresses.iter().any(|address| socket.came_through(self.key(), *address))
    }
}

/// A process with sockets: how many connect and listen, and its traffic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRow {
    pub pid: Option<u32>,
    pub connections: usize,
    pub listening: usize,
    pub rate: Rate,
    /// Whether the system counts the bytes of any of its sockets.
    pub counted: bool,
}

/// A socket that talks to one other end, as an index into the sample's sockets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionRow {
    pub socket: usize,
    pub rate: Rate,
}

/// An interface, as an index into the sample's interfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterfaceRow {
    pub interface: usize,
    pub rate: Rate,
}

/// What rows are built from: the sample, its traffic, and the names addresses resolve to.
pub(crate) struct Source<'a> {
    pub(crate) sample: &'a NetworkSample,
    pub(crate) traffic: &'a Traffic,
    pub(crate) hosts: &'a HashMap<IpAddr, Option<String>>,
    pub(crate) neighborhood: &'a Neighborhood,
    pub(crate) presence: &'a [Presence],
    pub(crate) watch: &'a GatewayWatch,
    pub(crate) scanned: &'a HashSet<IpAddr>,
    pub(crate) probes: &'a HashMap<IpAddr, Probed>,
    pub(crate) memory: &'a NeighborMemory,
    pub(crate) places: &'a [LocalNetwork],
    pub(crate) routed: &'a [Routed],
    pub(crate) internet: &'a [InternetRoute],
    pub(crate) filter: &'a NetworkFilter,
}

impl Source<'_> {
    fn name(&self, pid: Option<u32>) -> Option<String> {
        pid.and_then(|pid| self.sample.names.get(&pid)).cloned()
    }

    fn host_words(&self, address: IpAddr) -> impl Iterator<Item = String> + '_ {
        let name = self.hosts.get(&address).cloned().flatten();
        std::iter::once(address.to_string()).chain(name)
    }

    pub(crate) fn ports(&self) -> Vec<PortRow> {
        let mut rows: BTreeMap<(Transport, u16, Option<u32>), PortRow> = BTreeMap::new();
        for socket in self.sample.sockets.iter().filter(|s| s.listens()) {
            let row = rows.entry((socket.transport, socket.local.port(), socket.pid)).or_insert_with(|| PortRow {
                transport: socket.transport,
                port: socket.local.port(),
                pid: socket.pid,
                addresses: Vec::new(),
                exposure: Exposure::Everywhere,
                connections: 0,
                rate: Rate::default(),
            });
            if !row.addresses.contains(&socket.local.ip()) {
                row.addresses.push(socket.local.ip());
            }
        }
        let mut rows: Vec<_> = rows.into_values().collect();
        for socket in &self.sample.sockets {
            if let Some(row) = rows.iter_mut().find(|row| row.covers(socket)) {
                row.connections += 1;
            }
        }
        for row in &mut rows {
            row.addresses.sort();
            row.exposure = Exposure::of(&row.addresses);
            row.rate = self.traffic.port(row.key());
        }
        rows
    }

    pub(crate) fn port_haystack(&self, row: &PortRow) -> Haystack {
        Haystack {
            process: self.name(row.pid),
            pid: row.pid,
            ports: vec![row.port],
            services: service(row.port).into_iter().collect(),
            hosts: row.addresses.iter().map(IpAddr::to_string).collect(),
            state: (row.transport == Transport::Tcp).then(|| TcpState::Listen.name()),
            protocols: vec![row.transport.name()],
            interface: None,
            labels: vec![super::exposure_label(row.exposure).to_owned()],
            ..Haystack::default()
        }
    }

    pub(crate) fn processes(&self) -> Vec<ProcessRow> {
        let mut listening: HashMap<Option<u32>, BTreeSet<(Transport, u16)>> = HashMap::new();
        let mut rows: HashMap<Option<u32>, ProcessRow> = HashMap::new();
        for socket in &self.sample.sockets {
            let row = rows.entry(socket.pid).or_insert_with(|| ProcessRow {
                pid: socket.pid,
                connections: 0,
                listening: 0,
                rate: self.traffic.process(socket.pid),
                counted: false,
            });
            row.counted |= socket.received.is_some();
            if socket.connects() {
                row.connections += 1;
            } else if socket.listens() {
                listening.entry(socket.pid).or_default().insert((socket.transport, socket.local.port()));
            }
        }
        rows.into_values()
            .map(|row| ProcessRow { listening: listening.get(&row.pid).map_or(0, BTreeSet::len), ..row })
            .collect()
    }

    pub(crate) fn process_haystack(&self, pid: Option<u32>) -> Haystack {
        let sockets: Vec<_> = self.sample.sockets.iter().filter(|s| s.pid == pid).collect();
        let ports: BTreeSet<u16> = sockets
            .iter()
            .filter_map(|s| if s.listens() { Some(s.local.port()) } else { s.remote.map(|r| r.port()) })
            .collect();
        let remotes: BTreeSet<IpAddr> = sockets.iter().filter_map(|s| s.remote.map(|r| r.ip())).collect();
        Haystack {
            process: self.name(pid),
            pid,
            services: ports.iter().filter_map(|port| service(*port)).collect(),
            ports: ports.into_iter().collect(),
            hosts: remotes.into_iter().flat_map(|address| self.host_words(address)).collect(),
            ..Haystack::default()
        }
    }

    pub(crate) fn connections(&self) -> Vec<ConnectionRow> {
        self.sample
            .sockets
            .iter()
            .enumerate()
            .filter(|(_, socket)| socket.connects())
            .map(|(index, socket)| ConnectionRow { socket: index, rate: self.traffic.socket(&socket.key()) })
            .collect()
    }

    pub(crate) fn connection_haystack(&self, socket: &Socket) -> Haystack {
        let remote = socket.remote.map(|r| r.port());
        let ports: Vec<u16> = remote.into_iter().chain([socket.local.port()]).collect();
        Haystack {
            process: self.name(socket.pid),
            pid: socket.pid,
            services: ports.iter().filter_map(|port| service(*port)).collect(),
            ports,
            hosts: socket
                .remote
                .into_iter()
                .flat_map(|remote| self.host_words(remote.ip()))
                .chain([socket.local.ip().to_string()])
                .collect(),
            state: socket.state.map(TcpState::name),
            protocols: vec![socket.transport.name()],
            interface: None,
            labels: Vec::new(),
            ..Haystack::default()
        }
    }

    pub(crate) fn interfaces(&self) -> Vec<InterfaceRow> {
        self.sample
            .interfaces
            .iter()
            .enumerate()
            .map(|(index, interface)| InterfaceRow { interface: index, rate: self.traffic.interface(&interface.name) })
            .collect()
    }

    pub(crate) fn interface_haystack(&self, interface: &Interface) -> Haystack {
        Haystack {
            hosts: interface.addresses.iter().map(|(address, _)| address.to_string()).collect(),
            interface: Some(interface.name.clone()),
            labels: vec![super::kind_label(interface.kind).to_owned()],
            mac: interface.mac.clone(),
            ..Haystack::default()
        }
    }

    pub(crate) fn neighbors(&self) -> Vec<NeighborRow> {
        super::neighbors::rows(self.neighborhood, self.presence, &self.sample.interfaces, self.watch, self.scanned)
    }

    pub(crate) fn neighbor_haystack(&self, row: &NeighborRow) -> Haystack {
        let neighbor = &self.neighborhood.neighbors[row.neighbor];
        Haystack {
            hosts: self.host_words(neighbor.address).collect(),
            state: neighbor.state.map(NeighborState::name),
            interface: Some(neighbor.interface.clone()),
            ports: super::neighbors::open_ports(self.probes, self.memory, self.places, neighbor.address)
                .map(<[u16]>::to_vec)
                .unwrap_or_default(),
            labels: neighbor_notes(row, neighbor)
                .into_iter()
                .chain(matches!(row.presence, Presence::Offline(_)).then(|| tr!(network_offline_state)))
                .map(str::to_owned)
                .collect(),
            mac: neighbor.mac.map(|mac| mac.to_string()),
            vendor: neighbor.mac.and_then(vendor),
            network: self.network_of(neighbor),
            ..Haystack::default()
        }
    }

    /// The network a host is on: the routed one holding it, or its interface's own subnet.
    fn network_of(&self, neighbor: &Neighbor) -> Option<String> {
        let IpAddr::V4(address) = neighbor.address else { return None };
        if let Some(index) = super::neighbors::holding(self.routed, address) {
            return Some(self.routed[index].cidr());
        }
        let place = self.places.iter().find(|place| place.interface == neighbor.interface && place.contains(address));
        place.map(|place| format!("{}{CIDR_SEPARATOR}{}", place.network, place.prefix))
    }

    /// What a heading of the Neighbors page offers the filter: an interface's name, kind and
    /// addresses, a routed network and the routers on its way, or the way to the internet.
    pub(crate) fn entry_haystack(&self, entry: NeighborEntry) -> Haystack {
        match entry {
            NeighborEntry::Host(row) => self.neighbor_haystack(&row),
            NeighborEntry::Interface(index) => {
                self.sample.interfaces.get(index).map_or_else(Haystack::default, |interface| Haystack {
                    network: super::neighbors::attached(std::slice::from_ref(interface))
                        .first()
                        .map(|(network, prefix)| format!("{network}{CIDR_SEPARATOR}{prefix}")),
                    ..self.interface_haystack(interface)
                })
            }
            NeighborEntry::Network(index) => self.routed.get(index).map_or_else(Haystack::default, |routed| Haystack {
                hosts: routed.path().iter().map(ToString::to_string).collect(),
                interface: Some(routed.interface.clone()),
                network: Some(routed.cidr()),
                ..Haystack::default()
            }),
            NeighborEntry::Internet(index) => {
                self.internet.get(index).map_or_else(Haystack::default, |route| Haystack {
                    hosts: route.gateways.iter().map(ToString::to_string).collect(),
                    interface: Some(route.interface.clone()),
                    labels: vec![tr!(network_internet).to_owned()],
                    ..Haystack::default()
                })
            }
        }
    }

    /// Each page's rows that the filter keeps, and how many there are without it.
    pub(crate) fn filtered(&self) -> (Rows, [usize; PAGES]) {
        let (mut ports, mut processes, mut connections, mut interfaces, mut neighbors) =
            (self.ports(), self.processes(), self.connections(), self.interfaces(), self.neighbors());
        let totals = [ports.len(), processes.len(), connections.len(), interfaces.len(), neighbors.len()];
        ports.retain(|row| self.filter.matches(&self.port_haystack(row)));
        processes.retain(|row| self.filter.matches(&self.process_haystack(row.pid)));
        connections.retain(|row| self.filter.matches(&self.connection_haystack(&self.sample.sockets[row.socket])));
        interfaces.retain(|row| self.filter.matches(&self.interface_haystack(&self.sample.interfaces[row.interface])));
        neighbors.retain(|row| self.filter.matches(&self.neighbor_haystack(row)));
        (Rows { ports, processes, connections, interfaces, neighbors }, totals)
    }
}

/// Every page's rows.
pub(crate) struct Rows {
    pub(crate) ports: Vec<PortRow>,
    pub(crate) processes: Vec<ProcessRow>,
    pub(crate) connections: Vec<ConnectionRow>,
    pub(crate) interfaces: Vec<InterfaceRow>,
    pub(crate) neighbors: Vec<NeighborRow>,
}

pub fn neighbor_notes(row: &NeighborRow, neighbor: &Neighbor) -> Vec<&'static str> {
    [
        row.gateway.then(|| tr!(network_gateway_note)),
        row.changed.map(|_| tr!(network_gateway_changed_note)),
        row.shared.then(|| tr!(network_mac_shared_note)),
        row.randomized(neighbor).then(|| tr!(network_randomized_note)),
    ]
    .into_iter()
    .flatten()
    .collect()
}
