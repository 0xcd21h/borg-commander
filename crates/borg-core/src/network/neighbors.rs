use std::{
    collections::{HashMap, HashSet},
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::OnceLock,
    time::Duration,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use super::{HeardHost, Interface, InterfaceKind};

mod map;
mod memory;
mod routes;

pub use map::{MapParts, NeighborEntry, entries};
pub use memory::{KnownHost, KnownNetwork, KnownRoute, NEIGHBORS_FORMAT, NeighborMemory, Noted, SavedNeighbors};
pub(crate) use routes::network_part;
pub use routes::{
    Discovery, InternetRoute, MAX_HOPS, Origin, ROUTER_HOSTS, Route, Routed, Trace, WIDEST_NETWORK, Way, attached,
    discovery, holding, inside, internet, routed, shows, table_networks,
};

const MAC_OCTETS: usize = 6;
const MAC_SEPARATORS: [char; 2] = [':', '-'];
const OCTET_DIGITS: usize = 2;
const HEX: u32 = 16;
const BITS_PER_OCTET: u32 = 8;
const LOCALLY_ADMINISTERED: u8 = 0x02;
const GROUP: u8 = 0x01;
const OUI_OCTETS: usize = 3;
const VENDORS: &str = include_str!("neighbors/vendors.tsv");
const VENDOR_SEPARATOR: char = '\t';
const IPV4_BITS: u8 = 32;
const IPV6_BITS: u8 = 128;
pub(crate) const SCAN_PREFIX: u8 = 24;
const POINT_TO_POINT_PREFIX: u8 = 31;
const SCANNED_KINDS: [InterfaceKind; 3] = [InterfaceKind::Ethernet, InterfaceKind::WiFi, InterfaceKind::Bridge];
const INVALID_MAC: &str = "invalid MAC address";
pub const SCAN_PORTS: [u16; 3] = [22, 80, 443];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Mac(pub [u8; MAC_OCTETS]);

impl Mac {
    pub fn parse(text: &str) -> Option<Self> {
        let mut octets = [0_u8; MAC_OCTETS];
        let mut parts = text.trim().split(MAC_SEPARATORS);
        for octet in &mut octets {
            let part = parts.next()?;
            if part.is_empty() || part.len() > OCTET_DIGITS {
                return None;
            }
            *octet = u8::from_str_radix(part, HEX).ok()?;
        }
        parts.next().is_none().then_some(Self(octets))
    }

    pub fn locally_administered(self) -> bool {
        self.0[0] & LOCALLY_ADMINISTERED != 0
    }

    pub fn group(self) -> bool {
        self.0[0] & GROUP != 0
    }

    pub fn unset(self) -> bool {
        self.0 == [0; MAC_OCTETS]
    }

    fn oui(self) -> u32 {
        self.0[..OUI_OCTETS].iter().fold(0, |oui, octet| (oui << BITS_PER_OCTET) | u32::from(*octet))
    }
}

impl fmt::Display for Mac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{g:02x}")
    }
}

impl Serialize for Mac {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Mac {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| D::Error::custom(INVALID_MAC))
    }
}

fn vendors() -> &'static [(u32, &'static str)] {
    static TABLE: OnceLock<Vec<(u32, &'static str)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table: Vec<(u32, &'static str)> = VENDORS
            .lines()
            .filter_map(|line| {
                let (oui, name) = line.split_once(VENDOR_SEPARATOR)?;
                Some((u32::from_str_radix(oui, HEX).ok()?, name))
            })
            .collect();
        table.sort_unstable_by_key(|(oui, _)| *oui);
        table
    })
}

pub fn vendor(mac: Mac) -> Option<&'static str> {
    if mac.locally_administered() || mac.group() {
        return None;
    }
    let table = vendors();
    table.binary_search_by_key(&mac.oui(), |(oui, _)| *oui).ok().map(|index| table[index].1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NeighborState {
    Reachable,
    Stale,
    Delay,
    Probe,
    Incomplete,
    Failed,
    Permanent,
}

const NEIGHBOR_STATES: [(NeighborState, &str); 7] = [
    (NeighborState::Reachable, "REACHABLE"),
    (NeighborState::Stale, "STALE"),
    (NeighborState::Delay, "DELAY"),
    (NeighborState::Probe, "PROBE"),
    (NeighborState::Incomplete, "INCOMPLETE"),
    (NeighborState::Failed, "FAILED"),
    (NeighborState::Permanent, "PERMANENT"),
];

impl NeighborState {
    pub fn name(self) -> &'static str {
        NEIGHBOR_STATES.iter().find(|(state, _)| *state == self).map_or("", |(_, name)| name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbor {
    pub address: IpAddr,
    pub mac: Option<Mac>,
    pub interface: String,
    pub state: Option<NeighborState>,
    pub router: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gateway {
    pub address: IpAddr,
    pub interface: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Neighborhood {
    pub neighbors: Vec<Neighbor>,
    pub gateways: Vec<Gateway>,
    /// The IPv4 routes to networks other than the default ones.
    pub routes: Vec<Route>,
    pub dns: Vec<IpAddr>,
    pub error: Option<String>,
    /// When the tables were read, in seconds since the epoch.
    pub read_at: u64,
}

impl Neighborhood {
    pub fn gateway(&self, neighbor: &Neighbor) -> bool {
        neighbor.router
            || self
                .gateways
                .iter()
                .any(|gateway| gateway.address == neighbor.address && gateway.interface == neighbor.interface)
    }

    pub fn gateways_on<'a>(&'a self, interface: &'a str) -> impl Iterator<Item = &'a Gateway> + 'a {
        self.gateways.iter().filter(move |gateway| gateway.interface == interface)
    }

    pub fn first_gateway(&self) -> Option<&Gateway> {
        self.gateways.iter().find(|gateway| gateway.address.is_ipv4()).or(self.gateways.first())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacChange {
    pub from: Mac,
    pub to: Mac,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GatewayKey {
    address: IpAddr,
    interface: String,
}

#[derive(Debug, Clone, Default)]
pub struct GatewayWatch {
    first: HashMap<GatewayKey, Mac>,
    changes: HashMap<GatewayKey, MacChange>,
}

impl GatewayWatch {
    pub fn watch(&mut self, neighborhood: &Neighborhood) -> Vec<(IpAddr, MacChange)> {
        let mut fresh = Vec::new();
        for neighbor in &neighborhood.neighbors {
            let Some(mac) = neighbor.mac.filter(|_| neighborhood.gateway(neighbor)) else { continue };
            let key = GatewayKey { address: neighbor.address, interface: neighbor.interface.clone() };
            let first = *self.first.entry(key.clone()).or_insert(mac);
            if mac == first {
                continue;
            }
            let change = MacChange { from: first, to: mac };
            if self.changes.insert(key, change).is_none() {
                fresh.push((neighbor.address, change));
            }
        }
        fresh
    }

    pub fn change(&self, neighbor: &Neighbor) -> Option<MacChange> {
        let key = GatewayKey { address: neighbor.address, interface: neighbor.interface.clone() };
        self.changes.get(&key).copied()
    }
}

/// How Borg knows of a host: the neighbor tables list it, a scan found it, listening heard it,
/// or it is remembered from an earlier visit and was last seen at a time in seconds since the
/// epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Listed,
    Answered,
    Heard,
    Offline(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeighborRow {
    pub neighbor: usize,
    pub gateway: bool,
    pub shared: bool,
    pub changed: Option<MacChange>,
    pub presence: Presence,
    /// Listening heard what the host announced, this session or before.
    pub heard: bool,
}

impl NeighborRow {
    pub fn randomized(&self, neighbor: &Neighbor) -> bool {
        neighbor.mac.is_some_and(Mac::locally_administered)
    }
}

fn own_addresses(interfaces: &[Interface]) -> HashSet<IpAddr> {
    interfaces.iter().flat_map(|interface| interface.addresses.iter().map(|(address, _)| *address)).collect()
}

fn broadcasts(interfaces: &[Interface]) -> HashSet<IpAddr> {
    interfaces
        .iter()
        .flat_map(|interface| &interface.addresses)
        .filter_map(|(address, prefix)| match address {
            IpAddr::V4(address) if *prefix < POINT_TO_POINT_PREFIX => {
                Some(IpAddr::V4(Ipv4Addr::from(u32::from(*address) | !mask(*prefix))))
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn mask(prefix: u8) -> u32 {
    u32::MAX.checked_shl(u32::from(IPV4_BITS.saturating_sub(prefix))).unwrap_or(0)
}

struct Excluded {
    own: HashSet<IpAddr>,
    broadcast: HashSet<IpAddr>,
}

impl Excluded {
    fn shows(&self, neighbor: &Neighbor, presence: Presence, scanned: &HashSet<IpAddr>) -> bool {
        let address = neighbor.address;
        let special = match address {
            IpAddr::V4(v4) => v4.is_broadcast() || v4.is_multicast() || v4.is_loopback() || v4.is_unspecified(),
            IpAddr::V6(v6) => v6.is_multicast() || v6.is_loopback() || v6.is_unspecified(),
        };
        let group = neighbor.mac.is_some_and(Mac::group);
        let unanswered = presence == Presence::Listed && neighbor.mac.is_none() && scanned.contains(&address);
        !special && !group && !unanswered && !self.own.contains(&address) && !self.broadcast.contains(&address)
    }
}

pub(crate) fn rows(
    neighborhood: &Neighborhood,
    presence: &[Presence],
    interfaces: &[Interface],
    watch: &GatewayWatch,
    scanned: &HashSet<IpAddr>,
    heard: &HashSet<IpAddr>,
) -> Vec<NeighborRow> {
    let excluded = Excluded { own: own_addresses(interfaces), broadcast: broadcasts(interfaces) };
    let seen = |index: usize| presence.get(index).copied().unwrap_or(Presence::Listed);
    let listed: Vec<(usize, &Neighbor)> = neighborhood
        .neighbors
        .iter()
        .enumerate()
        .filter(|(index, neighbor)| excluded.shows(neighbor, seen(*index), scanned))
        .collect();
    let here = |index: usize| !matches!(seen(index), Presence::Offline(_));
    let mut sharing: HashMap<(&str, Mac), usize> = HashMap::new();
    for (_, neighbor) in listed.iter().filter(|(index, neighbor)| neighbor.address.is_ipv4() && here(*index)) {
        if let Some(mac) = neighbor.mac {
            *sharing.entry((neighbor.interface.as_str(), mac)).or_default() += 1;
        }
    }
    listed
        .into_iter()
        .map(|(index, neighbor)| NeighborRow {
            neighbor: index,
            gateway: neighborhood.gateway(neighbor),
            shared: here(index)
                && neighbor.address.is_ipv4()
                && neighbor.mac.is_some_and(|mac| {
                    sharing.get(&(neighbor.interface.as_str(), mac)).is_some_and(|count| *count > 1)
                }),
            changed: watch.change(neighbor),
            presence: seen(index),
            heard: heard.contains(&neighbor.address),
        })
        .collect()
}

/// A scanned address: whether it answered ping and how fast, which of the scanned ports took
/// the connection, whether one refused it, which a host that is up does too, and for a discovery
/// the way to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub address: Ipv4Addr,
    pub ping: Option<Duration>,
    pub open: Vec<u16>,
    pub refused: bool,
    pub trace: Option<Trace>,
}

impl Probe {
    pub fn answered(&self) -> bool {
        self.ping.is_some() || !self.open.is_empty() || self.refused
    }
}

/// A probe with the interface whose subnet was scanned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probed {
    pub interface: String,
    pub probe: Probe,
}

/// A network this machine sits on: its IPv4 subnet on an interface, and the MAC address of its
/// gateway, which tells two networks with the same addresses apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalNetwork {
    pub interface: String,
    pub network: Ipv4Addr,
    pub prefix: u8,
    pub own: Ipv4Addr,
    pub gateway: Option<Mac>,
}

impl LocalNetwork {
    pub fn contains(&self, address: Ipv4Addr) -> bool {
        u32::from(address) & mask(self.prefix) == u32::from(self.network)
    }

    /// Whether `address` is another host's: inside the subnet, and neither this machine, the
    /// network address nor the broadcast address.
    pub fn holds(&self, address: Ipv4Addr) -> bool {
        let broadcast = u32::from(self.network) | !mask(self.prefix);
        self.contains(address) && address != self.own && address != self.network && u32::from(address) != broadcast
    }
}

/// The scanned ports a host took connections on: this session's scan, or else what Borg
/// remembers from an earlier one.
pub fn open_ports<'a>(
    probes: &'a HashMap<IpAddr, Probed>,
    memory: &'a NeighborMemory,
    places: &[LocalNetwork],
    address: IpAddr,
) -> Option<&'a [u16]> {
    match probes.get(&address) {
        Some(probed) => Some(&probed.probe.open),
        None => memory.host(places, address).filter(|host| host.scanned.is_some()).map(|host| host.ports.as_slice()),
    }
}

fn home_address(interface: &Interface) -> Option<(Ipv4Addr, u8)> {
    if !interface.up || !SCANNED_KINDS.contains(&interface.kind) {
        return None;
    }
    interface.addresses.iter().find_map(|(address, prefix)| match address {
        IpAddr::V4(v4) if !v4.is_loopback() && !v4.is_link_local() && *prefix < POINT_TO_POINT_PREFIX => {
            Some((*v4, *prefix))
        }
        _ => None,
    })
}

/// The networks this machine sits on whose gateway, if they have one, answered with its MAC
/// address.
pub fn places(interfaces: &[Interface], neighborhood: &Neighborhood) -> Vec<LocalNetwork> {
    interfaces
        .iter()
        .filter_map(|interface| {
            let (own, prefix) = home_address(interface)?;
            let gateway = match neighborhood.gateways_on(&interface.name).find(|gateway| gateway.address.is_ipv4()) {
                Some(gateway) => Some(
                    neighborhood
                        .neighbors
                        .iter()
                        .find(|neighbor| neighbor.address == gateway.address && neighbor.interface == interface.name)?
                        .mac?,
                ),
                None => None,
            };
            let network = Ipv4Addr::from(u32::from(own) & mask(prefix));
            Some(LocalNetwork { interface: interface.name.clone(), network, prefix, own, gateway })
        })
        .collect()
}

/// The neighbor tables with the scanned hosts that answered added, and the remembered hosts that
/// are not here now, each with how Borg knows of it.
pub(crate) fn merge(
    table: &Neighborhood,
    probes: &HashMap<IpAddr, Probed>,
    hearing: &[HeardHost],
    memory: &NeighborMemory,
    places: &[LocalNetwork],
) -> (Neighborhood, Vec<Presence>) {
    let mut merged = table.clone();
    let mut presence = vec![Presence::Listed; merged.neighbors.len()];
    let mut answered: Vec<&Probed> = probes.values().filter(|probed| probed.probe.answered()).collect();
    answered.sort_by_key(|probed| probed.probe.address);
    for probed in &answered {
        let address = IpAddr::V4(probed.probe.address);
        match merged.neighbors.iter().position(|neighbor| neighbor.address == address) {
            Some(index) if merged.neighbors[index].mac.is_none() => presence[index] = Presence::Answered,
            Some(_) => {}
            None => {
                merged.neighbors.push(Neighbor {
                    address,
                    mac: None,
                    interface: probed.interface.clone(),
                    state: None,
                    router: false,
                });
                presence.push(Presence::Answered);
            }
        }
    }
    for host in hearing {
        match merged.neighbors.iter().position(|neighbor| neighbor.address == host.address) {
            Some(index) => {
                let neighbor = &mut merged.neighbors[index];
                neighbor.router |= host.router;
                if neighbor.mac.is_none() {
                    neighbor.mac = host.mac;
                    if presence[index] == Presence::Listed {
                        presence[index] = Presence::Heard;
                    }
                }
            }
            None => {
                merged.neighbors.push(Neighbor {
                    address: host.address,
                    mac: host.mac,
                    interface: host.interface.clone(),
                    state: None,
                    router: host.router,
                });
                presence.push(Presence::Heard);
            }
        }
    }
    let here: HashSet<IpAddr> = table
        .neighbors
        .iter()
        .filter(|neighbor| neighbor.mac.is_some())
        .map(|neighbor| neighbor.address)
        .chain(answered.iter().map(|probed| IpAddr::V4(probed.probe.address)))
        .chain(hearing.iter().map(|host| host.address))
        .collect();
    for place in places {
        for host in memory.hosts(place).iter().filter(|host| !here.contains(&IpAddr::V4(host.address))) {
            let address = IpAddr::V4(host.address);
            let entry =
                Neighbor { address, mac: host.mac, interface: place.interface.clone(), state: None, router: false };
            let at = merged.neighbors.iter().position(|neighbor| neighbor.address == address);
            match at {
                Some(index) => {
                    merged.neighbors[index] = entry;
                    presence[index] = Presence::Offline(host.seen);
                }
                None => {
                    merged.neighbors.push(entry);
                    presence.push(Presence::Offline(host.seen));
                }
            }
        }
    }
    (merged, presence)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subnet {
    pub network: IpAddr,
    pub prefix: u8,
    pub own: IpAddr,
}

fn network_of(address: IpAddr, prefix: u8) -> IpAddr {
    match address {
        IpAddr::V4(v4) => IpAddr::V4(Ipv4Addr::from(u32::from(v4) & mask(prefix))),
        IpAddr::V6(v6) => {
            let mask = u128::MAX.checked_shl(u32::from(IPV6_BITS.saturating_sub(prefix))).unwrap_or(0);
            IpAddr::V6(Ipv6Addr::from(u128::from(v6) & mask))
        }
    }
}

pub fn subnet(interfaces: &[Interface], neighbor: &Neighbor) -> Option<Subnet> {
    let interface = interfaces.iter().find(|interface| interface.name == neighbor.interface)?;
    interface.addresses.iter().find_map(|(own, prefix)| {
        let network = network_of(*own, *prefix);
        let same = own.is_ipv4() == neighbor.address.is_ipv4() && network_of(neighbor.address, *prefix) == network;
        same.then_some(Subnet { network, prefix: *prefix, own: *own })
    })
}

/// The addresses a subnet scan probes: every host address of a small subnet, or of the /24
/// around this machine in a larger one, without this machine's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubnetScan {
    pub interface: String,
    pub network: Ipv4Addr,
    pub prefix: u8,
    pub addresses: Vec<Ipv4Addr>,
    /// Whether the network is on the interface's own link, where the system asks each host for
    /// its MAC address.
    pub on_link: bool,
}

/// Why a typed network cannot be scanned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanProblem {
    Unreadable,
    TooWide,
    NoRoute,
}

const PREFIX_SEPARATOR: char = '/';
const NARROWEST_SCAN: u8 = 30;

/// Reads `a.b.c.d/prefix`, or an address alone as the /24 holding it.
pub fn typed_network(text: &str) -> Result<(Ipv4Addr, u8), ScanProblem> {
    let text = text.trim();
    let (address, prefix) = match text.split_once(PREFIX_SEPARATOR) {
        Some((address, prefix)) => (address, prefix.parse::<u8>().map_err(|_| ScanProblem::Unreadable)?),
        None => (text, SCAN_PREFIX),
    };
    let address: Ipv4Addr = address.parse().map_err(|_| ScanProblem::Unreadable)?;
    match prefix {
        prefix if prefix < SCAN_PREFIX => Err(ScanProblem::TooWide),
        prefix if prefix > NARROWEST_SCAN => Err(ScanProblem::Unreadable),
        prefix => Ok((Ipv4Addr::from(u32::from(address) & mask(prefix)), prefix)),
    }
}

fn host_addresses(network: Ipv4Addr, prefix: u8, own: &HashSet<IpAddr>) -> Vec<Ipv4Addr> {
    let mask = mask(prefix);
    let network = u32::from(network) & mask;
    let broadcast = network | !mask;
    (network + 1..broadcast).map(Ipv4Addr::from).filter(|address| !own.contains(&IpAddr::V4(*address))).collect()
}

impl SubnetScan {
    pub fn subnet(&self) -> String {
        format!("{}/{}", self.network, self.prefix)
    }

    pub fn of(interface: &Interface) -> Option<Self> {
        let (own, prefix) = home_address(interface)?;
        let prefix = prefix.max(SCAN_PREFIX);
        let network = Ipv4Addr::from(u32::from(own) & mask(prefix));
        let own = HashSet::from([IpAddr::V4(own)]);
        let addresses = host_addresses(network, prefix, &own);
        Some(Self { interface: interface.name.clone(), network, prefix, addresses, on_link: true })
    }

    /// The scan of a typed network: on the link of the interface whose subnet holds it, else
    /// through the routed network holding it, else the way internet traffic leaves.
    pub fn typed(
        text: &str,
        interfaces: &[Interface],
        routed: &[Routed],
        internet: &[InternetRoute],
    ) -> Result<Self, ScanProblem> {
        let (network, prefix) = typed_network(text)?;
        let linked = interfaces.iter().filter(|interface| interface.up).find(|interface| {
            interface.addresses.iter().any(|(own, own_prefix)| match own {
                IpAddr::V4(own) => {
                    *own_prefix < POINT_TO_POINT_PREFIX && inside(network, network_part(*own, *own_prefix), *own_prefix)
                }
                IpAddr::V6(_) => false,
            })
        });
        let (interface, on_link) = match linked {
            Some(interface) => (interface.name.clone(), true),
            None => {
                let through = holding(routed, network).map(|index| routed[index].interface.clone());
                let exit = || internet.iter().find(|route| route.in_use).map(|route| route.interface.clone());
                (through.or_else(exit).ok_or(ScanProblem::NoRoute)?, false)
            }
        };
        let addresses = host_addresses(network, prefix, &own_addresses(interfaces));
        Ok(Self { interface, network, prefix, addresses, on_link })
    }
}

#[cfg(test)]
mod tests;
