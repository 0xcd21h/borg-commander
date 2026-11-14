use std::net::{IpAddr, Ipv4Addr};

use serde::{Deserialize, Serialize};

use super::{LocalNetwork, Mac, Neighbor, Probe, mask};

pub const NEIGHBORS_FORMAT: u8 = 1;
const MAX_NETWORKS: usize = 32;
const MAX_HOSTS: usize = 1024;
const MAX_PREFIX: u8 = 32;
const UNKNOWN_FORMAT: &str = "unsupported neighbors format";
const TOO_MANY_NETWORKS: &str = "too many remembered networks";
const TOO_MANY_HOSTS: &str = "too many remembered hosts";
const BAD_PREFIX: &str = "a remembered network has an invalid prefix";
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownNetwork {
    pub network: Ipv4Addr,
    pub prefix: u8,
    pub gateway: Option<Mac>,
    pub hosts: Vec<KnownHost>,
}

impl KnownNetwork {
    fn at(&self, place: &LocalNetwork) -> bool {
        self.network == place.network && self.prefix == place.prefix && self.gateway == place.gateway
    }

    fn last_seen(&self) -> u64 {
        self.hosts.iter().map(|host| host.seen).max().unwrap_or_default()
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

impl NeighborMemory {
    pub fn saved(&self) -> SavedNeighbors {
        SavedNeighbors { version: NEIGHBORS_FORMAT, networks: self.networks.clone() }
    }

    pub fn restore(saved: SavedNeighbors) -> Result<Self, &'static str> {
        if saved.version != NEIGHBORS_FORMAT {
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
            let inside = |address: Ipv4Addr| u32::from(address) & mask(network.prefix) == u32::from(network.network);
            if !network.hosts.iter().all(|host| inside(host.address)) {
                return Err(OUTSIDE);
            }
        }
        Ok(Self { networks: saved.networks })
    }

    pub fn hosts(&self, place: &LocalNetwork) -> &[KnownHost] {
        self.networks.iter().find(|network| network.at(place)).map_or(&[], |network| &network.hosts)
    }

    pub fn host(&self, places: &[LocalNetwork], address: IpAddr) -> Option<&KnownHost> {
        let IpAddr::V4(address) = address else { return None };
        let place = places.iter().find(|place| place.contains(address))?;
        self.hosts(place).iter().find(|host| host.address == address)
    }

    fn network(&mut self, place: &LocalNetwork) -> &mut KnownNetwork {
        let found = self.networks.iter().position(|network| network.at(place));
        let index = found.unwrap_or_else(|| {
            self.networks.push(KnownNetwork {
                network: place.network,
                prefix: place.prefix,
                gateway: place.gateway,
                hosts: Vec::new(),
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

    /// Notes a scanned host that answered, with its open ports and whether it answered ping.
    pub(crate) fn note_probe(&mut self, places: &[LocalNetwork], probe: &Probe, now: u64) -> Noted {
        let Some(place) = places.iter().find(|place| place.holds(probe.address)) else { return Noted::default() };
        if !probe.answered() {
            return Noted::default();
        }
        let network = self.network(place);
        let (host, new) = network.host(probe.address, now);
        let learned = new || host.ports != probe.open || host.ping != probe.ping.is_some();
        host.seen = now;
        host.scanned = Some(now);
        host.ports.clone_from(&probe.open);
        host.ping = probe.ping.is_some();
        network.trim();
        self.trim();
        Noted { learned, refreshed: true }
    }

    pub(crate) fn forget(&mut self, place: &LocalNetwork, address: Ipv4Addr) -> bool {
        let Some(network) = self.networks.iter_mut().find(|network| network.at(place)) else { return false };
        let before = network.hosts.len();
        network.hosts.retain(|host| host.address != address);
        let forgot = network.hosts.len() != before;
        self.networks.retain(|network| !network.hosts.is_empty());
        forgot
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
