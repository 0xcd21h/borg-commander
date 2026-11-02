//! The details pane: the selected port, process, connection or interface, its traffic over the
//! last minute, and the connections it has.

use super::{DOWN, Field, UP, cells, column, endpoint, frame, list::reach, open, page_label, rate, socket_rate};
use crate::{
    activity_view::{Unit, body, dim, emphasis, heading, row, text},
    docker_view::{Facts, Series, charts, tiles},
    format,
    look::Look,
};
use borg_core::{Interface, Meter, Network, NetworkPage, PortRow, ProcessRow, Rate, Socket, kind_label, service};
use borg_i18n::tr;
use ratatui::{buffer::Buffer, layout::Rect};
use unicode_width::UnicodeWidthStr;

/// Rows the charts take when the pane also lists connections under them.
const CHART_HEIGHT: u16 = 10;
/// Cells between a fact's label and its value.
const LABEL_GAP: u16 = 2;
const TABLE_RATE_WIDTH: u16 = 9;
const TABLE_STATE_WIDTH: u16 = 12;

pub(super) fn draw_details(network: &Network, look: &Look<'_>, rect: Rect, buf: &mut Buffer) {
    let focused = network.details_focused;
    match network.page {
        NetworkPage::Ports => match network.port() {
            Some(listener) => port(network, listener, look, rect, buf),
            None => _ = frame(network, look, rect, page_label(network.page), focused, buf),
        },
        NetworkPage::Processes => match network.process() {
            Some(entry) => process(network, entry, look, rect, buf),
            None => _ = frame(network, look, rect, page_label(network.page), focused, buf),
        },
        NetworkPage::Connections => match network.connection() {
            Some((socket, rate)) => connection(network, socket, rate, look, rect, buf),
            None => _ = frame(network, look, rect, page_label(network.page), focused, buf),
        },
        NetworkPage::Interfaces => match network.interface() {
            Some((interface, rate)) => interface_details(network, interface, rate, look, rect, buf),
            None => _ = frame(network, look, rect, page_label(network.page), focused, buf),
        },
    }
}

/// The process line under a row's summary: its name and PID, or what the screen calls a
/// process it cannot see.
fn subject(network: &Network, pid: Option<u32>) -> String {
    match pid {
        Some(pid) => tr!(process_control_subject, name = network.name(Some(pid)), pid = pid),
        None => network.name(None),
    }
}

fn facts<'a>(area: Rect, y: u16, labels: &[&str], look: &Look<'a>) -> Facts<'a> {
    let label = labels.iter().map(|label| label.width() as u16).max().unwrap_or(0) + LABEL_GAP;
    Facts { area, y, label, theme: look.theme }
}

/// Download and upload over the last minute.
fn traffic(meter: Option<&Meter>) -> (Vec<u64>, Vec<u64>) {
    meter.map_or_else(Default::default, |meter| (meter.downloads(), meter.uploads()))
}

fn draw_charts(downloads: &[u64], uploads: &[u64], area: Rect, look: &Look<'_>, buf: &mut Buffer) {
    charts(
        buf,
        area,
        [
            Series { title: tr!(network_download_chart), values: downloads, unit: Unit::Rate },
            Series { title: tr!(network_upload_chart), values: uploads, unit: Unit::Rate },
        ],
        look,
    );
}

fn moved(rate: Rate) -> String {
    format!("↓ {} ↑ {}", format::size(rate.down), format::size(rate.up))
}

/// What is left of `area` below its first `used` rows.
fn below(area: Rect, used: u16) -> Rect {
    Rect { y: area.y + used.min(area.height), height: area.height.saturating_sub(used), ..area }
}

fn port(network: &Network, listener: &PortRow, look: &Look<'_>, rect: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let title = format!("{} {}", listener.transport.name(), listener.port);
    let inner = frame(network, look, rect, &title, network.details_focused, buf);
    if inner.is_empty() {
        return;
    }
    text(buf, row(inner, 0), reach(listener), emphasis(theme));
    text(buf, row(inner, 1), subject(network, listener.pid), theme.styles.overlay);
    let body = body(rect);
    let labels = [tr!(network_service_label), tr!(network_addresses_label), tr!(network_traffic_label)];
    let mut facts = facts(body, 0, &labels, look);
    facts.add(buf, labels[0], service(listener.port).unwrap_or_default());
    facts.add(buf, labels[1], listener.addresses.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "));
    facts.add(buf, labels[2], format!("↓ {} ↑ {}", format::rate(listener.rate.down), format::rate(listener.rate.up)));
    let rest = below(body, facts.y + 1);
    let (downloads, uploads) = traffic(network.traffic.ports.get(&listener.key()));
    let charted = CHART_HEIGHT.min(rest.height / 2);
    draw_charts(&downloads, &uploads, Rect { height: charted, ..rest }, look, buf);
    connections(network, look, below(rest, charted + 1), buf);
}

fn process(network: &Network, entry: &ProcessRow, look: &Look<'_>, rect: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let inner = frame(network, look, rect, &network.name(entry.pid), network.details_focused, buf);
    if inner.is_empty() {
        return;
    }
    text(buf, row(inner, 0), subject(network, entry.pid), emphasis(theme));
    let ports: Vec<String> =
        network.listening().iter().map(|port| format!("{} {}", port.transport.name(), port.port)).collect();
    if !ports.is_empty() {
        text(buf, row(inner, 1), tr!(network_listens_on, ports = ports.join(", ")), theme.styles.overlay);
    }
    let body = body(rect);
    let meter = network.traffic.processes.get(&entry.pid);
    let counted = |value: u64| rate(entry.counted.then_some(value));
    let used = tiles(
        buf,
        body,
        &[
            (tr!(network_download_tile), counted(entry.rate.down)),
            (tr!(network_upload_tile), counted(entry.rate.up)),
            (tr!(network_connections_tab), entry.connections.to_string()),
            (tr!(network_since_opened_tile), meter.map_or_else(|| moved(Rate::default()), |m| moved(m.moved))),
        ],
        theme,
    );
    let rest = below(body, used);
    if !entry.counted {
        text(buf, row(rest, 0), tr!(network_bytes_uncounted), dim(theme));
        connections(network, look, below(rest, 2), buf);
        return;
    }
    let (downloads, uploads) = traffic(meter);
    let charted = CHART_HEIGHT.min(rest.height / 2);
    draw_charts(&downloads, &uploads, Rect { height: charted, ..rest }, look, buf);
    connections(network, look, below(rest, charted + 1), buf);
}

/// The selected port's or process's connections, busiest first, from where the details are
/// scrolled to.
fn connections(network: &Network, look: &Look<'_>, area: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    if area.height < 2 {
        return;
    }
    let related = network.related();
    text(buf, row(area, 0), format!("{} · {}", tr!(network_connections_tab), related.len()), heading(theme));
    if related.is_empty() {
        text(buf, row(area, 1), tr!(network_no_connections_now), dim(theme));
        return;
    }
    let columns = [
        open(Field::Remote, None),
        column(Field::State, TABLE_STATE_WIDTH, None),
        column(Field::Down, TABLE_RATE_WIDTH, None),
        column(Field::Up, TABLE_RATE_WIDTH, None),
    ];
    for (column, cell) in columns.iter().zip(cells(&columns, row(area, 1))) {
        text(buf, cell, column.field.title(), theme.styles.menu);
    }
    let shown = usize::from(area.height.saturating_sub(2));
    let top = network.details_top.min(related.len().saturating_sub(shown));
    for (offset, (socket, rate)) in related.iter().skip(top).take(shown).enumerate() {
        let line = row(area, offset as u16 + 2);
        let values = [
            socket.remote.map_or_else(String::new, |remote| endpoint(network, remote)),
            socket.state.map_or(socket.transport.name(), |state| state.name()).to_owned(),
            socket_rate(socket, rate.down),
            socket_rate(socket, rate.up),
        ];
        for (value, cell) in values.into_iter().zip(cells(&columns, line)) {
            text(buf, cell, format::truncate_end(&value, usize::from(cell.width)), theme.styles.overlay);
        }
    }
}

fn connection(network: &Network, socket: &Socket, rate: Rate, look: &Look<'_>, rect: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let title = socket.remote.map_or_else(|| page_label(network.page).to_owned(), |remote| endpoint(network, remote));
    let inner = frame(network, look, rect, &title, network.details_focused, buf);
    if inner.is_empty() {
        return;
    }
    let summary = match socket.state {
        Some(state) => format!("{} · {}", socket.transport.name(), state.name()),
        None => socket.transport.name().to_owned(),
    };
    text(buf, row(inner, 0), summary, emphasis(theme));
    text(buf, row(inner, 1), subject(network, socket.pid), theme.styles.overlay);
    let body = body(rect);
    let used = tiles(
        buf,
        body,
        &[
            (tr!(network_download_tile), socket_rate(socket, rate.down)),
            (tr!(network_upload_tile), socket_rate(socket, rate.up)),
            (tr!(network_received_tile), socket.received.map_or_else(|| super::UNKNOWN.into(), format::size)),
            (tr!(network_sent_tile), socket.sent.map_or_else(|| super::UNKNOWN.into(), format::size)),
        ],
        theme,
    );
    let rest = below(body, used);
    let labels =
        [tr!(network_local_label), tr!(network_remote_label), tr!(network_host_label), tr!(network_service_label)];
    let mut facts = facts(rest, 0, &labels, look);
    facts.add(buf, labels[0], socket.local.to_string());
    if let Some(remote) = socket.remote {
        facts.add(buf, labels[1], remote.to_string());
        facts.add(buf, labels[2], network.host(remote.ip()).unwrap_or_default());
        facts.add(buf, labels[3], service(remote.port()).or(service(socket.local.port())).unwrap_or_default());
    }
    let left = below(rest, facts.y + 1);
    if socket.received.is_none() {
        text(buf, row(left, 0), tr!(network_bytes_uncounted), dim(theme));
        return;
    }
    let (downloads, uploads) = traffic(network.traffic.sockets.get(&socket.key()));
    draw_charts(&downloads, &uploads, left, look, buf);
}

fn interface_details(
    network: &Network,
    interface: &Interface,
    rate: Rate,
    look: &Look<'_>,
    rect: Rect,
    buf: &mut Buffer,
) {
    let theme = look.theme;
    let inner = frame(network, look, rect, &interface.name, network.details_focused, buf);
    if inner.is_empty() {
        return;
    }
    let status = if interface.up { tr!(network_active) } else { tr!(network_inactive) };
    let dot = if interface.up { UP } else { DOWN };
    text(buf, row(inner, 0), format!("{dot} {status} · {}", kind_label(interface.kind)), emphasis(theme));
    text(buf, row(inner, 1), interface.mac.clone().unwrap_or_default(), theme.styles.overlay);
    let body = body(rect);
    let used = tiles(
        buf,
        body,
        &[
            (tr!(network_download_tile), format::rate(rate.down)),
            (tr!(network_upload_tile), format::rate(rate.up)),
            (tr!(network_received_tile), format::size(interface.received)),
            (tr!(network_sent_tile), format::size(interface.sent)),
            (tr!(network_errors_tile), interface.errors.to_string()),
        ],
        theme,
    );
    let rest = below(body, used);
    let labels = [tr!(network_addresses_label), tr!(network_mtu_label), tr!(network_packets_label)];
    let mut facts = facts(rest, 0, &labels, look);
    let addresses: Vec<String> =
        interface.addresses.iter().map(|(address, prefix)| format!("{address}/{prefix}")).collect();
    facts.add(buf, labels[0], addresses.join(", "));
    facts.add(buf, labels[1], interface.mtu.map_or_else(String::new, |mtu| mtu.to_string()));
    facts.add(
        buf,
        labels[2],
        tr!(network_packets_value, received = interface.packets_received, sent = interface.packets_sent),
    );
    let (downloads, uploads) = traffic(network.traffic.interfaces.get(&interface.name));
    draw_charts(&downloads, &uploads, below(rest, facts.y + 1), look, buf);
}
