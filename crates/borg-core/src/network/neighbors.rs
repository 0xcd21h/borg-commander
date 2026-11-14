use std::{
    collections::{HashMap, HashSet},
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::OnceLock,
};

use super::{Interface, InterfaceKind};

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
const SWEEP_PREFIX: u8 = 24;
const POINT_TO_POINT_PREFIX: u8 = 31;
const SWEPT_KINDS: [InterfaceKind; 3] = [InterfaceKind::Ethernet, InterfaceKind::WiFi, InterfaceKind::Bridge];

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
    pub dns: Vec<IpAddr>,
    pub error: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeighborRow {
    pub neighbor: usize,
    pub gateway: bool,
    pub shared: bool,
    pub changed: Option<MacChange>,
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

fn mask(prefix: u8) -> u32 {
    u32::MAX.checked_shl(u32::from(IPV4_BITS.saturating_sub(prefix))).unwrap_or(0)
}

fn shown(neighbor: &Neighbor, own: &HashSet<IpAddr>, broadcast: &HashSet<IpAddr>, swept: &HashSet<IpAddr>) -> bool {
    let address = neighbor.address;
    let special = match address {
        IpAddr::V4(v4) => v4.is_broadcast() || v4.is_multicast() || v4.is_loopback() || v4.is_unspecified(),
        IpAddr::V6(v6) => v6.is_multicast() || v6.is_loopback() || v6.is_unspecified(),
    };
    let group = neighbor.mac.is_some_and(Mac::group);
    let unanswered = neighbor.mac.is_none() && swept.contains(&address);
    !special && !group && !unanswered && !own.contains(&address) && !broadcast.contains(&address)
}

pub(crate) fn rows(
    neighborhood: &Neighborhood,
    interfaces: &[Interface],
    watch: &GatewayWatch,
    swept: &HashSet<IpAddr>,
) -> Vec<NeighborRow> {
    let (own, broadcast) = (own_addresses(interfaces), broadcasts(interfaces));
    let listed: Vec<(usize, &Neighbor)> = neighborhood
        .neighbors
        .iter()
        .enumerate()
        .filter(|(_, neighbor)| shown(neighbor, &own, &broadcast, swept))
        .collect();
    let mut sharing: HashMap<(&str, Mac), usize> = HashMap::new();
    for (_, neighbor) in listed.iter().filter(|(_, neighbor)| neighbor.address.is_ipv4()) {
        if let Some(mac) = neighbor.mac {
            *sharing.entry((neighbor.interface.as_str(), mac)).or_default() += 1;
        }
    }
    listed
        .into_iter()
        .map(|(index, neighbor)| NeighborRow {
            neighbor: index,
            gateway: neighborhood.gateway(neighbor),
            shared: neighbor.address.is_ipv4()
                && neighbor.mac.is_some_and(|mac| {
                    sharing.get(&(neighbor.interface.as_str(), mac)).is_some_and(|count| *count > 1)
                }),
            changed: watch.change(neighbor),
        })
        .collect()
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sweep {
    pub interface: String,
    pub network: Ipv4Addr,
    pub prefix: u8,
    pub addresses: Vec<Ipv4Addr>,
}

impl Sweep {
    pub fn subnet(&self) -> String {
        format!("{}/{}", self.network, self.prefix)
    }

    pub fn of(interface: &Interface) -> Option<Self> {
        if !interface.up || !SWEPT_KINDS.contains(&interface.kind) {
            return None;
        }
        let (own, prefix) = interface.addresses.iter().find_map(|(address, prefix)| match address {
            IpAddr::V4(v4) if !v4.is_loopback() && !v4.is_link_local() && *prefix < POINT_TO_POINT_PREFIX => {
                Some((*v4, *prefix))
            }
            _ => None,
        })?;
        let prefix = prefix.max(SWEEP_PREFIX);
        let mask = mask(prefix);
        let network = u32::from(own) & mask;
        let broadcast = network | !mask;
        let addresses = (network + 1..broadcast).map(Ipv4Addr::from).filter(|address| *address != own).collect();
        Some(Self { interface: interface.name.clone(), network: Ipv4Addr::from(network), prefix, addresses })
    }
}

#[cfg(test)]
mod tests;
