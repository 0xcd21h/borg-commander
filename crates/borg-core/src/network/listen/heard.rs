//! What listening heard this session: each host with what it announced of itself, and the switch
//! port each interface is plugged into.

use super::packets::{Announcement, Heard, LONGEST_TEXT, ListenProtocol, SwitchPort};
use crate::network::{Interface, Mac, Neighbor, neighbors::mask};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr},
};

/// Device types, and services, Borg keeps of one host.
pub const MOST_KINDS: usize = 16;
/// Hosts one session keeps.
const MOST_HOSTS: usize = 1024;
/// Hardware addresses heard without an IP address that one session keeps.
const MOST_UNPLACED: usize = 256;
const IPV6_BITS: u32 = 128;
/// Prefixes this long or longer have no network or broadcast address.
const POINT_TO_POINT_PREFIX: u8 = 31;

/// Something a host said, and the protocol it said it in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Said {
    pub protocol: ListenProtocol,
    pub text: String,
}

impl Said {
    fn valid(&self) -> bool {
        !self.text.is_empty() && self.text.chars().count() <= LONGEST_TEXT
    }
}

/// What a host announced of itself, gathered over every packet heard from it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Announced {
    /// One name for each protocol that gave one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<Said>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Said>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<String>,
    /// The vendor class its DHCP client sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    /// Every protocol it was heard in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protocols: Vec<ListenProtocol>,
}

impl Announced {
    pub fn is_empty(&self) -> bool {
        self.protocols.is_empty()
            && self.names.is_empty()
            && self.model.is_none()
            && self.kinds.is_empty()
            && self.services.is_empty()
            && self.client.is_none()
    }

    /// The name to show: the one from the most trusted protocol.
    pub fn name(&self) -> Option<&Said> {
        self.names.iter().min_by_key(|name| name.protocol)
    }

    fn heard_in(&mut self, protocol: ListenProtocol) {
        if let Err(at) = self.protocols.binary_search(&protocol) {
            self.protocols.insert(at, protocol);
        }
    }

    fn name_from(&mut self, said: Said) {
        match self.names.iter_mut().find(|name| name.protocol == said.protocol) {
            Some(name) => *name = said,
            None => {
                self.names.push(said);
                self.names.sort_by_key(|name| name.protocol);
            }
        }
    }

    /// A model replaces one from a protocol trusted less, or the same.
    fn model_from(&mut self, said: Said) {
        if self.model.as_ref().is_none_or(|model| said.protocol <= model.protocol) {
            self.model = Some(said);
        }
    }

    fn add(list: &mut Vec<String>, found: &[String]) {
        for item in found {
            if list.len() < MOST_KINDS && !list.contains(item) {
                list.push(item.clone());
            }
        }
    }

    /// Takes in what one packet announced, and whether that told Borg something new.
    pub fn absorb(&mut self, announcement: &Announcement) -> bool {
        let before = self.clone();
        let protocol = announcement.protocol;
        self.heard_in(protocol);
        if let Some(name) = &announcement.name {
            self.name_from(Said { protocol, text: name.clone() });
        }
        if let Some(model) = &announcement.model {
            self.model_from(Said { protocol, text: model.clone() });
        }
        Self::add(&mut self.kinds, &announcement.kinds);
        Self::add(&mut self.services, &announcement.services);
        if announcement.client.is_some() {
            self.client.clone_from(&announcement.client);
        }
        *self != before
    }

    /// Takes in what another record of the same host holds, which is newer, and whether that
    /// told Borg something new.
    pub fn merge(&mut self, other: &Announced) -> bool {
        let before = self.clone();
        for protocol in &other.protocols {
            self.heard_in(*protocol);
        }
        for name in &other.names {
            self.name_from(name.clone());
        }
        if let Some(model) = &other.model {
            self.model_from(model.clone());
        }
        Self::add(&mut self.kinds, &other.kinds);
        Self::add(&mut self.services, &other.services);
        if other.client.is_some() {
            self.client.clone_from(&other.client);
        }
        *self != before
    }

    /// Whether a remembered record keeps to what listening would have written.
    pub(crate) fn valid(&self) -> bool {
        let short = |text: &String| !text.is_empty() && text.chars().count() <= LONGEST_TEXT;
        let one_each = self.names.iter().map(|name| name.protocol).collect::<HashSet<_>>().len() == self.names.len();
        one_each
            && self.names.iter().all(Said::valid)
            && self.model.as_ref().is_none_or(Said::valid)
            && self.kinds.len() <= MOST_KINDS
            && self.services.len() <= MOST_KINDS
            && self.kinds.iter().chain(&self.services).chain(&self.client).all(short)
            && self.protocols.len() <= ListenProtocol::ALL.len()
    }
}

/// A host listening heard, on the interface it was heard on, first and last at times in
/// seconds since the epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeardHost {
    pub address: IpAddr,
    pub mac: Option<Mac>,
    pub interface: String,
    pub router: bool,
    pub announced: Announced,
    pub first: u64,
    pub last: u64,
}

/// The switch port an interface is plugged into, as LLDP or CDP last named it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeardSwitch {
    pub interface: String,
    pub port: SwitchPort,
    pub at: u64,
}

/// What a host announced with only its hardware address, as a DHCP client does before it has
/// an IP address.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Unplaced {
    interface: String,
    mac: Mac,
    announced: Announced,
    at: u64,
}

/// Everything listening heard this session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hearing {
    pub hosts: Vec<HeardHost>,
    pub switches: Vec<HeardSwitch>,
    unplaced: Vec<Unplaced>,
}

/// Which addresses a frame heard on one interface may name: another host's on the interface's
/// networks, or a link-local one.
struct Scope {
    own: HashSet<IpAddr>,
    macs: Vec<Mac>,
    networks: Vec<(IpAddr, u8)>,
}

impl Scope {
    fn of(interface: &str, interfaces: &[Interface]) -> Self {
        let own = interfaces.iter().flat_map(|interface| interface.addresses.iter().map(|(address, _)| *address));
        let macs = interfaces.iter().filter_map(|interface| interface.mac.as_deref().and_then(Mac::parse));
        let networks = interfaces.iter().filter(|known| known.name == interface).flat_map(|known| &known.addresses);
        Self { own: own.collect(), macs: macs.collect(), networks: networks.copied().collect() }
    }

    fn accepts(&self, address: IpAddr) -> bool {
        if self.own.contains(&address) || !ordinary(address) {
            return false;
        }
        match address {
            IpAddr::V4(address) => address.is_link_local() || self.networks.iter().any(|own| holds_v4(*own, address)),
            IpAddr::V6(address) => {
                address.is_unicast_link_local()
                    || self.networks.iter().any(|(own, prefix)| match own {
                        IpAddr::V6(own) => {
                            let mask = u128::MAX.checked_shl(IPV6_BITS.saturating_sub(u32::from(*prefix))).unwrap_or(0);
                            u128::from(*own) & mask == u128::from(address) & mask
                        }
                        IpAddr::V4(_) => false,
                    })
            }
        }
    }
}

/// Whether `address` is another host's on the IPv4 network `own` sits on.
fn holds_v4((own, prefix): (IpAddr, u8), address: Ipv4Addr) -> bool {
    let IpAddr::V4(own) = own else { return false };
    let mask = mask(prefix);
    let network = u32::from(own) & mask;
    let inside = u32::from(address) & mask == network;
    let edge =
        prefix < POINT_TO_POINT_PREFIX && (u32::from(address) == network || u32::from(address) == network | !mask);
    inside && !edge
}

/// Whether `address` can be one host's.
fn ordinary(address: IpAddr) -> bool {
    let broadcast = matches!(address, IpAddr::V4(address) if address.is_broadcast());
    !(address.is_unspecified() || address.is_multicast() || address.is_loopback() || broadcast)
}

impl Hearing {
    pub fn host(&self, address: IpAddr) -> Option<&HeardHost> {
        self.hosts.iter().find(|host| host.address == address)
    }

    /// The switch port `interface` is plugged into, from LLDP before CDP.
    pub fn switch(&self, interface: &str) -> Option<&HeardSwitch> {
        self.switches.iter().filter(|switch| switch.interface == interface).min_by_key(|switch| switch.port.protocol)
    }

    pub(crate) fn forget(&mut self, address: IpAddr) -> bool {
        let before = self.hosts.len();
        self.hosts.retain(|host| host.address != address);
        self.hosts.len() != before
    }

    /// Takes in what frames heard on `interface` announced, at `at` in seconds since the epoch,
    /// and returns the addresses of the hosts they told Borg about. Announcements about this
    /// machine, and addresses off the interface's networks, are left out.
    pub(crate) fn take(
        &mut self,
        interface: &str,
        heard: Vec<Heard>,
        interfaces: &[Interface],
        at: u64,
    ) -> Vec<IpAddr> {
        let scope = Scope::of(interface, interfaces);
        let mut taken = Vec::new();
        for heard in heard {
            match heard {
                Heard::Switch(port) => self.note_switch(interface, port, at),
                Heard::Host(mut announcement) => {
                    if announcement.mac.is_some_and(|mac| scope.macs.contains(&mac)) {
                        continue;
                    }
                    announcement.mac = announcement.mac.filter(|mac| !mac.group() && !mac.unset());
                    match (announcement.address, announcement.mac) {
                        (Some(address), _) if scope.accepts(address) => {
                            if self.note_host(interface, address, &announcement, at) {
                                taken.push(address);
                            }
                        }
                        (None, Some(mac)) => taken.extend(self.note_unplaced(interface, mac, &announcement, at)),
                        _ => {}
                    }
                }
            }
        }
        taken.sort_unstable();
        taken.dedup();
        taken
    }

    fn note_switch(&mut self, interface: &str, port: SwitchPort, at: u64) {
        let same = |switch: &&mut HeardSwitch| switch.interface == interface && switch.port.protocol == port.protocol;
        match self.switches.iter_mut().find(same) {
            Some(switch) => {
                switch.port = port;
                switch.at = at;
            }
            None => self.switches.push(HeardSwitch { interface: interface.to_owned(), port, at }),
        }
    }

    /// Notes a host and what it announced, and whether there was room for it.
    fn note_host(&mut self, interface: &str, address: IpAddr, announcement: &Announcement, at: u64) -> bool {
        let Self { hosts, unplaced, .. } = self;
        let index = match hosts.iter().position(|host| host.address == address) {
            Some(index) => index,
            None if hosts.len() < MOST_HOSTS => {
                hosts.push(HeardHost {
                    address,
                    mac: None,
                    interface: interface.to_owned(),
                    router: false,
                    announced: Announced::default(),
                    first: at,
                    last: at,
                });
                hosts.len() - 1
            }
            None => return false,
        };
        let host = &mut hosts[index];
        host.mac = announcement.mac.or(host.mac);
        host.router |= announcement.router;
        host.announced.absorb(announcement);
        host.last = host.last.max(at);
        if let Some(mac) = host.mac {
            for alone in unplaced.iter().filter(|alone| alone.mac == mac && alone.interface == interface) {
                host.announced.merge(&alone.announced);
            }
        }
        true
    }

    /// Notes what a host announced with its hardware address alone, and gives it to the hosts
    /// heard with that address, whose addresses it returns.
    fn note_unplaced(&mut self, interface: &str, mac: Mac, announcement: &Announcement, at: u64) -> Vec<IpAddr> {
        let found = self.unplaced.iter().position(|alone| alone.mac == mac && alone.interface == interface);
        let index = match found {
            Some(index) => Some(index),
            None if self.unplaced.len() < MOST_UNPLACED => {
                let announced = Announced::default();
                self.unplaced.push(Unplaced { interface: interface.to_owned(), mac, announced, at });
                Some(self.unplaced.len() - 1)
            }
            None => None,
        };
        if let Some(index) = index {
            self.unplaced[index].announced.absorb(announcement);
            self.unplaced[index].at = at;
        }
        let mut told = Vec::new();
        for host in self.hosts.iter_mut().filter(|host| host.mac == Some(mac) && host.interface == interface) {
            host.announced.absorb(announcement);
            host.last = host.last.max(at);
            told.push(host.address);
        }
        told
    }

    /// Gives what was heard with a hardware address alone to the neighbors the tables list with
    /// it, and whether that told Borg something new.
    pub(crate) fn place(&mut self, neighbors: &[Neighbor]) -> bool {
        let mut changed = false;
        for alone in &self.unplaced {
            let listed = neighbors.iter().filter(|neighbor| {
                neighbor.mac == Some(alone.mac) && neighbor.interface == alone.interface && ordinary(neighbor.address)
            });
            for neighbor in listed {
                let known = self.hosts.iter().position(|host| host.address == neighbor.address);
                match known {
                    Some(index) => changed |= self.hosts[index].announced.merge(&alone.announced),
                    None if self.hosts.len() < MOST_HOSTS => {
                        self.hosts.push(HeardHost {
                            address: neighbor.address,
                            mac: Some(alone.mac),
                            interface: alone.interface.clone(),
                            router: neighbor.router,
                            announced: alone.announced.clone(),
                            first: alone.at,
                            last: alone.at,
                        });
                        changed = true;
                    }
                    None => {}
                }
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests;
