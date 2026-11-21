//! One host's port scan as a report.

use std::net::IpAddr;

use borg_i18n::{Count, count, tr};
use serde::Serialize;

use super::text::{self, LINES, VALUE_SEPARATOR, fact};
use crate::{Ending, Mac, PortChoice, PortRange, PortScanRecord, ScanNetwork, ScanSpeed, describe_ports, port_name};

pub(super) const TITLE: &str = "# ";
pub(super) const SECTION: &str = "## ";
pub(super) const SUBSECTION: &str = "### ";
pub(super) const MINOR: &str = "#### ";
const CHOICE_GAP: &str = " ";
const CSV_HEADS: [&str; 6] = ["address", "target", "scanned_at", "mode", "port", "service"];

/// An open port and the service IANA names for it.
#[derive(Debug, Serialize)]
pub(super) struct OpenPort {
    port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    service: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub(super) struct NetworkDocument {
    interface: String,
    subnet: String,
    this_machine: IpAddr,
    #[serde(skip_serializing_if = "Option::is_none")]
    gateway: Option<IpAddr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gateway_mac: Option<Mac>,
}

/// A scan as the JSON report has it.
#[derive(Debug, Serialize)]
pub(super) struct ScanDocument<'a> {
    target: &'a str,
    address: IpAddr,
    scanned_at: String,
    mode: ScanSpeed,
    ports: &'a PortChoice,
    ports_asked: usize,
    open: Vec<OpenPort>,
    closed: usize,
    filtered: usize,
    #[serde(skip_serializing_if = "<[PortRange]>::is_empty")]
    not_scanned: &'a [PortRange],
    ending: &'a Ending,
    #[serde(skip_serializing_if = "Option::is_none")]
    network: Option<NetworkDocument>,
}

pub(super) fn document(record: &PortScanRecord) -> ScanDocument<'_> {
    ScanDocument {
        target: &record.target,
        address: record.address,
        scanned_at: text::utc(record.at),
        mode: record.speed,
        ports: &record.choice,
        ports_asked: record.count,
        open: record.open.iter().map(|&port| OpenPort { port, service: port_name(port) }).collect(),
        closed: record.closed,
        filtered: record.filtered,
        not_scanned: &record.not_scanned,
        ending: &record.ending,
        network: record.network.as_ref().map(network_document),
    }
}

pub(super) fn network_document(network: &ScanNetwork) -> NetworkDocument {
    NetworkDocument {
        interface: network.interface.clone(),
        subnet: network.cidr(),
        this_machine: IpAddr::V4(network.own),
        gateway: network.gateway,
        gateway_mac: network.gateway_mac,
    }
}

/// Which ports the scan was to try: the choice, the typed list, and how many.
fn choice(record: &PortScanRecord) -> String {
    let kind = match &record.choice {
        PortChoice::Common => tr!(port_scan_common).to_owned(),
        PortChoice::All => tr!(port_scan_all).to_owned(),
        PortChoice::Custom(list) => format!("{}{CHOICE_GAP}{list}", tr!(port_scan_list)),
    };
    format!("{kind}{VALUE_SEPARATOR}{}", count(record.count, Count::Port))
}

/// The network the scan ran from, as Markdown facts.
pub(super) fn network_facts(network: &ScanNetwork) -> Vec<String> {
    let subnet = tr!(network_subnet_value, subnet = network.cidr(), address = network.own);
    let gateway = [network.gateway.map(|address| address.to_string()), network.gateway_mac.map(|mac| mac.to_string())]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    [
        Some(fact(tr!(network_subnet_label), subnet)),
        Some(fact(tr!(network_interface_label), &network.interface)),
        (!gateway.is_empty()).then(|| fact(tr!(network_gateway_label), gateway.join(VALUE_SEPARATOR))),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Why the scan stopped before it tried every port, and which ports it left.
fn ending_notes(record: &PortScanRecord) -> Vec<String> {
    let skipped = count(record.skipped(), Count::Port);
    let note = match record.ending {
        Ending::RateLimited { port } => tr!(port_scan_limited, port = port, count = skipped),
        Ending::Cancelled => tr!(report_cancelled, count = skipped),
        Ending::Done | Ending::Failed(_) => return Vec::new(),
    };
    let left = (!record.not_scanned.is_empty())
        .then(|| fact(tr!(report_not_scanned_label), describe_ports(&record.not_scanned)));
    [Some(note), left].into_iter().flatten().collect()
}

/// The scan's facts, open ports and ending, under a heading of `level`.
pub(super) fn section(record: &PortScanRecord, level: &str, ports_level: &str) -> Vec<String> {
    let facts = [
        fact(tr!(network_port_scan_label), text::readable(record.at)),
        fact(tr!(port_scan_mode_label), record.speed.label()),
        fact(tr!(port_scan_ports_label), choice(record)),
        fact(tr!(network_open_ports_label), record.open.len()),
        fact(tr!(report_closed_label), record.closed),
        fact(tr!(report_filtered_label), record.filtered),
    ]
    .into_iter()
    .chain(record.network.as_ref().map(network_facts).unwrap_or_default())
    .collect::<Vec<_>>();
    let ports = if record.open.is_empty() {
        tr!(port_scan_no_open).to_owned()
    } else {
        let rows: Vec<Vec<String>> = record
            .open
            .iter()
            .map(|&port| vec![port.to_string(), port_name(port).unwrap_or_default().to_owned()])
            .collect();
        text::table(&[tr!(network_port_column), tr!(network_service_label)], &rows)
    };
    [
        format!("{level}{}", tr!(report_scan_heading, target = record.label())),
        facts.join(LINES),
        format!("{ports_level}{}", tr!(network_ports_label)),
        ports,
    ]
    .into_iter()
    .chain(ending_notes(record))
    .collect()
}

pub(super) fn markdown(record: &PortScanRecord) -> String {
    text::document(section(record, TITLE, SECTION))
}

pub(super) fn json(record: &PortScanRecord) -> String {
    text::json(&document(record))
}

/// One row for each open port.
pub(super) fn csv(record: &PortScanRecord) -> String {
    let rows: Vec<Vec<String>> = record
        .open
        .iter()
        .map(|&port| {
            vec![
                record.address.to_string(),
                record.target.clone(),
                text::utc(record.at),
                text::plain(record.speed),
                port.to_string(),
                port_name(port).unwrap_or_default().to_owned(),
            ]
        })
        .collect();
    text::csv(&CSV_HEADS, &rows)
}

/// Ports joined with `separator`.
pub(super) fn port_list(ports: &[u16], separator: &str) -> String {
    ports.iter().map(ToString::to_string).collect::<Vec<_>>().join(separator)
}
