//! The form H opens on the Neighbors page: the interface to listen on, for how long, and as
//! whom.

use crate::network::{CaptureAccess, Interface, InterfaceKind, NeighborEntry, Network};

/// How long listening can run: a few minutes at most, or until it is stopped.
pub const LISTEN_DURATIONS: [Option<u64>; 4] = [Some(30), Some(60), Some(300), None];
/// The duration the form starts on: a minute, which hears most devices announce themselves.
const FIRST_DURATION: usize = 1;
/// Interfaces with no link to hear on.
const LINKLESS: [InterfaceKind; 2] = [InterfaceKind::Loopback, InterfaceKind::Tunnel];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenField {
    Interface,
    Duration,
    Access,
}

/// Every field, **Run as** last since it shows only where Borg can listen as administrator.
const LISTEN_FIELDS: [ListenField; 3] = [ListenField::Interface, ListenField::Duration, ListenField::Access];

/// What listening asks `tcpdump` for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenRequest {
    pub interface: String,
    /// How long it runs, or `None` until it is stopped.
    pub seconds: Option<u64>,
    /// Run `tcpdump` as administrator, after the system asks for your password.
    pub administrator: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenForm {
    pub interfaces: Vec<String>,
    pub interface: usize,
    pub duration: usize,
    pub administrator: bool,
    pub access: CaptureAccess,
    pub field: usize,
}

impl ListenForm {
    /// The form on the selected row's interface, or `None` when no interface can listen.
    pub(crate) fn of(network: &Network) -> Option<Self> {
        let interfaces = listen_interfaces(network);
        (!interfaces.is_empty()).then_some(Self {
            interfaces,
            interface: 0,
            duration: FIRST_DURATION,
            administrator: false,
            access: network.access,
            field: 0,
        })
    }

    /// The fields the form shows.
    pub fn fields(&self) -> &'static [ListenField] {
        let shown = if self.access.elevate { LISTEN_FIELDS.len() } else { LISTEN_FIELDS.len() - 1 };
        &LISTEN_FIELDS[..shown]
    }

    pub fn selected(&self) -> ListenField {
        self.fields()[self.field.min(self.fields().len() - 1)]
    }

    pub fn request(&self) -> Option<ListenRequest> {
        Some(ListenRequest {
            interface: self.interfaces.get(self.interface)?.clone(),
            seconds: LISTEN_DURATIONS[self.duration],
            administrator: self.administrator,
        })
    }

    /// Moves the selected field's value by `step`, round past either end.
    pub(crate) fn change(&mut self, step: isize) {
        let turn = |value: usize, count: usize| (value as isize + step).rem_euclid(count.max(1) as isize) as usize;
        match self.selected() {
            ListenField::Interface => self.interface = turn(self.interface, self.interfaces.len()),
            ListenField::Duration => self.duration = turn(self.duration, LISTEN_DURATIONS.len()),
            ListenField::Access => self.administrator = !self.administrator,
        }
    }
}

/// The interfaces that are up on a link: the selected row's first, then the one internet traffic
/// leaves through, then the rest by name.
pub fn listen_interfaces(network: &Network) -> Vec<String> {
    let selected = network.entry().and_then(|entry| match entry {
        NeighborEntry::Interface(index) => network.sample.interfaces.get(index).map(|interface| interface.name.clone()),
        NeighborEntry::Host(row) => network.neighborhood.neighbors.get(row.neighbor).map(|host| host.interface.clone()),
        NeighborEntry::Network(index) => network.routed.get(index).map(|routed| routed.interface.clone()),
        NeighborEntry::Internet(index) => network.internet.get(index).map(|route| route.interface.clone()),
    });
    let exit = network.internet.iter().find(|route| route.in_use).map(|route| route.interface.clone());
    let mut up: Vec<&Interface> = network
        .sample
        .interfaces
        .iter()
        .filter(|interface| interface.up && !LINKLESS.contains(&interface.kind) && !interface.addresses.is_empty())
        .collect();
    up.sort_by_key(|interface| {
        (Some(&interface.name) != selected.as_ref(), Some(&interface.name) != exit.as_ref(), interface.name.clone())
    });
    up.into_iter().map(|interface| interface.name.clone()).collect()
}
