//! The Neighbors page as a map: each interface this machine has, the hosts on its own network,
//! the networks it reaches through a router or a tunnel with the hosts found there, and its way
//! to the internet.

use std::net::IpAddr;

use super::{Interface, InterfaceKind, InternetRoute, Neighbor, NeighborRow, Routed, holding, inside, shows};

const LAN_KINDS: [InterfaceKind; 3] = [InterfaceKind::Ethernet, InterfaceKind::WiFi, InterfaceKind::Bridge];

/// A row of the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeighborEntry {
    /// An interface, by its index among the sample's interfaces.
    Interface(usize),
    /// A routed network, by its index among the routed networks.
    Network(usize),
    /// An interface's way to the internet, by its index among the internet routes.
    Internet(usize),
    Host(NeighborRow),
}

impl NeighborEntry {
    pub fn host(self) -> Option<NeighborRow> {
        match self {
            Self::Host(row) => Some(row),
            Self::Interface(_) | Self::Network(_) | Self::Internet(_) => None,
        }
    }
}

/// Where a host sits under its interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Own,
    Network(usize),
    Internet,
}

/// What the map is drawn from.
pub struct MapParts<'a> {
    pub interfaces: &'a [Interface],
    pub routed: &'a [Routed],
    pub internet: &'a [InternetRoute],
    pub neighbors: &'a [Neighbor],
}

impl MapParts<'_> {
    fn on_own_network(&self, neighbor: &Neighbor, address: std::net::Ipv4Addr) -> bool {
        let interface = self.interfaces.iter().find(|interface| interface.name == neighbor.interface);
        interface.is_some_and(|interface| {
            interface.addresses.iter().any(|(own, prefix)| match own {
                IpAddr::V4(own) => inside(address, super::network_part(*own, *prefix), *prefix),
                IpAddr::V6(_) => false,
            })
        })
    }

    /// The interface a host goes under and where: the routed network holding it, the interface's
    /// own network, or past it the interface's way to the internet.
    fn slot(&self, row: NeighborRow) -> (&str, Slot) {
        let neighbor = &self.neighbors[row.neighbor];
        let own = (neighbor.interface.as_str(), Slot::Own);
        let IpAddr::V4(address) = neighbor.address else { return own };
        if let Some(index) = holding(self.routed, address) {
            return (self.routed[index].interface.as_str(), Slot::Network(index));
        }
        let beyond = !self.on_own_network(neighbor, address)
            && self.internet.iter().any(|route| route.interface == neighbor.interface);
        if beyond { (neighbor.interface.as_str(), Slot::Internet) } else { own }
    }

    /// The interfaces the map shows: the one internet traffic leaves through first, then the
    /// other ways to the internet, the local networks and the tunnels, each by name.
    fn names<'b>(&'b self, hosts: &'b [(&'b str, Slot, NeighborRow)]) -> Vec<&'b str> {
        let mut names: Vec<&str> = self
            .interfaces
            .iter()
            .filter(|interface| shows(interface))
            .map(|interface| interface.name.as_str())
            .chain(self.routed.iter().map(|network| network.interface.as_str()))
            .chain(self.internet.iter().map(|route| route.interface.as_str()))
            .chain(hosts.iter().map(|(name, _, _)| *name))
            .collect();
        names.sort_unstable();
        names.dedup();
        let way = |name: &str| self.internet.iter().find(|route| route.interface == name);
        let rank = |name: &str| match (way(name), self.interfaces.iter().find(|interface| interface.name == name)) {
            (Some(route), _) if route.in_use => 0,
            (Some(_), _) => 1,
            (None, Some(interface)) if LAN_KINDS.contains(&interface.kind) => 2,
            (None, Some(interface)) if interface.kind == InterfaceKind::Tunnel => 3,
            (None, _) => 4,
        };
        names.sort_by_key(|name| rank(name));
        names
    }
}

/// The map's rows from `hosts`, which come filtered and sorted. Without a filter every heading
/// shows; with one, a heading stays when it `matches` or holds a row that stays.
pub fn entries(
    parts: &MapParts<'_>,
    hosts: Vec<NeighborRow>,
    filtered: bool,
    matches: impl Fn(NeighborEntry) -> bool,
) -> Vec<NeighborEntry> {
    let placed: Vec<(&str, Slot, NeighborRow)> = hosts
        .into_iter()
        .map(|row| {
            let (name, slot) = parts.slot(row);
            (name, slot, row)
        })
        .collect();
    let under = |name: &str, slot: Slot| -> Vec<NeighborEntry> {
        placed
            .iter()
            .filter(|(at, placed, _)| *at == name && *placed == slot)
            .map(|(_, _, row)| NeighborEntry::Host(*row))
            .collect()
    };
    let keeps = |heading: NeighborEntry, rows: &[NeighborEntry]| !filtered || !rows.is_empty() || matches(heading);
    let mut entries = Vec::new();
    for name in parts.names(&placed) {
        let mut group = under(name, Slot::Own);
        for index in (0..parts.routed.len()).filter(|index| parts.routed[*index].interface == name) {
            let rows = under(name, Slot::Network(index));
            if keeps(NeighborEntry::Network(index), &rows) {
                group.push(NeighborEntry::Network(index));
                group.extend(rows);
            }
        }
        if let Some(index) = parts.internet.iter().position(|route| route.interface == name) {
            let rows = under(name, Slot::Internet);
            if keeps(NeighborEntry::Internet(index), &rows) {
                group.push(NeighborEntry::Internet(index));
                group.extend(rows);
            }
        }
        let heading =
            parts.interfaces.iter().position(|interface| interface.name == name).map(NeighborEntry::Interface);
        let kept = match heading {
            Some(heading) => keeps(heading, &group),
            None => !group.is_empty(),
        };
        if kept {
            entries.extend(heading);
            entries.extend(group);
        }
    }
    entries
}

#[cfg(test)]
mod tests;
