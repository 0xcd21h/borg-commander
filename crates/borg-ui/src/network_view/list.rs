//! The list pane: the page tabs, the filter, the column titles and the page's rows, with the
//! machine's traffic in the footer.

use super::{
    DOWN, FILTER_ROW, Field, HEADER_ROW, TABS_ROW, UNKNOWN, UP, cells, columns, endpoint, first_address,
    neighbor_label, neighbor_state, open_ports, ping_time, rate, seen, socket_rate, tabs,
};
use crate::{
    activity_view::{FOOTER_ROWS, body, dim, heading, readable, row, text, wrap},
    format,
    look::Look,
    spinner::Spinner,
};
use borg_core::{
    AppState, CaptureLimit, CaptureRequest, Exposure, Network, NetworkPage, PortRow, Presence, Recording, ScanRun,
    exposure_label, kind_label, neighbor_notes,
};
use borg_i18n::tr;
use jiff::Zoned;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use std::time::Instant;
use unicode_width::UnicodeWidthStr;

const SORTED_DOWN: &str = " ↓";
const SORTED_UP: &str = " ↑";
const SEPARATOR: &str = " · ";
const LIST_SEPARATOR: &str = ", ";
/// How far ahead a capture's limit is told in seconds; further ahead it is told in minutes.
const SECONDS_SHOWN_UNTIL: u64 = 120;
const SECONDS_PER_MINUTE: u64 = 60;

pub(super) fn draw_list(
    state: &AppState,
    look: &Look<'_>,
    rect: Rect,
    clock: &Zoned,
    spinner: Option<Spinner>,
    now: Instant,
    buf: &mut Buffer,
) {
    let network = &state.network;
    let theme = look.theme;
    let styles = &theme.styles;
    let focused = !network.details_focused;
    let status = if network.paused { tr!(activity_paused) } else { tr!(activity_live) };
    let inner = crate::results_view::pane_frame(
        network.expanded,
        look,
        rect,
        &format!("{} · {status}", tr!(network_title)),
        focused,
        buf,
    );
    if inner.is_empty() {
        return;
    }
    for (cell, label, page) in tabs(row(inner, TABS_ROW)) {
        let style = if page == network.page { styles.cursor } else { styles.menu };
        buf.set_style(cell, style);
        text(buf, Rect { x: cell.x + 1, width: cell.width.saturating_sub(1), ..cell }, label, style);
    }
    let line = FilterLine {
        filter: &network.filter,
        filtering: network.filtering,
        shown: network.len(network.page),
        total: network.totals[network.page.index()],
        placeholder: if network.page == NetworkPage::Neighbors {
            tr!(network_neighbors_filter_placeholder)
        } else {
            tr!(network_filter_placeholder)
        },
        esc_clears: false,
    };
    filter_line(&line, look, row(inner, FILTER_ROW), buf);
    let header = row(inner, HEADER_ROW);
    let columns = columns(network.page, header.width);
    let order = network.order();
    let mut marked = false;
    for (column, cell) in columns.iter().zip(cells(&columns, header)) {
        let sorted = column.sort == Some(order.sort) && !marked;
        marked |= sorted;
        let arrow = match (sorted, order.descending) {
            (false, _) => "",
            (true, true) => SORTED_DOWN,
            (true, false) => SORTED_UP,
        };
        text(buf, cell, format!("{}{arrow}", column.field.title()), styles.menu);
    }
    let body = body(rect);
    if let Some(error) = &network.error {
        for (offset, line) in wrap(error, usize::from(body.width)).into_iter().enumerate() {
            text(buf, row(body, offset as u16), line, styles.notice);
        }
    } else if !network.loaded {
        if let Some(spinner) = spinner {
            spinner.draw_centered(tr!(network_loading), look, dim(theme), body, buf);
        }
    } else if network.len(network.page) == 0 {
        match (&network.neighborhood.error, network.page) {
            (Some(error), NetworkPage::Neighbors) => {
                let note = tr!(network_cannot_read, error = error);
                for (offset, line) in wrap(&note, usize::from(body.width)).into_iter().enumerate() {
                    text(buf, row(body, offset as u16), line, styles.notice);
                }
            }
            _ if network.filter.is_empty() => text(buf, row(body, 0), empty_note(network.page), dim(theme)),
            _ => text(buf, row(body, 0), tr!(network_no_matches), dim(theme)),
        }
    }
    let start = network.list_start(usize::from(body.height));
    let end = network.len(network.page).min(start + usize::from(body.height));
    for (offset, index) in (start..end).enumerate() {
        let line = row(body, offset as u16);
        let base = match (index == network.cursor(), focused) {
            (true, true) => styles.cursor,
            (true, false) => styles.cursor_inactive,
            _ => styles.overlay,
        };
        buf.set_style(line, base);
        let selected = index == network.cursor();
        for (column, cell) in columns.iter().zip(cells(&columns, line)) {
            let (value, style) = value(network, index, column.field, look, clock, base, selected);
            Line::from(Span::styled(format::truncate_end(&value, usize::from(cell.width)), style)).render(cell, buf);
        }
    }
    let footer = Rect {
        y: inner.bottom().saturating_sub(FOOTER_ROWS.min(inner.height)),
        height: FOOTER_ROWS.min(inner.height),
        ..inner
    };
    let total = network.total();
    let summary = neighborhood_summary(network)
        .filter(|_| network.page == NetworkPage::Neighbors)
        .unwrap_or_else(|| tr!(network_total, down = format::rate(total.down), up = format::rate(total.up)));
    let note = state.notice.clone().filter(|_| network.loaded);
    match (note, &network.scan, &network.recording) {
        (Some(note), _, _) => text(buf, row(footer, 0), note, dim(theme)),
        (None, Some(scan), _) => text(buf, row(footer, 0), scan_note(scan), styles.notice),
        (None, None, Some(recording)) => text(buf, row(footer, 0), recording_note(recording, now), styles.notice),
        (None, None, None) => text(buf, row(footer, 0), summary, dim(theme)),
    }
}

/// How far a subnet scan got, and how many hosts answered.
fn scan_note(scan: &ScanRun) -> String {
    if scan.stopping {
        return tr!(network_scan_stopping, subnet = scan.subnet);
    }
    tr!(network_scanning, subnet = scan.subnet, done = scan.done, total = scan.total, answered = scan.answered)
}

/// How a capture is going: where it runs, what it has so far, the time it has left, and how
/// it keeps TLS keys.
fn recording_note(recording: &Recording, now: Instant) -> String {
    if recording.stopping {
        return tr!(capture_stopping).into();
    }
    if recording.waiting {
        return tr!(capture_waiting, interface = recording.request.interface);
    }
    let running = tr!(
        capture_running,
        interface = recording.request.interface,
        packets = format::count(recording.packets, format::Count::Packet),
        size = format::size(recording.bytes)
    );
    let tls_keys = match (&recording.proxy, &recording.key_log) {
        (Some(proxy), _) => Some(tr!(capture_proxy_running, address = proxy.address)),
        (None, Some(_)) => Some(tr!(capture_tls_field).to_owned()),
        (None, None) => None,
    };
    let left = recording.left(now).map(|seconds| tr!(capture_left, seconds = seconds));
    let ahead = recording.limit_ahead(now).map(|(limit, seconds)| limit_ahead(&recording.request, limit, seconds));
    [Some(running), left, ahead, tls_keys].into_iter().flatten().collect::<Vec<_>>().join(SEPARATOR)
}

/// How soon `limit` stops the capture: in seconds for the last couple of minutes, else in
/// whole minutes.
fn limit_ahead(request: &CaptureRequest, limit: CaptureLimit, seconds: u64) -> String {
    let limit = limit.label(request.keep_free_mib);
    if seconds < SECONDS_SHOWN_UNTIL {
        return tr!(capture_ahead_seconds, seconds = seconds, limit = limit);
    }
    tr!(capture_ahead_minutes, minutes = seconds.div_ceil(SECONDS_PER_MINUTE), limit = limit)
}

fn empty_note(page: NetworkPage) -> &'static str {
    match page {
        NetworkPage::Ports => tr!(network_no_ports),
        NetworkPage::Processes => tr!(network_no_processes),
        NetworkPage::Connections => tr!(network_no_connections),
        NetworkPage::Interfaces => tr!(network_no_interfaces),
        NetworkPage::Neighbors => tr!(network_no_neighbors),
    }
}

fn neighborhood_summary(network: &Network) -> Option<String> {
    let neighborhood = &network.neighborhood;
    let gateway = neighborhood.first_gateway().map(|gateway| tr!(network_gateway_footer, gateway = gateway.address));
    let servers: Vec<String> = neighborhood.dns.iter().map(ToString::to_string).collect();
    let dns = (!servers.is_empty()).then(|| tr!(network_dns_footer, servers = servers.join(LIST_SEPARATOR)));
    let parts: Vec<String> = [gateway, dns].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(SEPARATOR))
}

/// Who can reach a port, as the list shows it: in words, or the address it is bound to.
pub(super) fn reach(row: &PortRow) -> String {
    match (row.exposure, row.addresses.as_slice()) {
        (Exposure::Addresses, [first, rest @ ..]) if !rest.is_empty() => format!("{first} +{}", rest.len()),
        (Exposure::Addresses, [only]) => only.to_string(),
        (exposure, _) => exposure_label(exposure).to_owned(),
    }
}

/// What the page's row at `index` shows in `field`, and its style.
fn value(
    network: &Network,
    index: usize,
    field: Field,
    look: &Look<'_>,
    clock: &Zoned,
    base: Style,
    selected: bool,
) -> (String, Style) {
    let theme = look.theme;
    let styles = &theme.styles;
    let tone = |color| if selected { base } else { readable(theme, color).map_or(base, |color| base.fg(color)) };
    let quiet = if selected { base } else { dim(theme) };
    let pid = |pid: Option<u32>| pid.map_or_else(|| UNKNOWN.into(), |pid| pid.to_string());
    match network.page {
        NetworkPage::Ports => {
            let row = &network.ports[index];
            match field {
                Field::Port => (row.port.to_string(), if selected { base } else { heading(theme) }),
                Field::Protocol => (row.transport.name().to_owned(), base),
                Field::Reach => match row.exposure {
                    Exposure::Everywhere => (reach(row), tone(styles.marked.fg)),
                    Exposure::ThisMachine => (reach(row), quiet),
                    Exposure::Addresses => (reach(row), base),
                },
                Field::Process => (network.name(row.pid), base),
                Field::Pid => (pid(row.pid), base),
                _ => (row.connections.to_string(), base),
            }
        }
        NetworkPage::Processes => {
            let row = &network.processes[index];
            match field {
                Field::Process => (network.name(row.pid), base),
                Field::Pid => (pid(row.pid), base),
                Field::Down => (rate(row.counted.then_some(row.rate.down)), base),
                Field::Up => (rate(row.counted.then_some(row.rate.up)), base),
                _ => (row.connections.to_string(), base),
            }
        }
        NetworkPage::Connections => {
            let row = network.connections[index];
            let socket = &network.sample.sockets[row.socket];
            match field {
                Field::Process => (network.name(socket.pid), base),
                Field::Remote => (socket.remote.map_or_else(String::new, |remote| endpoint(network, remote)), base),
                Field::Protocol => (socket.transport.name().to_owned(), base),
                Field::State => (socket.state.map_or("", |state| state.name()).to_owned(), quiet),
                Field::Down => (socket_rate(socket, row.rate.down), base),
                _ => (socket_rate(socket, row.rate.up), base),
            }
        }
        NetworkPage::Interfaces => {
            let row = network.interfaces[index];
            let interface = &network.sample.interfaces[row.interface];
            match field {
                Field::Interface => {
                    let dot = if interface.up { UP } else { DOWN };
                    let style = if interface.up { tone(styles.executable.fg) } else { quiet };
                    (format!("{dot} {}", interface.name), style.add_modifier(Modifier::BOLD))
                }
                Field::Kind => (kind_label(interface.kind).to_owned(), quiet),
                Field::Address => {
                    (first_address(&interface.addresses).map_or_else(String::new, |a| a.to_string()), base)
                }
                Field::Down => (format::rate(row.rate.down), base),
                _ => (format::rate(row.rate.up), base),
            }
        }
        NetworkPage::Neighbors => {
            let row = network.neighbors[index];
            let neighbor = &network.neighborhood.neighbors[row.neighbor];
            let warned = row.shared || row.changed.is_some();
            let gone = match row.presence {
                Presence::Offline(at) => Some(at),
                _ => None,
            };
            let present = neighbor.mac.is_some() || matches!(row.presence, Presence::Answered);
            let style = match (warned, present && gone.is_none(), row.gateway) {
                (true, _, _) => tone(styles.notice.fg),
                (false, false, _) => quiet,
                (false, true, true) if !selected => heading(theme),
                _ => base,
            };
            let probe = network.probes.get(&neighbor.address).map(|probed| &probed.probe);
            match field {
                Field::Address => (neighbor.address.to_string(), style),
                Field::Mac => (neighbor.mac.map_or_else(String::new, |mac| mac.to_string()), style),
                Field::Vendor => (neighbor.mac.and_then(borg_core::vendor).unwrap_or_default().to_owned(), quiet),
                Field::Name => (neighbor_label(network, neighbor), style),
                Field::Host => (network.host(neighbor.address).unwrap_or(UNKNOWN).to_owned(), style),
                Field::State => (neighbor_state(neighbor, row).to_owned(), quiet),
                Field::Ports => (open_ports(network, neighbor).unwrap_or_default(), style),
                Field::Ping => (probe.and_then(|probe| probe.ping).map(ping_time).unwrap_or_default(), quiet),
                _ => {
                    let when = gone.map(|at| tr!(network_seen_note, when = seen(at, clock)));
                    let notes = when.into_iter().chain(neighbor_notes(&row, neighbor).into_iter().map(str::to_owned));
                    (notes.collect::<Vec<_>>().join(LIST_SEPARATOR), if warned { style } else { quiet })
                }
            }
        }
    }
}

/// What the filter row shows: the filter, whether it is typed, and the rows it keeps.
pub(crate) struct FilterLine<'a> {
    pub(crate) filter: &'a str,
    pub(crate) filtering: bool,
    pub(crate) shown: usize,
    pub(crate) total: usize,
    pub(crate) placeholder: &'a str,
    pub(crate) esc_clears: bool,
}

/// The filter: an input with a cursor while typing, a badge while a filter applies, and a
/// hint otherwise.
pub(crate) fn filter_line(line: &FilterLine<'_>, look: &Look<'_>, area: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let styles = &theme.styles;
    let label = tr!(activity_filter_label);
    let (shown, total) = (line.shown, line.total);
    if line.filtering {
        buf.set_style(area, styles.input);
        let typed = format!(" {}", line.filter);
        Line::from(vec![Span::styled(label, styles.cursor), Span::styled(typed.clone(), styles.input)])
            .render(area, buf);
        let x = area.x.saturating_add((label.width() + typed.width()) as u16);
        if x < area.right() {
            buf[(x, area.y)].set_style(styles.input.add_modifier(Modifier::REVERSED));
        }
        let hint = tr!(activity_filter_typing, shown = shown, total = total);
        let start = area.right().saturating_sub(hint.width() as u16 + 1).max(x.saturating_add(2));
        text(buf, Rect { x: start, width: area.right().saturating_sub(start), ..area }, hint, styles.input);
    } else if !line.filter.is_empty() {
        let hint = if line.esc_clears {
            tr!(traffic_filter_applied, shown = shown, total = total)
        } else {
            tr!(activity_filter_applied, shown = shown, total = total)
        };
        Line::from(vec![
            Span::styled(label, styles.cursor),
            Span::styled(format!(" {} ", line.filter), heading(theme)),
            Span::styled(hint, dim(theme)),
        ])
        .render(area, buf);
    } else {
        text(buf, area, line.placeholder, dim(theme));
    }
}
