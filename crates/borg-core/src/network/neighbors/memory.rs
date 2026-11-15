use std::net::{IpAddr, Ipv4Addr};

use serde::{Deserialize, Serialize};

use super::{
    LocalNetwork, MAX_HOPS, Mac, Neighbor, Origin, Probe, Routed, Trace, WIDEST_NETWORK, holding, inside, mask,
};

pub const NEIGHBORS_FORMAT: u8 = 2;
/// The oldest format Borg still reads: the one before routes were remembered.
const OLDEST_FORMAT: u8 = 1;
const MAX_NETWORKS: usize = 32;
/// Hosts a network keeps, those behind its routes included.
const MAX_HOSTS: usize = 1024;
const MAX_ROUTES: usize = 16;
const MAX_PREFIX: u8 = 32;
const UNKNOWN_FORMAT: &str = "unsupported neighbors format";
const TOO_MANY_NETWORKS: &str = "too many remembered networks";
const TOO_MANY_HOSTS: &str = "too many remembered hosts";
const TOO_MANY_ROUTES: &str = "too many remembered routes";
const BAD_PREFIX: &str = "a remembered network has an invalid prefix";
const BAD_ROUTE: &str = "a remembered route is invalid";
const OUTSIDE: &str = "a remembered host lies outside its network";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownHost {
    pub address: Ipv4Addr,
    pub mac: Option<Mac>,
    pub seen: u64,
    #[serde(default)]
    pub scanned: Option<u64>,
    #[serde(default)]
    pub ports: Vec<u16>,
    #[serde(default)]
    pub ping: bool,
}

/// A network Borg reached from a remembered one through its gateway, and when it last found
/// something there, in seconds since the epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownRoute {
    pub network: Ipv4Addr,
    pub prefix: u8,
    #[serde(default)]
    pub gateway: Option<Ipv4Addr>,
    #[serde(default)]
    pub trace: Trace,
    pub origin: Origin,
    pub seen: u64,
}

impl KnownRoute {
    fn contains(&self, address: Ipv4Addr) -> bool {
        inside(address, self.network, self.prefix)
    }

    fn valid(&self) -> bool {
        (WIDEST_NETWORK..=MAX_PREFIX).contains(&self.prefix)
            && u32::from(self.network) & !mask(self.prefix) == 0
            && self.trace.path.len() <= usize::from(MAX_HOPS)
            && self.trace.hops.is_none_or(|hops| hops <= MAX_HOPS)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownNetwork {
    pub network: Ipv4Addr,
    pub prefix: u8,
    pub gateway: Option<Mac>,
    pub hosts: Vec<KnownHost>,
    #[serde(default)]
    pub routes: Vec<KnownRoute>,
}

impl KnownNetwork {
    fn at(&self, place: &LocalNetwork) -> bool {
        self.network == place.network && self.prefix == place.prefix && self.gateway == place.gateway
    }

    fn last_seen(&self) -> u64 {
        let hosts = self.hosts.iter().map(|host| host.seen);
        hosts.chain(self.routes.iter().map(|route| route.seen)).max().unwrap_or_default()
    }

    /// Whether `address` is on the network itself or behind one of its routes.
    fn holds(&self, address: Ipv4Addr) -> bool {
        inside(address, self.network, self.prefix) || self.routes.iter().any(|route| route.contains(address))
    }

    /// Notes a routed network, and whether that taught Borg something new.
    fn route(&mut self, routed: &Routed, now: u64) -> bool {
        let found =
            self.routes.iter().position(|route| (route.network, route.prefix) == (routed.network, routed.prefix));
        let Some(index) = found else {
            self.routes.push(KnownRoute {
                network: routed.network,
                prefix: routed.prefix,
                gateway: routed.gateway,
                trace: routed.trace.clone(),
                origin: routed.origin,
                seen: now,
            });
            return true;
        };
        let route = &mut self.routes[index];
        route.seen = now;
        let traced = routed.trace.hops.is_some() && route.trace != routed.trace;
        if traced {
            route.trace = routed.trace.clone();
        }
        traced
    }

    fn host(&mut self, address: Ipv4Addr, now: u64) -> (&mut KnownHost, bool) {
        let found = self.hosts.iter().position(|host| host.address == address);
        let index = found.unwrap_or_else(|| {
            self.hosts.push(KnownHost { address, mac: None, seen: now, scanned: None, ports: Vec::new(), ping: false });
            self.hosts.len() - 1
        });
        (&mut self.hosts[index], found.is_none())
    }

    fn trim(&mut self) {
        if self.routes.len() > MAX_ROUTES {
            self.routes.sort_by_key(|route| std::cmp::Reverse(route.seen));
            self.routes.truncate(MAX_ROUTES);
        }
        self.routes.sort_by_key(|route| (route.network, route.prefix));
        let (network, prefix, routes) = (self.network, self.prefix, &self.routes);
        self.hosts.retain(|host| {
            inside(host.address, network, prefix) || routes.iter().any(|route| route.contains(host.address))
        });
        if self.hosts.len() > MAX_HOSTS {
            self.hosts.sort_by_key(|host| std::cmp::Reverse(host.seen));
            self.hosts.truncate(MAX_HOSTS);
        }
        self.hosts.sort_by_key(|host| host.address);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedNeighbors {
    pub version: u8,
    pub networks: Vec<KnownNetwork>,
}

/// What Borg found on the networks this machine joined, by network, so a host that is gone
/// still shows with the time it was last seen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NeighborMemory {
    networks: Vec<KnownNetwork>,
}

/// Whether noting what was found taught Borg something new or only refreshed the times.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Noted {
    pub learned: bool,
    pub refreshed: bool,
}

impl Noted {
    pub fn with(self, other: Self) -> Self {
        Self { learned: self.learned || other.learned, refreshed: self.refreshed || other.refreshed }
    }
}

impl NeighborMemory {
    pub fn saved(&self) -> SavedNeighbors {
        SavedNeighbors { version: NEIGHBORS_FORMAT, networks: self.networks.clone() }
    }

    pub fn restore(saved: SavedNeighbors) -> Result<Self, &'static str> {
        if !(OLDEST_FORMAT..=NEIGHBORS_FORMAT).contains(&saved.version) {
            return Err(UNKNOWN_FORMAT);
        }
        if saved.networks.len() > MAX_NETWORKS {
            return Err(TOO_MANY_NETWORKS);
        }
        for network in &saved.networks {
            if network.prefix > MAX_PREFIX {
                return Err(BAD_PREFIX);
            }
            if network.hosts.len() > MAX_HOSTS {
                return Err(TOO_MANY_HOSTS);
            }
            if network.routes.len() > MAX_ROUTES {
                return Err(TOO_MANY_ROUTES);
            }
            if !network.routes.iter().all(KnownRoute::valid) {
                return Err(BAD_ROUTE);
            }
            if !network.hosts.iter().all(|host| network.holds(host.address)) {
                return Err(OUTSIDE);
            }
        }
        Ok(Self { networks: saved.networks })
    }

    fn known(&self, place: &LocalNetwork) -> Option<&KnownNetwork> {
        self.networks.iter().find(|network| network.at(place))
    }

    pub fn hosts(&self, place: &LocalNetwork) -> &[KnownHost] {
        self.known(place).map_or(&[], |network| &network.hosts)
    }

    pub fn routes(&self, place: &LocalNetwork) -> &[KnownRoute] {
        self.known(place).map_or(&[], |network| &network.routes)
    }

    pub fn host(&self, places: &[LocalNetwork], address: IpAddr) -> Option<&KnownHost> {
        let IpAddr::V4(address) = address else { return None };
        places.iter().find_map(|place| self.hosts(place).iter().find(|host| host.address == address))
    }

    /// The networks remembered behind the places this machine is on, as the map shows them.
    pub fn remembered(&self, places: &[LocalNetwork]) -> Vec<Routed> {
        places
            .iter()
            .flat_map(|place| {
                self.routes(place).iter().map(|route| Routed {
                    network: route.network,
                    prefix: route.prefix,
                    interface: place.interface.clone(),
                    gateway: route.gateway,
                    trace: route.trace.clone(),
                    origin: route.origin,
                    remembered: Some(route.seen),
                })
            })
            .collect()
    }

    fn network(&mut self, place: &LocalNetwork) -> &mut KnownNetwork {
        let found = self.networks.iter().position(|network| network.at(place));
        let index = found.unwrap_or_else(|| {
            self.networks.push(KnownNetwork {
                network: place.network,
                prefix: place.prefix,
                gateway: place.gateway,
                hosts: Vec::new(),
                routes: Vec::new(),
            });
            self.networks.len() - 1
        });
        &mut self.networks[index]
    }

    /// Notes the hosts the neighbor tables list with a MAC address, at `now` in seconds since
    /// the epoch.
    pub(crate) fn note_listed(&mut self, places: &[LocalNetwork], neighbors: &[Neighbor], now: u64) -> Noted {
        let mut noted = Noted::default();
        for place in places {
            let listed: Vec<(Ipv4Addr, Mac)> = neighbors
                .iter()
                .filter(|neighbor| neighbor.interface == place.interface)
                .filter_map(|neighbor| match (neighbor.address, neighbor.mac) {
                    (IpAddr::V4(address), Some(mac)) if place.holds(address) && !mac.group() => Some((address, mac)),
                    _ => None,
                })
                .collect();
            if listed.is_empty() {
                continue;
            }
            let network = self.network(place);
            for (address, mac) in listed {
                let (host, new) = network.host(address, now);
                noted.learned |= new || host.mac != Some(mac);
                noted.refreshed |= host.seen != now;
                host.mac = Some(mac);
                host.seen = now;
            }
            network.trim();
        }
        self.trim();
        noted
    }

    /// Notes a scanned host that answered, with its open ports and whether it answered ping:
    /// on a place's own network, or behind a routed network that leaves through a place's
    /// interface.
    pub(crate) fn note_probe(&mut self, places: &[LocalNetwork], routed: &[Routed], probe: &Probe, now: u64) -> Noted {
        if !probe.answered() {
            return Noted::default();
        }
        let own = places.iter().find(|place| place.holds(probe.address));
        let behind = || {
            let routed = &routed[holding(routed, probe.address)?];
            Some((places.iter().find(|place| place.interface == routed.interface)?, routed))
        };
        let (place, route) = match own {
            Some(place) => (place, None),
            None => match behind() {
                Some((place, routed)) => (place, Some(routed)),
                None => return Noted::default(),
            },
        };
        let network = self.network(place);
        let routed = route.is_some_and(|route| network.route(route, now));
        let (host, new) = network.host(probe.address, now);
        let learned = routed || new || host.ports != probe.open || host.ping != probe.ping.is_some();
        host.seen = now;
        host.scanned = Some(now);
        host.ports.clone_from(&probe.open);
        host.ping = probe.ping.is_some();
        network.trim();
        self.trim();
        Noted { learned, refreshed: true }
    }

    /// Notes a network a discovery found behind a place's gateway, and the address that answered
    /// there.
    pub(crate) fn note_reached(&mut self, place: &LocalNetwork, routed: &Routed, probe: &Probe, now: u64) -> Noted {
        if !probe.answered() {
            return Noted::default();
        }
        let network = self.network(place);
        let routed = network.route(routed, now);
        let (host, new) = network.host(probe.address, now);
        let learned = routed || new || (probe.ping.is_some() && !host.ping);
        host.seen = now;
        host.ping |= probe.ping.is_some();
        network.trim();
        self.trim();
        Noted { learned, refreshed: true }
    }

    pub(crate) fn forget(&mut self, place: &LocalNetwork, address: Ipv4Addr) -> bool {
        let Some(network) = self.networks.iter_mut().find(|network| network.at(place)) else { return false };
        let before = network.hosts.len();
        network.hosts.retain(|host| host.address != address);
        let forgot = network.hosts.len() != before;
        self.drop_empty();
        forgot
    }

    /// Forgets a routed network and the hosts behind it.
    pub(crate) fn forget_route(&mut self, place: &LocalNetwork, network: Ipv4Addr, prefix: u8) -> bool {
        let Some(known) = self.networks.iter_mut().find(|known| known.at(place)) else { return false };
        let before = known.routes.len();
        known.routes.retain(|route| (route.network, route.prefix) != (network, prefix));
        let forgot = known.routes.len() != before;
        known.trim();
        self.drop_empty();
        forgot
    }

    fn drop_empty(&mut self) {
        self.networks.retain(|network| !network.hosts.is_empty() || !network.routes.is_empty());
    }

    fn trim(&mut self) {
        if self.networks.len() > MAX_NETWORKS {
            self.networks.sort_by_key(|network| std::cmp::Reverse(network.last_seen()));
            self.networks.truncate(MAX_NETWORKS);
        }
    }
}

#[cfg(test)]
mod tests;
