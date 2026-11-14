//! The networks this machine reaches beyond its own subnets: the routes the system's table lists,
//! the networks a discovery found behind the gateway, and each interface's way to the internet.

use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv4Addr},
};

use serde::{Deserialize, Serialize};

use super::{Interface, InterfaceKind, Neighborhood, POINT_TO_POINT_PREFIX, SCAN_PREFIX, mask};

/// Routes wider than this cover most of the address space, the way VPNs' halves of it do, and
/// count as a way to the internet.
pub const WIDEST_NETWORK: u8 = 8;
/// Routes narrower than this lead to a host or a pair of them, not to a network.
const NARROWEST_NETWORK: u8 = 30;
/// The farthest discovery traces the way to a network that answered.
pub const MAX_HOPS: u8 = 8;
/// The block of /24 networks around this machine's own that discovery tries.
const AROUND_PREFIX: u8 = 16;
/// The /24 networks of a routed network that discovery tries at most.
const NETWORKS_PER_ROUTE: usize = 256;
/// The /24 networks one discovery tries at most.
const MOST_NETWORKS: usize = 512;
/// The host numbers routers take most often: the first and the last of a /24.
pub const ROUTER_HOSTS: [u8; 2] = [1, 254];
/// Networks home and office routers hand out out of the box.
const COMMON_NETWORKS: [Ipv4Addr; 7] = [
    Ipv4Addr::new(10, 0, 0, 0),
    Ipv4Addr::new(10, 0, 1, 0),
    Ipv4Addr::new(10, 1, 1, 0),
    Ipv4Addr::new(10, 10, 10, 0),
    Ipv4Addr::new(172, 16, 0, 0),
    Ipv4Addr::new(192, 168, 0, 0),
    Ipv4Addr::new(192, 168, 1, 0),
];
/// The first octet from which addresses are multicast or reserved.
const MULTICAST_FROM: u8 = 224;
/// The first octet of "this network", which no route leads to.
const THIS_NETWORK: u8 = 0;
const LAN_KINDS: [InterfaceKind; 3] = [InterfaceKind::Ethernet, InterfaceKind::WiFi, InterfaceKind::Bridge];

/// A route the system's IPv4 table lists, other than a default one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub network: Ipv4Addr,
    pub prefix: u8,
    /// The router it goes through; `None` for a route straight onto the interface's link.
    pub gateway: Option<Ipv4Addr>,
    pub interface: String,
}

/// How Borg came to know a network beyond the attached ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Table,
    Discovery,
}

/// What probes with a short time to live told about the way to an address: how many hops away it
/// answered, and the routers on the way that said they dropped the shorter ones.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trace {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hops: Option<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<Ipv4Addr>,
}

/// A network this machine reaches through a router or a tunnel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Routed {
    pub network: Ipv4Addr,
    pub prefix: u8,
    pub interface: String,
    /// The router the route goes through; `None` for one straight into a tunnel.
    pub gateway: Option<Ipv4Addr>,
    pub trace: Trace,
    pub origin: Origin,
    /// When a host behind it was last seen, for a network that only Borg's memory holds now, in
    /// seconds since the epoch.
    pub remembered: Option<u64>,
}

impl Routed {
    pub fn contains(&self, address: Ipv4Addr) -> bool {
        inside(address, self.network, self.prefix)
    }

    pub fn cidr(&self) -> String {
        format!("{}/{}", self.network, self.prefix)
    }

    /// Whether the gateway itself owns the address that answered: it did at the first hop.
    pub fn on_gateway(&self) -> bool {
        self.trace.hops == Some(1)
    }

    /// The routers on the way, from the gateway on.
    pub fn path(&self) -> Vec<Ipv4Addr> {
        let mut path: Vec<Ipv4Addr> = self.gateway.into_iter().collect();
        path.extend(self.trace.path.iter().copied().filter(|router| Some(*router) != self.gateway));
        path
    }

    /// The /24 a scan of this network covers: the one holding its gateway's next router, or its
    /// first.
    pub fn scanned_part(&self) -> (Ipv4Addr, u8) {
        if self.prefix >= SCAN_PREFIX {
            return (self.network, self.prefix);
        }
        let inner = self.trace.path.iter().copied().find(|router| self.contains(*router));
        let base = inner.map_or(self.network, |router| network_part(router, SCAN_PREFIX));
        (base, SCAN_PREFIX)
    }

    fn from_table(route: &Route) -> Self {
        Self {
            network: route.network,
            prefix: route.prefix,
            interface: route.interface.clone(),
            gateway: route.gateway,
            trace: Trace::default(),
            origin: Origin::Table,
            remembered: None,
        }
    }
}

/// One interface's way to the internet: its default gateways and the wide routes that take most
/// addresses, and whether internet traffic leaves through it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternetRoute {
    pub interface: String,
    pub gateways: Vec<IpAddr>,
    pub wide: Vec<Route>,
    pub in_use: bool,
}

impl InternetRoute {
    /// The IPv4 router internet traffic through this interface goes to first.
    pub fn gateway(&self) -> Option<Ipv4Addr> {
        let wide = self.wide.iter().filter_map(|route| route.gateway);
        let default = self.gateways.iter().filter_map(|gateway| match gateway {
            IpAddr::V4(v4) => Some(*v4),
            IpAddr::V6(_) => None,
        });
        wide.chain(default).next()
    }
}

/// Whether `address` lies in the network `network/prefix`.
pub fn inside(address: Ipv4Addr, network: Ipv4Addr, prefix: u8) -> bool {
    u32::from(address) & mask(prefix) == u32::from(network)
}

pub fn network_part(address: Ipv4Addr, prefix: u8) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(address) & mask(prefix))
}

/// Whether one network holds the other, or they are the same.
fn overlap((a, x): (Ipv4Addr, u8), (b, y): (Ipv4Addr, u8)) -> bool {
    let prefix = x.min(y);
    network_part(a, prefix) == network_part(b, prefix)
}

/// The IPv4 subnets this machine has an address on.
pub fn attached(interfaces: &[Interface]) -> Vec<(Ipv4Addr, u8)> {
    interfaces
        .iter()
        .filter(|interface| interface.up)
        .flat_map(|interface| &interface.addresses)
        .filter_map(|(address, prefix)| match address {
            IpAddr::V4(v4) if !v4.is_loopback() && *prefix < POINT_TO_POINT_PREFIX => {
                Some((network_part(*v4, *prefix), *prefix))
            }
            _ => None,
        })
        .collect()
}

/// Whether an interface belongs on the map without any host or route: it is up, it is not the
/// loopback, and it has an address beyond the link it is on.
pub fn shows(interface: &Interface) -> bool {
    interface.up
        && interface.kind != InterfaceKind::Loopback
        && interface.addresses.iter().any(|(address, _)| match address {
            IpAddr::V4(v4) => !v4.is_link_local() && !v4.is_loopback(),
            IpAddr::V6(v6) => !v6.is_unicast_link_local() && !v6.is_loopback(),
        })
}

fn special(network: Ipv4Addr) -> bool {
    let first = network.octets()[0];
    network.is_loopback() || network.is_link_local() || first >= MULTICAST_FROM || first == THIS_NETWORK
}

/// Whether a table route leads to a network the map shows: neither a host nor most of the address
/// space, not a special range, not inside this machine's own subnets, through an interface that is
/// up and not the loopback.
fn leads_somewhere(route: &Route, interfaces: &[Interface], own: &[(Ipv4Addr, u8)]) -> bool {
    let up = interfaces.iter().any(|interface| {
        interface.name == route.interface && interface.up && interface.kind != InterfaceKind::Loopback
    });
    (WIDEST_NETWORK..=NARROWEST_NETWORK).contains(&route.prefix)
        && up
        && !special(route.network)
        && !own.iter().any(|&(network, prefix)| route.prefix >= prefix && inside(route.network, network, prefix))
}

/// The networks the table's routes reach beyond the attached ones.
pub fn table_networks(routes: &[Route], interfaces: &[Interface]) -> Vec<Routed> {
    let own = attached(interfaces);
    routes.iter().filter(|route| leads_somewhere(route, interfaces, &own)).map(Routed::from_table).collect()
}

/// Each interface's way to the internet, with the one traffic takes now marked: the most specific
/// wide route if there is one, else the default the system lists first.
pub fn internet(neighborhood: &Neighborhood, interfaces: &[Interface]) -> Vec<InternetRoute> {
    let wide: Vec<&Route> =
        neighborhood.routes.iter().filter(|route| route.prefix < WIDEST_NETWORK && route.prefix > 0).collect();
    let in_use = wide
        .iter()
        .max_by_key(|route| route.prefix)
        .map(|route| route.interface.as_str())
        .or_else(|| neighborhood.first_gateway().map(|gateway| gateway.interface.as_str()));
    let names: Vec<&str> = neighborhood
        .gateways
        .iter()
        .map(|gateway| gateway.interface.as_str())
        .chain(wide.iter().map(|route| route.interface.as_str()))
        .collect();
    let mut seen = BTreeSet::new();
    names
        .into_iter()
        .filter(|name| seen.insert(*name))
        .filter(|name| interfaces.iter().any(|interface| interface.name == *name && shows(interface)))
        .map(|name| InternetRoute {
            interface: name.to_owned(),
            gateways: neighborhood.gateways_on(name).map(|gateway| gateway.address).collect(),
            wide: wide.iter().filter(|route| route.interface == name).map(|route| (*route).clone()).collect(),
            in_use: in_use == Some(name),
        })
        .collect()
}

/// Every routed network the map shows: the table's, the ones a discovery found that no table
/// route covers, and the ones only memory holds, ordered by address.
pub fn routed(table: Vec<Routed>, reached: &[Routed], remembered: Vec<Routed>) -> Vec<Routed> {
    let mut routed = table;
    for found in reached.iter().cloned().chain(remembered) {
        let same = routed.iter().position(|known| (known.network, known.prefix) == (found.network, found.prefix));
        let covered = routed.iter().any(|known| known.prefix < found.prefix && known.contains(found.network));
        match same {
            Some(index) if routed[index].trace.hops.is_none() => routed[index].trace = found.trace,
            Some(_) => {}
            None if covered => {}
            None => routed.push(found),
        }
    }
    routed.sort_by_key(|network| (network.network, network.prefix));
    routed
}

/// The narrowest routed network holding `address`.
pub fn holding(routed: &[Routed], address: Ipv4Addr) -> Option<usize> {
    routed
        .iter()
        .enumerate()
        .filter(|(_, network)| network.contains(address))
        .max_by_key(|(_, network)| network.prefix)
        .map(|(index, _)| index)
}

/// Where a discovery's probes leave this machine: an interface and the router on it, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Way {
    pub interface: String,
    pub gateway: Option<Ipv4Addr>,
}

/// What a discovery probes: the likely routers of each /24 it guesses, all sent the way internet
/// traffic leaves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    pub way: Way,
    pub networks: usize,
    pub addresses: Vec<Ipv4Addr>,
}

fn twenty_fours(network: Ipv4Addr, prefix: u8) -> impl Iterator<Item = u32> {
    let base = u32::from(network_part(network, prefix.min(SCAN_PREFIX)));
    let count = 1_u32 << (SCAN_PREFIX.saturating_sub(prefix.min(SCAN_PREFIX)));
    (0..count).map(move |index| base + (index << (u32::from(super::IPV4_BITS - SCAN_PREFIX))))
}

/// The networks to look for the way internet traffic leaves: every /24 of the /16 around each
/// private network this machine is on, the /24s of the table's routes through that interface and
/// a few common router networks, without the attached ones and those another interface's routes
/// take.
pub fn discovery(interfaces: &[Interface], routes: &[Route], internet: &[InternetRoute]) -> Option<Discovery> {
    let exit = internet.iter().find(|route| route.in_use)?;
    let own = attached(interfaces);
    let (through, elsewhere): (Vec<Routed>, Vec<Routed>) =
        table_networks(routes, interfaces).into_iter().partition(|network| network.interface == exit.interface);
    let lans = interfaces.iter().filter(|interface| interface.up && LAN_KINDS.contains(&interface.kind));
    let around = lans
        .flat_map(|interface| &interface.addresses)
        .filter_map(|(address, prefix)| match address {
            IpAddr::V4(v4) if v4.is_private() && *prefix < POINT_TO_POINT_PREFIX => Some(*v4),
            _ => None,
        })
        .flat_map(|address| twenty_fours(address, AROUND_PREFIX));
    let routed =
        through.iter().flat_map(|network| twenty_fours(network.network, network.prefix).take(NETWORKS_PER_ROUTE));
    let common = COMMON_NETWORKS.iter().map(|network| u32::from(*network));
    let mut chosen = BTreeSet::new();
    let networks: Vec<Ipv4Addr> = around
        .chain(routed)
        .chain(common)
        .map(Ipv4Addr::from)
        .filter(|network| !own.iter().any(|attached| overlap(*attached, (*network, SCAN_PREFIX))))
        .filter(|network| {
            !elsewhere.iter().any(|route| overlap((route.network, route.prefix), (*network, SCAN_PREFIX)))
        })
        .filter(|network| chosen.insert(*network))
        .take(MOST_NETWORKS)
        .collect();
    let addresses = networks
        .iter()
        .flat_map(|network| ROUTER_HOSTS.map(|host| Ipv4Addr::from(u32::from(*network) | u32::from(host))))
        .collect();
    let way = Way { interface: exit.interface.clone(), gateway: exit.gateway() };
    Some(Discovery { way, networks: networks.len(), addresses })
}

#[cfg(test)]
mod tests;
