//! What the Neighbors page knows, as a report: the attached networks, every host with what the
//! tables, scans and memory say about it, and the last port scan of each.

use std::{net::IpAddr, time::Duration};

use borg_i18n::tr;
use serde::Serialize;

use super::{
    scan::{self, MINOR, NetworkDocument, SECTION, SUBSECTION, ScanDocument, TITLE, port_list},
    text::{self, CSV_LIST_SEPARATOR, LINES, LIST_SEPARATOR, fact},
};
use crate::{AppState, Mac, NeighborState, PortScanRecord, Presence, ScanNetwork, vendor};

const MICROS_PER_MILLI: u128 = 1_000;
const CSV_HEADS: [&str; 14] = [
    "address",
    "mac",
    "vendor",
    "name",
    "interface",
    "state",
    "presence",
    "last_seen",
    "ports",
    "ping_ms",
    "flags",
    "port_scan_at",
    "port_scan_mode",
    "port_scan_open",
];

/// What the page says to look at in a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostFlag {
    Gateway,
    GatewayMacChanged,
    MacShared,
    Randomized,
}

impl HostFlag {
    fn label(self) -> &'static str {
        match self {
            Self::Gateway => tr!(network_gateway_note),
            Self::GatewayMacChanged => tr!(network_gateway_changed_note),
            Self::MacShared => tr!(network_mac_shared_note),
            Self::Randomized => tr!(network_randomized_note),
        }
    }
}

/// How Borg knows a host: from the system's tables, from a scan alone, or from memory while it
/// is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportPresence {
    Listed,
    Answered,
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportHost {
    pub address: IpAddr,
    pub mac: Option<Mac>,
    pub vendor: Option<&'static str>,
    pub name: Option<String>,
    pub interface: String,
    pub state: Option<NeighborState>,
    pub presence: ReportPresence,
    /// When a host that is gone was last seen, in seconds since the epoch.
    pub last_seen: Option<u64>,
    pub flags: Vec<HostFlag>,
    /// The ports a subnet scan found open.
    pub ports: Option<Vec<u16>>,
    pub ping: Option<Duration>,
    pub port_scan: Option<PortScanRecord>,
}

/// The Neighbors page when the report was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeighborsReport {
    /// When the tables were read, in seconds since the epoch.
    pub read_at: u64,
    pub networks: Vec<ScanNetwork>,
    pub dns: Vec<IpAddr>,
    pub hosts: Vec<ReportHost>,
}

#[derive(Serialize)]
struct HostDocument<'a> {
    address: IpAddr,
    #[serde(skip_serializing_if = "Option::is_none")]
    mac: Option<Mac>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vendor: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    interface: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<&'static str>,
    presence: ReportPresence,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_seen: Option<String>,
    #[serde(skip_serializing_if = "<[HostFlag]>::is_empty")]
    flags: &'a [HostFlag],
    #[serde(skip_serializing_if = "Option::is_none")]
    ports: Option<&'a [u16]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ping_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    port_scan: Option<ScanDocument<'a>>,
}

#[derive(Serialize)]
struct NeighborsDocument<'a> {
    read_at: String,
    networks: Vec<NetworkDocument>,
    dns: &'a [IpAddr],
    hosts: Vec<HostDocument<'a>>,
}

/// A ping time in whole milliseconds, rounded up as the page shows it.
fn milliseconds(ping: Duration) -> u128 {
    ping.as_micros().div_ceil(MICROS_PER_MILLI)
}

impl ReportHost {
    fn of(state: &AppState, row: crate::NeighborRow) -> Self {
        let network = &state.network;
        let neighbor = &network.neighborhood.neighbors[row.neighbor];
        let address = neighbor.address;
        let randomized = row.randomized(neighbor);
        let flags = [
            row.gateway.then_some(HostFlag::Gateway),
            row.changed.map(|_| HostFlag::GatewayMacChanged),
            row.shared.then_some(HostFlag::MacShared),
            randomized.then_some(HostFlag::Randomized),
        ];
        let (presence, last_seen) = match row.presence {
            Presence::Listed => (ReportPresence::Listed, None),
            Presence::Answered => (ReportPresence::Answered, None),
            Presence::Offline(at) => (ReportPresence::Offline, Some(at)),
        };
        Self {
            address,
            mac: neighbor.mac,
            vendor: neighbor.mac.filter(|_| !randomized).and_then(vendor),
            name: network.host(address).map(str::to_owned),
            interface: neighbor.interface.clone(),
            state: neighbor.state,
            presence,
            last_seen,
            flags: flags.into_iter().flatten().collect(),
            ports: network.open_ports(address).map(<[u16]>::to_vec),
            ping: network.probes.get(&address).and_then(|probed| probed.probe.ping),
            port_scan: crate::port_scan::scan_of(state, address).cloned(),
        }
    }

    fn document(&self) -> HostDocument<'_> {
        HostDocument {
            address: self.address,
            mac: self.mac,
            vendor: self.vendor,
            name: self.name.as_deref(),
            interface: &self.interface,
            state: self.state.map(NeighborState::name),
            presence: self.presence,
            last_seen: self.last_seen.map(text::utc),
            flags: &self.flags,
            ports: self.ports.as_deref(),
            ping_ms: self.ping.map(milliseconds),
            port_scan: self.port_scan.as_ref().map(scan::document),
        }
    }

    /// The state column: the system's word, or that the host is gone.
    fn state_cell(&self) -> String {
        match self.presence {
            ReportPresence::Offline => tr!(network_offline_state).to_owned(),
            ReportPresence::Listed | ReportPresence::Answered => {
                self.state.map(NeighborState::name).unwrap_or_default().to_owned()
            }
        }
    }

    fn note_cell(&self) -> String {
        let seen = self.last_seen.map(|at| tr!(network_seen_note, when = text::readable(at)));
        self.flags.iter().map(|flag| flag.label().to_owned()).chain(seen).collect::<Vec<_>>().join(LIST_SEPARATOR)
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.address.to_string(),
            self.mac.map(|mac| mac.to_string()).unwrap_or_default(),
            self.vendor.unwrap_or_default().to_owned(),
            self.name.clone().unwrap_or_default(),
            self.state_cell(),
            self.ports.as_deref().map(|ports| port_list(ports, LIST_SEPARATOR)).unwrap_or_default(),
            self.ping.map(|ping| tr!(network_ping_time, ms = milliseconds(ping))).unwrap_or_default(),
            self.note_cell(),
        ]
    }

    fn csv_row(&self) -> Vec<String> {
        let scan = self.port_scan.as_ref();
        vec![
            self.address.to_string(),
            self.mac.map(|mac| mac.to_string()).unwrap_or_default(),
            self.vendor.unwrap_or_default().to_owned(),
            self.name.clone().unwrap_or_default(),
            self.interface.clone(),
            self.state.map(NeighborState::name).unwrap_or_default().to_owned(),
            text::plain(self.presence),
            self.last_seen.map(text::utc).unwrap_or_default(),
            self.ports.as_deref().map(|ports| port_list(ports, CSV_LIST_SEPARATOR)).unwrap_or_default(),
            self.ping.map(|ping| milliseconds(ping).to_string()).unwrap_or_default(),
            self.flags.iter().map(text::plain).collect::<Vec<_>>().join(CSV_LIST_SEPARATOR),
            scan.map(|scan| text::utc(scan.at)).unwrap_or_default(),
            scan.map(|scan| text::plain(scan.speed)).unwrap_or_default(),
            scan.map(|scan| port_list(&scan.open, CSV_LIST_SEPARATOR)).unwrap_or_default(),
        ]
    }
}

impl NeighborsReport {
    /// Every host the page knows, the ones its filter hides too.
    pub fn of(state: &AppState) -> Self {
        let network = &state.network;
        Self {
            read_at: network.neighborhood.read_at,
            networks: network.places.iter().map(|place| crate::port_scan::scan_network(network, place)).collect(),
            dns: network.neighborhood.dns.clone(),
            hosts: network.every_neighbor().into_iter().map(|row| ReportHost::of(state, row)).collect(),
        }
    }

    pub(super) fn markdown(&self) -> String {
        let facts = std::iter::once(fact(tr!(report_read_label), text::readable(self.read_at)))
            .chain(self.networks.iter().flat_map(scan::network_facts))
            .chain((!self.dns.is_empty()).then(|| {
                let dns: Vec<String> = self.dns.iter().map(ToString::to_string).collect();
                fact(tr!(network_dns_label), dns.join(LIST_SEPARATOR))
            }))
            .collect::<Vec<_>>();
        let heads = [
            tr!(network_address_column),
            tr!(network_mac_column),
            tr!(network_vendor_column),
            tr!(network_name_column),
            tr!(network_state_column),
            tr!(network_ports_column),
            tr!(network_ping_column),
            tr!(network_note_column),
        ];
        let rows: Vec<Vec<String>> = self.hosts.iter().map(ReportHost::row).collect();
        let scans: Vec<&PortScanRecord> = self.hosts.iter().filter_map(|host| host.port_scan.as_ref()).collect();
        let scan_blocks = (!scans.is_empty())
            .then(|| format!("{SECTION}{}", tr!(report_port_scans_heading)))
            .into_iter()
            .chain(scans.into_iter().flat_map(|record| scan::section(record, SUBSECTION, MINOR)));
        let blocks = [format!("{TITLE}{}", tr!(network_neighbors_tab)), facts.join(LINES), text::table(&heads, &rows)]
            .into_iter()
            .chain(scan_blocks)
            .collect();
        text::document(blocks)
    }

    pub(super) fn json(&self) -> String {
        text::json(&NeighborsDocument {
            read_at: text::utc(self.read_at),
            networks: self.networks.iter().map(scan::network_document).collect(),
            dns: &self.dns,
            hosts: self.hosts.iter().map(ReportHost::document).collect(),
        })
    }

    /// One row for each host.
    pub(super) fn csv(&self) -> String {
        let rows: Vec<Vec<String>> = self.hosts.iter().map(ReportHost::csv_row).collect();
        text::csv(&CSV_HEADS, &rows)
    }
}
