//! The traffic viewer: a capture's page of processes, hosts, conversations or packets in the list
//! pane, and the selected row's details, or a packet's layers and bytes, in the other.

use super::{
    FILTER_ROW, HEADER_ROW, TABS_ROW, UNKNOWN, frame,
    list::{FilterLine, filter_line},
    spread, tab_cells,
};
use crate::{
    activity_view::{FOOTER_ROWS, body, dim, emphasis, heading, row, text},
    docker_view::tiles,
    format,
    look::Look,
};
use borg_core::{HEX_LINE_WIDTH, Network, Packet, TrafficPage, TrafficView, hex_lines, service};
use borg_i18n::{Count, count, tr};
use ratatui::{buffer::Buffer, layout::Rect, style::Style};
use std::{fmt::Write, net::IpAddr, time::Duration};
use unicode_width::UnicodeWidthStr;

const PID_WIDTH: u16 = 7;
const COUNT_WIDTH: u16 = 7;
const BYTES_WIDTH: u16 = 9;
const PROCESS_WIDTH: u16 = 16;
const PROTOCOL_WIDTH: u16 = 6;
const PROTOCOLS_WIDTH: u16 = 14;
const TIME_WIDTH: u16 = 10;
const LENGTH_WIDTH: u16 = 6;
const LABEL_GAP: usize = 2;
const COLUMN_GAP: usize = 2;
/// Seconds shown to the microsecond, as packet times usually are.
const TIME_DIGITS: usize = 6;
const SPAN_DIGITS: usize = 1;
const DURATION_DIGITS: usize = 3;
/// Panes at least this wide show where each packet comes from and goes in the list.
const ENDS_FROM: u16 = 80;
const OUTGOING: &str = "→";
const INCOMING: &str = "←";
const SEPARATOR: &str = " · ";

pub(super) fn page_label(page: TrafficPage) -> &'static str {
    match page {
        TrafficPage::Processes => tr!(network_processes_tab),
        TrafficPage::Hosts => tr!(traffic_hosts_tab),
        TrafficPage::Connections => tr!(network_connections_tab),
        TrafficPage::Packets => tr!(traffic_packets_tab),
    }
}

/// The viewer's page tabs, each with the digit that opens it.
pub(super) fn tabs(area: Rect) -> Vec<(Rect, String, TrafficPage)> {
    tab_cells(TrafficPage::ALL.map(|page| (format!("{} {}", page.key(), page_label(page)), page)), area)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Process,
    Pid,
    Hosts,
    Packets,
    Bytes,
    Host,
    Protocols,
    Remote,
    Protocol,
    Time,
    Source,
    Destination,
    Length,
    Info,
}

impl Field {
    fn title(self) -> &'static str {
        match self {
            Self::Process => tr!(network_process_column),
            Self::Pid => tr!(activity_pid),
            Self::Hosts => tr!(traffic_hosts_tab),
            Self::Packets => tr!(traffic_packets_tab),
            Self::Bytes => tr!(traffic_bytes_column),
            Self::Host => tr!(traffic_host_column),
            Self::Protocols | Self::Protocol => tr!(network_protocol_column),
            Self::Remote => tr!(network_remote_column),
            Self::Time => tr!(traffic_time_column),
            Self::Source => tr!(traffic_source_column),
            Self::Destination => tr!(traffic_destination_column),
            Self::Length => tr!(traffic_length_column),
            Self::Info => tr!(traffic_info_column),
        }
    }
}

/// A list column, with its width or none for one that shares what is left.
struct Column {
    field: Field,
    width: Option<u16>,
}

const fn fixed(field: Field, width: u16) -> Column {
    Column { field, width: Some(width) }
}

const fn open(field: Field) -> Column {
    Column { field, width: None }
}

fn columns(page: TrafficPage, width: u16) -> Vec<Column> {
    match page {
        TrafficPage::Processes => vec![
            open(Field::Process),
            fixed(Field::Pid, PID_WIDTH),
            fixed(Field::Hosts, COUNT_WIDTH),
            fixed(Field::Packets, COUNT_WIDTH),
            fixed(Field::Bytes, BYTES_WIDTH),
        ],
        TrafficPage::Hosts => vec![
            open(Field::Host),
            fixed(Field::Protocols, PROTOCOLS_WIDTH),
            fixed(Field::Packets, COUNT_WIDTH),
            fixed(Field::Bytes, BYTES_WIDTH),
        ],
        TrafficPage::Connections => vec![
            fixed(Field::Process, PROCESS_WIDTH),
            open(Field::Remote),
            fixed(Field::Protocol, PROTOCOL_WIDTH),
            fixed(Field::Packets, COUNT_WIDTH),
            fixed(Field::Bytes, BYTES_WIDTH),
        ],
        TrafficPage::Packets => [
            Some(fixed(Field::Time, TIME_WIDTH)),
            (width >= ENDS_FROM).then_some(open(Field::Source)),
            (width >= ENDS_FROM).then_some(open(Field::Destination)),
            Some(fixed(Field::Protocol, PROTOCOL_WIDTH)),
            Some(fixed(Field::Length, LENGTH_WIDTH)),
            Some(open(Field::Info)),
        ]
        .into_iter()
        .flatten()
        .collect(),
    }
}

fn cells(columns: &[Column], area: Rect) -> Vec<Rect> {
    spread(&columns.iter().map(|column| column.width).collect::<Vec<_>>(), area)
}

pub(super) fn draw(
    network: &Network,
    view: &TrafficView,
    look: &Look<'_>,
    notice: Option<&str>,
    panes: [Rect; 2],
    buf: &mut Buffer,
) {
    let [list, details] = panes;
    if !list.is_empty() {
        draw_list(network, view, look, notice, list, buf);
    }
    if !details.is_empty() {
        draw_details(network, view, look, details, buf);
    }
}

/// Where the capture came from: its file's name, or the interface it ran on.
fn source(view: &TrafficView) -> String {
    let file = view.path.as_ref().and_then(|path| path.file_name()).map(|name| name.to_string_lossy().into_owned());
    file.or_else(|| view.interface.clone()).unwrap_or_default()
}

fn draw_list(
    network: &Network,
    view: &TrafficView,
    look: &Look<'_>,
    notice: Option<&str>,
    rect: Rect,
    buf: &mut Buffer,
) {
    let theme = look.theme;
    let styles = &theme.styles;
    let focused = !network.details_focused;
    let title = format!("{}{SEPARATOR}{}", tr!(traffic_title), source(view));
    let inner = frame(network, look, rect, &title, focused, buf);
    if inner.is_empty() {
        return;
    }
    for (cell, label, page) in tabs(row(inner, TABS_ROW)) {
        let style = if page == view.page { styles.cursor } else { styles.menu };
        buf.set_style(cell, style);
        text(buf, Rect { x: cell.x + 1, width: cell.width.saturating_sub(1), ..cell }, label, style);
    }
    let line = FilterLine {
        filter: &view.filter,
        filtering: view.filtering,
        shown: view.len(view.page),
        total: view.totals[view.page.index()],
        placeholder: tr!(traffic_filter_placeholder),
    };
    filter_line(&line, look, row(inner, FILTER_ROW), buf);
    let header = row(inner, HEADER_ROW);
    let columns = columns(view.page, header.width);
    for (column, cell) in columns.iter().zip(cells(&columns, header)) {
        text(buf, cell, column.field.title(), styles.menu);
    }
    let body = body(rect);
    if view.len(view.page) == 0 {
        let note = if view.capture.packets.is_empty() { tr!(traffic_no_packets) } else { tr!(network_no_matches) };
        text(buf, row(body, 0), note, dim(theme));
    }
    let start = view.list_start(usize::from(body.height));
    let end = view.len(view.page).min(start + usize::from(body.height));
    for (offset, index) in (start..end).enumerate() {
        let line = row(body, offset as u16);
        let base = match (index == view.cursor(), focused) {
            (true, true) => styles.cursor,
            (true, false) => styles.cursor_inactive,
            _ => styles.overlay,
        };
        buf.set_style(line, base);
        for (column, cell) in columns.iter().zip(cells(&columns, line)) {
            let value = value(view, index, column.field);
            text(buf, cell, format::truncate_end(&value, usize::from(cell.width)), base);
        }
    }
    let footer = Rect {
        y: inner.bottom().saturating_sub(FOOTER_ROWS.min(inner.height)),
        height: FOOTER_ROWS.min(inner.height),
        ..inner
    };
    let summary = notice.map_or_else(|| summary(view), str::to_owned);
    text(buf, row(footer, 0), summary, dim(theme));
}

/// The capture in a line: its packets, bytes and length, and what Borg left out of it.
fn summary(view: &TrafficView) -> String {
    let capture = &view.capture;
    let mut line = tr!(
        traffic_summary,
        packets = count(capture.packets.len(), Count::Packet),
        size = format::size(capture.bytes()),
        seconds = seconds(capture.span(), SPAN_DIGITS)
    );
    if capture.skipped > 0 {
        let _ = write!(line, "{SEPARATOR}{}", tr!(traffic_skipped, packets = count(capture.skipped, Count::Packet)));
    }
    if capture.cut {
        let _ = write!(line, "{SEPARATOR}{}", tr!(traffic_cut));
    }
    line
}

/// A length of time in seconds, to `digits` decimals.
fn seconds(span: Duration, digits: usize) -> String {
    tr!(capture_seconds, seconds = format!("{:.digits$}", span.as_secs_f64()))
}

/// Seconds since the capture's first packet.
fn elapsed(view: &TrafficView, stamp: Duration) -> String {
    format!("{:.TIME_DIGITS$}", stamp.saturating_sub(view.capture.start()).as_secs_f64())
}

fn pid(pid: Option<u32>) -> String {
    pid.map_or_else(|| UNKNOWN.into(), |pid| pid.to_string())
}

fn value(view: &TrafficView, index: usize, field: Field) -> String {
    match view.page {
        TrafficPage::Processes => {
            let talker = &view.talkers[index];
            match field {
                Field::Process => view.name(talker.pid),
                Field::Pid => pid(talker.pid),
                Field::Hosts => talker.hosts.to_string(),
                Field::Packets => talker.packets.to_string(),
                _ => format::size(talker.bytes),
            }
        }
        TrafficPage::Hosts => {
            let host = &view.host_rows[index];
            match field {
                Field::Host => host_label(view, host.address),
                Field::Protocols => host.protocols.join(", "),
                Field::Packets => host.packets.to_string(),
                _ => format::size(host.bytes),
            }
        }
        TrafficPage::Connections => {
            let flow = view.flows[index];
            let (ends, conversation) = (view.ends[flow], &view.capture.flows[flow]);
            match field {
                Field::Process => view.name(ends.pid),
                Field::Remote => view.endpoint(ends.remote),
                Field::Protocol => TrafficView::protocol(conversation).to_owned(),
                Field::Packets => conversation.packets.len().to_string(),
                _ => format::size(conversation.bytes()),
            }
        }
        TrafficPage::Packets => {
            let packet = &view.capture.packets[view.packets[index]];
            let ip = packet.layers.ip;
            match field {
                Field::Time => elapsed(view, packet.stamp),
                Field::Source => ip.map_or_else(String::new, |ip| host_label(view, ip.source)),
                Field::Destination => ip.map_or_else(String::new, |ip| host_label(view, ip.destination)),
                Field::Protocol => packet.layers.protocol().to_owned(),
                Field::Length => packet.length.to_string(),
                _ => packet.summary(),
            }
        }
    }
}

/// A host by its name when known, or else its address.
fn host_label(view: &TrafficView, address: IpAddr) -> String {
    view.host(address).map_or_else(|| address.to_string(), str::to_owned)
}

fn draw_details(network: &Network, view: &TrafficView, look: &Look<'_>, rect: Rect, buf: &mut Buffer) {
    let focused = network.details_focused;
    let title = match view.page {
        TrafficPage::Processes => view.talker().map(|talker| view.name(talker.pid)),
        TrafficPage::Hosts => view.host_row().map(|host| host_label(view, host.address)),
        TrafficPage::Connections => view.flow().map(|(index, _)| view.endpoint(view.ends[index].remote)),
        TrafficPage::Packets => view.packet().map(|(index, _)| tr!(traffic_packet_title, number = index + 1)),
    };
    let inner = frame(network, look, rect, title.as_deref().unwrap_or(page_label(view.page)), focused, buf);
    if inner.is_empty() || title.is_none() {
        return;
    }
    let lines = match view.page {
        TrafficPage::Processes => talker_lines(view),
        TrafficPage::Hosts => host_lines(view),
        TrafficPage::Connections => flow_lines(view),
        TrafficPage::Packets => packet_lines(view, usize::from(inner.width) >= HEX_LINE_WIDTH),
    };
    let labels = lines.iter().map(|line| line.label.width()).max().unwrap_or_default() + LABEL_GAP;
    let theme = look.theme;
    let mut top = 0;
    let (summary, rest) = lines.split_at(lines.iter().position(|line| line.scrolls).unwrap_or(lines.len()));
    for line in summary {
        if top >= inner.height {
            return;
        }
        draw_line(line, labels, look, row(inner, top), buf);
        top += 1;
    }
    let area = Rect { y: inner.y + top, height: inner.height.saturating_sub(top), ..inner };
    let used = if let Some(tiles_row) = tiles_of(view) {
        tiles(buf, area, &tiles_row.iter().map(|(label, value)| (*label, value.clone())).collect::<Vec<_>>(), theme)
    } else {
        0
    };
    let area = Rect { y: area.y + used.min(area.height), height: area.height.saturating_sub(used), ..area };
    let skip = view.details_top.min(rest.len().saturating_sub(1));
    for (offset, line) in rest.iter().skip(skip).take(usize::from(area.height)).enumerate() {
        draw_line(line, labels, look, row(area, offset as u16), buf);
    }
}

/// A line of the details: an optional label column and a value, in one of a few tones.
struct Detail {
    label: String,
    value: String,
    tone: Tone,
    /// Lines from here on scroll with the details.
    scrolls: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Strong,
    Plain,
    Quiet,
    Heading,
    Code,
}

fn detail(label: impl Into<String>, value: impl Into<String>, tone: Tone) -> Detail {
    Detail { label: label.into(), value: value.into(), tone, scrolls: false }
}

fn scrolling(label: impl Into<String>, value: impl Into<String>, tone: Tone) -> Detail {
    Detail { scrolls: true, ..detail(label, value, tone) }
}

fn style(tone: Tone, look: &Look<'_>) -> Style {
    let theme = look.theme;
    match tone {
        Tone::Strong => emphasis(theme),
        Tone::Plain | Tone::Code => theme.styles.overlay,
        Tone::Quiet => dim(theme),
        Tone::Heading => heading(theme),
    }
}

/// A line, with its label in a column `labels` cells wide.
fn draw_line(line: &Detail, labels: usize, look: &Look<'_>, area: Rect, buf: &mut Buffer) {
    if line.label.is_empty() {
        text(buf, area, format::truncate_end(&line.value, usize::from(area.width)), style(line.tone, look));
        return;
    }
    let label = labels.min(usize::from(area.width)) as u16;
    text(buf, Rect { width: label, ..area }, &line.label, dim(look.theme));
    let rest = Rect { x: area.x + label, width: area.width - label, ..area };
    text(buf, rest, format::truncate_end(&line.value, usize::from(rest.width)), style(line.tone, look));
}

/// The figures across the top of a row's details.
fn tiles_of(view: &TrafficView) -> Option<Vec<(&'static str, String)>> {
    let figures = |packets: usize, bytes: u64| {
        vec![(tr!(traffic_packets_tab), packets.to_string()), (tr!(traffic_bytes_column), format::size(bytes))]
    };
    match view.page {
        TrafficPage::Processes => view.talker().map(|talker| {
            let mut tiles = figures(talker.packets, talker.bytes);
            tiles.push((tr!(traffic_hosts_tab), talker.hosts.to_string()));
            tiles.push((tr!(network_connections_tab), talker.flows.len().to_string()));
            tiles
        }),
        TrafficPage::Hosts => view.host_row().map(|host| {
            let mut tiles = figures(host.packets, host.bytes);
            tiles.push((tr!(network_connections_tab), host.flows.len().to_string()));
            tiles
        }),
        TrafficPage::Connections => view.flow().map(|(index, flow)| {
            let (sent, received) = view.sent_received(index);
            vec![
                (tr!(traffic_packets_tab), flow.packets.len().to_string()),
                (tr!(network_sent_tile), format::size(sent)),
                (tr!(network_received_tile), format::size(received)),
                (tr!(traffic_duration_tile), seconds(flow.duration(), DURATION_DIGITS)),
            ]
        }),
        TrafficPage::Packets => None,
    }
}

/// The subject line under a row's title: its process, as the network screen names it.
fn subject(view: &TrafficView, pid: Option<u32>) -> String {
    match pid {
        Some(pid) => tr!(process_control_subject, name = view.name(Some(pid)), pid = pid),
        None => view.name(None),
    }
}

/// A row's conversations, busiest first, as lines that scroll with their values in columns.
fn conversation_lines(view: &TrafficView, flows: &[usize], with_process: bool) -> Vec<Detail> {
    let mut flows = flows.to_vec();
    flows.sort_by_key(|&flow| std::cmp::Reverse(view.capture.flows[flow].bytes()));
    let rows: Vec<Vec<String>> = flows
        .iter()
        .map(|&index| {
            let (flow, ends) = (&view.capture.flows[index], view.ends[index]);
            let process = with_process.then(|| view.name(ends.pid));
            process
                .into_iter()
                .chain([
                    view.endpoint(ends.remote),
                    TrafficView::protocol(flow).to_owned(),
                    count(flow.packets.len(), Count::Packet),
                    format::size(flow.bytes()),
                ])
                .collect()
        })
        .collect();
    let heading = format!("{}{SEPARATOR}{}", tr!(network_connections_tab), flows.len());
    let mut lines = vec![scrolling("", heading, Tone::Heading)];
    lines.extend(aligned(&rows).into_iter().map(|line| scrolling("", line, Tone::Plain)));
    lines
}

/// Rows of cells as lines, each column as wide as its widest cell and `COLUMN_GAP` apart.
fn aligned(rows: &[Vec<String>]) -> Vec<String> {
    let columns = rows.iter().map(Vec::len).max().unwrap_or_default();
    let widths: Vec<usize> = (0..columns)
        .map(|column| rows.iter().filter_map(|cells| cells.get(column)).map(|cell| cell.width()).max().unwrap_or(0))
        .collect();
    rows.iter()
        .map(|cells| {
            let last = cells.len().saturating_sub(1);
            let padded = cells.iter().enumerate().map(|(column, cell)| {
                if column == last { cell.clone() } else { format::pad_end(cell, widths[column] + COLUMN_GAP) }
            });
            padded.collect()
        })
        .collect()
}

fn talker_lines(view: &TrafficView) -> Vec<Detail> {
    let Some(talker) = view.talker() else { return Vec::new() };
    let mut lines = vec![detail("", subject(view, talker.pid), Tone::Strong), detail("", "", Tone::Plain)];
    lines.extend(conversation_lines(view, &talker.flows, false));
    lines
}

fn host_lines(view: &TrafficView) -> Vec<Detail> {
    let Some(host) = view.host_row() else { return Vec::new() };
    let processes: Vec<String> = host.pids.iter().map(|pid| view.name(*pid)).collect();
    let mut lines =
        vec![detail("", host.address.to_string(), Tone::Strong), detail("", processes.join(", "), Tone::Plain)];
    lines.extend(conversation_lines(view, &host.flows, true));
    lines
}

fn flow_lines(view: &TrafficView) -> Vec<Detail> {
    let Some((index, flow)) = view.flow() else { return Vec::new() };
    let ends = view.ends[index];
    let label = |text: &'static str| text.to_owned();
    let mut lines = vec![
        detail("", format!("{}{SEPARATOR}{}", flow.kind.name(), TrafficView::protocol(flow)), Tone::Strong),
        detail("", subject(view, ends.pid), Tone::Plain),
    ];
    lines.push(scrolling(label(tr!(network_local_label)), ends.local.to_string(), Tone::Plain));
    lines.push(scrolling(label(tr!(network_remote_label)), ends.remote.to_string(), Tone::Plain));
    if let Some(host) = view.host(ends.remote.ip()) {
        lines.push(scrolling(label(tr!(network_host_label)), host, Tone::Plain));
    }
    if let Some(name) = &flow.server_name {
        lines.push(scrolling(label(tr!(traffic_asked_label)), name.clone(), Tone::Plain));
    }
    if let Some(known) = service(ends.remote.port()).or(service(ends.local.port())) {
        lines.push(scrolling(label(tr!(network_service_label)), known, Tone::Plain));
    }
    lines.push(scrolling(
        label(tr!(traffic_started_label)),
        tr!(capture_seconds, seconds = elapsed(view, flow.first)),
        Tone::Plain,
    ));
    lines.push(scrolling("", "", Tone::Plain));
    lines.push(scrolling("", format!("{}{SEPARATOR}{}", tr!(traffic_packets_tab), flow.packets.len()), Tone::Heading));
    lines.extend(flow.packets.iter().map(|&number| {
        let packet = &view.capture.packets[number];
        let outgoing = sender(packet).is_some_and(|source| source == ends.local.ip());
        let arrow = if outgoing { OUTGOING } else { INCOMING };
        scrolling(
            "",
            format!("{}  {arrow} {:>5}  {}", elapsed(view, packet.stamp), packet.length, packet.summary()),
            Tone::Plain,
        )
    }));
    lines
}

fn sender(packet: &Packet) -> Option<IpAddr> {
    packet.layers.ip.map(|ip| ip.source)
}

/// A packet's layers and its bytes, with the bytes as text beside them when `with_text`.
fn packet_lines(view: &TrafficView, with_text: bool) -> Vec<Detail> {
    let Some((_, packet)) = view.packet() else { return Vec::new() };
    let layers = &packet.layers;
    let mut lines = vec![
        detail("", format!("{}  {}", layers.protocol(), packet.summary()), Tone::Strong),
        detail(
            "",
            tr!(
                traffic_packet_size,
                seconds = elapsed(view, packet.stamp),
                length = packet.length,
                captured = packet.data.len()
            ),
            Tone::Plain,
        ),
    ];
    if let Some(pid) = view.packet_pid(packet) {
        lines.push(detail("", subject(view, Some(pid)), Tone::Quiet));
    }
    lines.push(scrolling("", "", Tone::Plain));
    if let Some(interface) = &packet.interface {
        lines.push(scrolling(tr!(network_interface_column), interface.clone(), Tone::Plain));
    }
    lines.extend(packet.layer_rows().into_iter().map(|(label, value)| scrolling(label, value, Tone::Plain)));
    lines.push(scrolling("", "", Tone::Plain));
    lines.push(scrolling("", tr!(traffic_bytes_column), Tone::Heading));
    lines.extend(hex_lines(&packet.data, with_text).into_iter().map(|line| scrolling("", line, Tone::Code)));
    lines
}

/// The status line: who the selected row belongs to and where it goes.
pub(super) fn status(view: &TrafficView) -> String {
    match view.page {
        TrafficPage::Processes => view.talker().map_or_else(String::new, |talker| subject(view, talker.pid)),
        TrafficPage::Hosts => view.host_row().map_or_else(String::new, |host| match view.host(host.address) {
            Some(name) => format!("{}{SEPARATOR}{name}", host.address),
            None => host.address.to_string(),
        }),
        TrafficPage::Connections => view.flow().map_or_else(String::new, |(index, _)| {
            let ends = view.ends[index];
            format!("{}{SEPARATOR}{} → {}", subject(view, ends.pid), ends.local, ends.remote)
        }),
        TrafficPage::Packets => view.packet().map_or_else(String::new, |(_, packet)| {
            format!("{}{SEPARATOR}{}", packet.layers.protocol(), packet.summary())
        }),
    }
}
