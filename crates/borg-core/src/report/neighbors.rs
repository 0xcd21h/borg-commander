//! What the Neighbors page knows, as a report: the attached networks, the networks reached
//! through routers and tunnels, each interface's way to the internet and the switch port it is
//! plugged into, every host with what the tables, scans, listening and memory say about it, and
//! the last port scan of each.

use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};

use borg_i18n::tr;
use serde::Serialize;

use super::{
    scan::{self, MINOR, NetworkDocument, SECTION, SUBSECTION, ScanDocument, TITLE, port_list},
    text::{self, CSV_LIST_SEPARATOR, LINES, LIST_SEPARATOR, fact},
};
use crate::{
    Announced, AppState, HeardSwitch, InternetRoute, ListenProtocol, Mac, NeighborState, Origin, PortScanRecord,
    Presence, Routed, ScanNetwork, SwitchPort, switch_line, vendor,
};

const MICROS_PER_MILLI: u128 = 1_000;
const VALUE_SEPARATOR: &str = " · ";
const CSV_HEADS: [&str; 21] = [
    "address",
    "mac",
    "vendor",
    "name",
    "interface",
    "network",
    "via",
    "state",
    "presence",
    "last_seen",
    "ports",
    "ping_ms",
    "flags",
    "port_scan_at",
    "port_scan_mode",
    "port_scan_open",
    "heard_at",
    "heard_from",
    "model",
    "device_types",
    "services",
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

/// How Borg knows a host: from the system's tables, from a scan alone, from listening alone, or
/// from memory while it is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportPresence {
    Listed,
    Answered,
    Heard,
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportHost {
    pub address: IpAddr,
    pub mac: Option<Mac>,
    pub vendor: Option<&'static str>,
    pub name: Option<String>,
    pub interface: String,
    /// The routed network the host is behind.
    pub network: Option<String>,
    /// The router a host without a MAC address is reached through.
    pub via: Option<Ipv4Addr>,
    pub state: Option<NeighborState>,
    pub presence: ReportPresence,
    /// When a host that is gone was last seen, in seconds since the epoch.
    pub last_seen: Option<u64>,
    pub flags: Vec<HostFlag>,
    /// The ports a subnet scan found open.
    pub ports: Option<Vec<u16>>,
    pub ping: Option<Duration>,
    pub port_scan: Option<PortScanRecord>,
    /// When listening last heard the host, in seconds since the epoch.
    pub heard: Option<u64>,
    /// What the host announced of itself.
    pub announced: Option<Announced>,
}

/// The Neighbors page when the report was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeighborsReport {
    /// When the tables were read, in seconds since the epoch.
    pub read_at: u64,
    pub networks: Vec<ScanNetwork>,
    pub routes: Vec<Routed>,
    pub internet: Vec<InternetRoute>,
    pub dns: Vec<IpAddr>,
    /// The switch ports listening heard the interfaces are plugged into.
    pub switches: Vec<HeardSwitch>,
    pub hosts: Vec<ReportHost>,
}

#[derive(Serialize)]
struct HeardDocument<'a> {
    last: String,
    #[serde(flatten)]
    announced: &'a Announced,
}

#[derive(Serialize)]
struct SwitchDocument<'a> {
    interface: &'a str,
    #[serde(flatten)]
    port: &'a SwitchPort,
    heard: String,
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
    network: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    via: Option<Ipv4Addr>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    heard: Option<HeardDocument<'a>>,
}

#[derive(Serialize)]
struct RouteDocument<'a> {
    network: String,
    interface: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    gateway: Option<Ipv4Addr>,
    #[serde(skip_serializing_if = "<[Ipv4Addr]>::is_empty")]
    path: &'a [Ipv4Addr],
    #[serde(skip_serializing_if = "Option::is_none")]
    hops: Option<u8>,
    source: Origin,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_seen: Option<String>,
}

#[derive(Serialize)]
struct InternetDocument<'a> {
    interface: &'a str,
    #[serde(skip_serializing_if = "<[IpAddr]>::is_empty")]
    gateways: &'a [IpAddr],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    wide: Vec<String>,
    in_use: bool,
}

#[derive(Serialize)]
struct NeighborsDocument<'a> {
    read_at: String,
    networks: Vec<NetworkDocument>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    routes: Vec<RouteDocument<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    internet: Vec<InternetDocument<'a>>,
    dns: &'a [IpAddr],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    switches: Vec<SwitchDocument<'a>>,
    hosts: Vec<HostDocument<'a>>,
}

fn route_document(routed: &Routed) -> RouteDocument<'_> {
    RouteDocument {
        network: routed.cidr(),
        interface: &routed.interface,
        gateway: routed.gateway,
        path: &routed.trace.path,
        hops: routed.trace.hops,
        source: routed.origin,
        last_seen: routed.remembered.map(text::utc),
    }
}

fn internet_document(route: &InternetRoute) -> InternetDocument<'_> {
    InternetDocument {
        interface: &route.interface,
        gateways: &route.gateways,
        wide: route.wide.iter().map(|wide| format!("{}/{}", wide.network, wide.prefix)).collect(),
        in_use: route.in_use,
    }
}

/// A routed network as the report's facts give it: the network, how it is reached, how far, and
/// the interface.
fn route_fact(routed: &Routed) -> String {
    let hops = routed.trace.hops.map(|hops| borg_i18n::count(usize::from(hops), borg_i18n::Count::Hop));
    let parts = [Some(routed.cidr()), crate::route_way(routed), hops, Some(routed.interface.clone())];
    fact(tr!(network_route_label), parts.into_iter().flatten().collect::<Vec<_>>().join(VALUE_SEPARATOR))
}

/// An interface's way to the internet as the report's facts give it.
fn internet_fact(route: &InternetRoute) -> String {
    let via = route.gateway().map(|gateway| tr!(network_via, gateway = gateway));
    let in_use = route.in_use.then(|| tr!(network_in_use).to_owned());
    let parts = [Some(route.interface.clone()), via, in_use];
    fact(tr!(network_internet), parts.into_iter().flatten().collect::<Vec<_>>().join(VALUE_SEPARATOR))
}

/// The switch port an interface is plugged into, as the report's facts give it.
fn switch_fact(switch: &HeardSwitch) -> String {
    fact(tr!(listen_switch_label), format!("{}{VALUE_SEPARATOR}{}", switch.interface, switch_line(&switch.port)))
}

fn switch_document(switch: &HeardSwitch) -> SwitchDocument<'_> {
    SwitchDocument { interface: &switch.interface, port: &switch.port, heard: text::utc(switch.at) }
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
        let routed = network.routed_network(address);
        let (presence, last_seen) = match row.presence {
            Presence::Listed => (ReportPresence::Listed, None),
            Presence::Answered => (ReportPresence::Answered, None),
            Presence::Heard => (ReportPresence::Heard, None),
            Presence::Offline(at) => (ReportPresence::Offline, Some(at)),
        };
        Self {
            address,
            mac: neighbor.mac,
            vendor: neighbor.mac.filter(|_| !randomized).and_then(vendor),
            name: network.host_name(address).map(str::to_owned),
            interface: neighbor.interface.clone(),
            network: routed.map(Routed::cidr),
            via: network.router_for(neighbor),
            state: neighbor.state,
            presence,
            last_seen,
            flags: flags.into_iter().flatten().collect(),
            ports: network.open_ports(address).map(<[u16]>::to_vec),
            ping: network.probes.get(&address).and_then(|probed| probed.probe.ping),
            port_scan: crate::port_scan::scan_of(state, address).cloned(),
            heard: network.last_heard(address),
            announced: network.announced(address),
        }
    }

    fn document(&self) -> HostDocument<'_> {
        HostDocument {
            address: self.address,
            mac: self.mac,
            vendor: self.vendor,
            name: self.name.as_deref(),
            interface: &self.interface,
            network: self.network.as_deref(),
            via: self.via,
            state: self.state.map(NeighborState::name),
            presence: self.presence,
            last_seen: self.last_seen.map(text::utc),
            flags: &self.flags,
            ports: self.ports.as_deref(),
            ping_ms: self.ping.map(milliseconds),
            port_scan: self.port_scan.as_ref().map(scan::document),
            heard: self
                .announced
                .as_ref()
                .map(|announced| HeardDocument { last: self.heard.map(text::utc).unwrap_or_default(), announced }),
        }
    }

    /// The state column: the system's word, that only listening heard the host, or that it is
    /// gone.
    fn state_cell(&self) -> String {
        match self.presence {
            ReportPresence::Offline => tr!(network_offline_state).to_owned(),
            ReportPresence::Heard => tr!(network_heard_state).to_owned(),
            ReportPresence::Listed | ReportPresence::Answered => {
                self.state.map(NeighborState::name).unwrap_or_default().to_owned()
            }
        }
    }

    /// The host's line in the Heard table, when it announced something.
    fn heard_row(&self) -> Option<Vec<String>> {
        let announced = self.announced.as_ref()?;
        let protocols: Vec<&str> = announced.protocols.iter().map(|protocol| protocol.name()).collect();
        Some(vec![
            self.address.to_string(),
            announced.name().map(|name| name.text.clone()).unwrap_or_default(),
            announced.model.as_ref().map(|model| model.text.clone()).unwrap_or_default(),
            announced.kinds.join(LIST_SEPARATOR),
            announced.services.join(LIST_SEPARATOR),
            protocols.join(LIST_SEPARATOR),
            self.heard.map(text::readable).unwrap_or_default(),
        ])
    }

    fn note_cell(&self) -> String {
        let via = self.via.map(|gateway| tr!(network_via, gateway = gateway));
        let seen = self.last_seen.map(|at| tr!(network_seen_note, when = text::readable(at)));
        let flags = self.flags.iter().map(|flag| flag.label().to_owned());
        via.into_iter().chain(flags).chain(seen).collect::<Vec<_>>().join(LIST_SEPARATOR)
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
        let announced = self.announced.as_ref();
        vec![
            self.address.to_string(),
            self.mac.map(|mac| mac.to_string()).unwrap_or_default(),
            self.vendor.unwrap_or_default().to_owned(),
            self.name.clone().unwrap_or_default(),
            self.interface.clone(),
            self.network.clone().unwrap_or_default(),
            self.via.map(|via| via.to_string()).unwrap_or_default(),
            self.state.map(NeighborState::name).unwrap_or_default().to_owned(),
            text::plain(self.presence),
            self.last_seen.map(text::utc).unwrap_or_default(),
            self.ports.as_deref().map(|ports| port_list(ports, CSV_LIST_SEPARATOR)).unwrap_or_default(),
            self.ping.map(|ping| milliseconds(ping).to_string()).unwrap_or_default(),
            self.flags.iter().map(text::plain).collect::<Vec<_>>().join(CSV_LIST_SEPARATOR),
            scan.map(|scan| text::utc(scan.at)).unwrap_or_default(),
            scan.map(|scan| text::plain(scan.speed)).unwrap_or_default(),
            scan.map(|scan| port_list(&scan.open, CSV_LIST_SEPARATOR)).unwrap_or_default(),
            self.heard.map(text::utc).unwrap_or_default(),
            announced.map(|announced| plain_list(&announced.protocols)).unwrap_or_default(),
            announced
                .and_then(|announced| announced.model.as_ref())
                .map(|model| model.text.clone())
                .unwrap_or_default(),
            announced.map(|announced| announced.kinds.join(CSV_LIST_SEPARATOR)).unwrap_or_default(),
            announced.map(|announced| announced.services.join(CSV_LIST_SEPARATOR)).unwrap_or_default(),
        ]
    }
}

/// Protocols as the CSV names them.
fn plain_list(protocols: &[ListenProtocol]) -> String {
    protocols.iter().map(text::plain).collect::<Vec<_>>().join(CSV_LIST_SEPARATOR)
}

impl NeighborsReport {
    /// Every host the page knows, the ones its filter hides too.
    pub fn of(state: &AppState) -> Self {
        let network = &state.network;
        Self {
            read_at: network.neighborhood.read_at,
            networks: network.places.iter().map(|place| crate::port_scan::scan_network(network, place)).collect(),
            routes: network.routed.clone(),
            internet: network.internet.clone(),
            dns: network.neighborhood.dns.clone(),
            switches: network.hearing.switches.clone(),
            hosts: network.every_neighbor().into_iter().map(|row| ReportHost::of(state, row)).collect(),
        }
    }

    pub(super) fn markdown(&self) -> String {
        let facts = std::iter::once(fact(tr!(report_read_label), text::readable(self.read_at)))
            .chain(self.networks.iter().flat_map(scan::network_facts))
            .chain(self.routes.iter().map(route_fact))
            .chain(self.internet.iter().map(internet_fact))
            .chain(self.switches.iter().map(switch_fact))
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
        let heard: Vec<Vec<String>> = self.hosts.iter().filter_map(ReportHost::heard_row).collect();
        let heard_heads = [
            tr!(network_address_column),
            tr!(network_heard_name_label),
            tr!(network_model_label),
            tr!(network_device_types_label),
            tr!(network_services_label),
            tr!(network_heard_in_label),
            tr!(network_last_heard_label),
        ];
        let heard_blocks = (!heard.is_empty())
            .then(|| [format!("{SECTION}{}", tr!(network_heard_heading)), text::table(&heard_heads, &heard)])
            .into_iter()
            .flatten();
        let scans: Vec<&PortScanRecord> = self.hosts.iter().filter_map(|host| host.port_scan.as_ref()).collect();
        let scan_blocks = (!scans.is_empty())
            .then(|| format!("{SECTION}{}", tr!(report_port_scans_heading)))
            .into_iter()
            .chain(scans.into_iter().flat_map(|record| scan::section(record, SUBSECTION, MINOR)));
        let blocks = [format!("{TITLE}{}", tr!(network_neighbors_tab)), facts.join(LINES), text::table(&heads, &rows)]
            .into_iter()
            .chain(heard_blocks)
            .chain(scan_blocks)
            .collect();
        text::document(blocks)
    }

    pub(super) fn json(&self) -> String {
        text::json(&NeighborsDocument {
            read_at: text::utc(self.read_at),
            networks: self.networks.iter().map(scan::network_document).collect(),
            routes: self.routes.iter().map(route_document).collect(),
            internet: self.internet.iter().map(internet_document).collect(),
            dns: &self.dns,
            switches: self.switches.iter().map(switch_document).collect(),
            hosts: self.hosts.iter().map(ReportHost::document).collect(),
        })
    }

    /// One row for each host.
    pub(super) fn csv(&self) -> String {
        let rows: Vec<Vec<String>> = self.hosts.iter().map(ReportHost::csv_row).collect();
        text::csv(&CSV_HEADS, &rows)
    }
}
