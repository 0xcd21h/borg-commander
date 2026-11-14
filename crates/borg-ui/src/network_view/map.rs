//! The Neighbors page's headings: each interface, the networks it reaches and its way to the
//! internet, as rows and in the details pane.

use super::{first_address, seen};
use crate::{
    activity_view::{body, dim, emphasis, heading, row, text, wrap},
    docker_view::Facts,
    look::Look,
};
use borg_core::{Interface, InternetRoute, Neighbor, NeighborEntry, Network, Routed, kind_label};
use borg_i18n::{Count, count, tr};
use jiff::Zoned;
use ratatui::{buffer::Buffer, layout::Rect, style::Modifier};
use std::net::Ipv4Addr;
use unicode_width::UnicodeWidthStr;

/// What sets a network or an internet heading apart from its interface's.
const BRANCH: &str = "› ";
const SEPARATOR: &str = " · ";
/// What joins the routers on the way to a network.
const HOP: &str = " › ";
const LIST_SEPARATOR: &str = ", ";
/// Cells between a fact's label and its value.
const LABEL_GAP: u16 = 2;
/// The prefix an address alone has.
const HOST_PREFIX: u8 = 32;

/// The hosts listed under the heading at `index`, up to the next heading.
fn hosts_under(network: &Network, index: usize) -> usize {
    network.neighbors[index + 1..].iter().take_while(|entry| entry.host().is_some()).count()
}

fn address_with_prefix(interface: &Interface) -> Option<String> {
    let address = first_address(&interface.addresses)?;
    let prefix = interface.addresses.iter().find(|(known, _)| *known == address).map(|(_, prefix)| *prefix)?;
    Some(if address.is_ipv4() && prefix < HOST_PREFIX { format!("{address}/{prefix}") } else { address.to_string() })
}

fn routers(path: &[Ipv4Addr]) -> String {
    path.iter().map(ToString::to_string).collect::<Vec<_>>().join(HOP)
}

fn reached(routed: &Routed) -> Option<String> {
    borg_core::route_way(routed)
}

fn hops(routed: &Routed) -> Option<String> {
    routed.trace.hops.map(|hops| count(usize::from(hops), Count::Hop))
}

fn internet_parts(route: &InternetRoute) -> Vec<String> {
    let via = route.gateway().map(|gateway| tr!(network_via, gateway = gateway));
    let in_use = route.in_use.then(|| tr!(network_in_use).to_owned());
    [Some(tr!(network_internet).to_owned()), via, in_use].into_iter().flatten().collect()
}

/// A heading row's text, or `None` for a host.
pub(super) fn heading_line(network: &Network, index: usize, clock: &Zoned) -> Option<String> {
    let line = match network.neighbors[index] {
        NeighborEntry::Host(_) => return None,
        NeighborEntry::Interface(at) => {
            let interface = &network.sample.interfaces[at];
            let switch = network.hearing.switch(&interface.name).map(|switch| borg_core::switch_line(&switch.port));
            let parts = [
                Some(interface.name.clone()),
                Some(kind_label(interface.kind).to_owned()),
                address_with_prefix(interface),
                switch.filter(|line| !line.is_empty()),
            ];
            parts.into_iter().flatten().collect::<Vec<_>>().join(SEPARATOR)
        }
        NeighborEntry::Network(at) => {
            let routed = &network.routed[at];
            let hosts =
                Some(hosts_under(network, index)).filter(|hosts| *hosts > 0).map(|hosts| count(hosts, Count::Host));
            let gone = routed
                .remembered
                .filter(|_| !network.routed_live(at))
                .map(|at| tr!(network_seen_note, when = seen(at, clock)));
            let parts = [Some(routed.cidr()), reached(routed), hops(routed), hosts, gone];
            format!("{BRANCH}{}", parts.into_iter().flatten().collect::<Vec<_>>().join(SEPARATOR))
        }
        NeighborEntry::Internet(at) => format!("{BRANCH}{}", internet_parts(&network.internet[at]).join(SEPARATOR)),
    };
    Some(line)
}

/// The status line on a heading: what it names and the interface it is on.
pub(super) fn status(network: &Network) -> String {
    let parts = match network.entry() {
        Some(NeighborEntry::Interface(at)) => {
            let interface = &network.sample.interfaces[at];
            vec![interface.name.clone(), kind_label(interface.kind).to_owned()]
        }
        Some(NeighborEntry::Network(at)) => {
            let routed = &network.routed[at];
            vec![routed.cidr(), routed.interface.clone()]
        }
        Some(NeighborEntry::Internet(at)) => {
            vec![tr!(network_internet).to_owned(), network.internet[at].interface.clone()]
        }
        Some(NeighborEntry::Host(_)) | None => Vec::new(),
    };
    parts.join(SEPARATOR)
}

/// The router a host without a MAC address is reached through, as the MAC column shows it.
pub(super) fn via(network: &Network, neighbor: &Neighbor) -> Option<String> {
    network.router_for(neighbor).map(|gateway| tr!(network_via, gateway = gateway))
}

/// The routed network a host is behind, as its details name it.
pub(super) fn route_of(network: &Network, neighbor: &Neighbor) -> Option<String> {
    let routed = network.routed_network(neighbor.address)?;
    Some([Some(routed.cidr()), reached(routed)].into_iter().flatten().collect::<Vec<_>>().join(SEPARATOR))
}

fn facts<'a>(area: Rect, y: u16, labels: &[&str], look: &Look<'a>) -> Facts<'a> {
    let label = labels.iter().map(|label| label.width() as u16).max().unwrap_or(0) + LABEL_GAP;
    Facts { area, y, label, theme: look.theme }
}

/// Sentences under the facts, wrapped, from row `y` on.
fn sentences(sentences: &[String], body: Rect, y: u16, look: &Look<'_>, buf: &mut Buffer) {
    let mut y = y;
    for sentence in sentences {
        for line in wrap(sentence, usize::from(body.width)) {
            if y < body.height {
                text(buf, row(body, y), line, dim(look.theme));
            }
            y += 1;
        }
        y += 1;
    }
}

/// A routed network's details: how it is reached, how far, how Borg knows it and what is there.
pub(super) fn network_details(
    network: &Network,
    index: usize,
    at: usize,
    look: &Look<'_>,
    rect: Rect,
    clock: &Zoned,
    buf: &mut Buffer,
) {
    let theme = look.theme;
    let routed = &network.routed[at];
    let inner =
        crate::results_view::pane_frame(network.expanded, look, rect, &routed.cidr(), network.details_focused, buf);
    if inner.is_empty() {
        return;
    }
    let way = reached(routed).unwrap_or_else(|| routed.interface.clone());
    text(buf, row(inner, 0), way, emphasis(theme));
    let kind = network
        .sample
        .interfaces
        .iter()
        .find(|interface| interface.name == routed.interface)
        .map(|interface| interface.kind);
    let interface = [Some(routed.interface.clone()), kind.map(|kind| kind_label(kind).to_owned())];
    text(buf, row(inner, 1), interface.into_iter().flatten().collect::<Vec<_>>().join(SEPARATOR), theme.styles.overlay);
    let body = body(rect);
    let labels = [
        tr!(network_route_label),
        tr!(network_hops_label),
        tr!(network_source_label),
        tr!(network_hosts_label),
        tr!(network_last_seen_label),
    ];
    let mut facts = facts(body, 0, &labels, look);
    let path = routed.path();
    let route = if path.is_empty() {
        routed.interface.clone()
    } else {
        format!("{}{SEPARATOR}{}", routers(&path), routed.interface)
    };
    facts.add(buf, labels[0], route);
    facts.add(buf, labels[1], hops(routed).unwrap_or_default());
    let source = match routed.origin {
        borg_core::Origin::Table => tr!(network_found_table),
        borg_core::Origin::Discovery => tr!(network_found_discovery),
    };
    facts.add(buf, labels[2], source.to_owned());
    facts.add(buf, labels[3], hosts_under(network, index).to_string());
    let live = network.routed_live(at);
    if let Some(when) = routed.remembered.filter(|_| !live) {
        facts.add(buf, labels[4], seen(when, clock));
    }
    let tunnel = kind == Some(borg_core::InterfaceKind::Tunnel) && routed.gateway.is_none();
    let notes = [
        routed.on_gateway().then(|| tr!(network_on_gateway_detail).to_owned()),
        tunnel.then(|| tr!(network_tunnel_detail, interface = routed.interface)),
        routed.remembered.is_some().then(|| tr!(network_remembered_detail).to_owned()),
    ];
    let notes: Vec<String> = notes.into_iter().flatten().collect();
    sentences(&notes, body, facts.y + 1, look, buf);
}

/// An interface's way to the internet: its gateways, the wide routes that take most addresses,
/// and whether traffic leaves this way now.
pub(super) fn internet_details(network: &Network, at: usize, look: &Look<'_>, rect: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let route = &network.internet[at];
    let inner = crate::results_view::pane_frame(
        network.expanded,
        look,
        rect,
        tr!(network_internet),
        network.details_focused,
        buf,
    );
    if inner.is_empty() {
        return;
    }
    text(buf, row(inner, 0), internet_parts(route)[1..].join(SEPARATOR), emphasis(theme));
    text(buf, row(inner, 1), route.interface.clone(), theme.styles.overlay);
    let body = body(rect);
    let labels = [tr!(network_interface_label), tr!(network_gateway_label), tr!(network_routes_label)];
    let mut facts = facts(body, 0, &labels, look);
    facts.add(buf, labels[0], route.interface.clone());
    let gateways: Vec<String> = route.gateways.iter().map(ToString::to_string).collect();
    facts.add(buf, labels[1], gateways.join(LIST_SEPARATOR));
    let wide: Vec<String> = route.wide.iter().map(|wide| format!("{}/{}", wide.network, wide.prefix)).collect();
    if !wide.is_empty() {
        facts.add(buf, labels[2], wide.join(LIST_SEPARATOR));
    }
    let exit = network.internet.iter().find(|other| other.in_use).map(|other| other.interface.clone());
    let leaves = match (route.in_use, exit) {
        (true, _) => Some(tr!(network_internet_in_use).to_owned()),
        (false, Some(exit)) => Some(tr!(network_internet_standby, interface = exit)),
        (false, None) => None,
    };
    let wide = (!wide.is_empty()).then(|| tr!(network_internet_wide_detail, routes = wide.join(LIST_SEPARATOR)));
    let notes: Vec<String> = [leaves, wide].into_iter().flatten().collect();
    sentences(&notes, body, facts.y + 1, look, buf);
}

/// How a heading row looks: bold, in the heading color unless the cursor is on it.
pub(super) fn heading_style(look: &Look<'_>, base: ratatui::style::Style, selected: bool) -> ratatui::style::Style {
    let style = if selected { base } else { heading(look.theme) };
    style.add_modifier(Modifier::BOLD)
}
