//! The network screen: a page of ports, processes, connections or interfaces in one pane, and
//! the selected row's details and traffic in the other. It shares the activity monitor's look.

use crate::{activity_view::text, format, look::Look, spinner::Spinner};
use borg_core::{AppState, Network, NetworkPage, NetworkSort, Socket};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{Block, Clear, Widget},
};
use std::net::{IpAddr, SocketAddr};
use unicode_width::UnicodeWidthStr;

mod capture;
mod controls;
mod details;
mod list;
mod traffic;

pub(crate) use capture::{draw_dialog as draw_capture_dialog, field_at as capture_field_at};
pub(crate) use controls::{keybar, panel_mouse, status};
pub(crate) use list::{FilterLine, filter_line};

const TABS_ROW: u16 = 0;
const FILTER_ROW: u16 = 1;
const HEADER_ROW: u16 = 2;
/// What a value the system does not tell shows as.
const UNKNOWN: &str = "—";
const UP: char = '●';
const DOWN: char = '○';
/// Panes at least this wide show a connection's protocol and state in the list.
const WIDE_FROM: u16 = 90;
/// Panes narrower than this leave out who can reach a port.
const REACH_FROM: u16 = 56;
const PORT_WIDTH: u16 = 6;
const PROTOCOL_WIDTH: u16 = 5;
const REACH_WIDTH: u16 = 15;
/// Seven digits, the longest PID Linux hands out, and a space before the next column.
const PID_WIDTH: u16 = 8;
const COUNT_WIDTH: u16 = 6;
const RATE_WIDTH: u16 = 9;
const STATE_WIDTH: u16 = 12;
const PROCESS_WIDTH: u16 = 16;
const INTERFACE_WIDTH: u16 = 11;
const KIND_WIDTH: u16 = 10;

pub(crate) fn draw(state: &AppState, look: &Look<'_>, panels: [Rect; 2], now: std::time::Instant, buf: &mut Buffer) {
    let network = &state.network;
    let Some(side) = network.side else { return };
    let spinner = Spinner::of(&state.busy, borg_core::Task::Network, now);
    let list = panels[side.index()];
    let details = panels[side.other().index()];
    if let Some(view) = network.viewer.as_deref() {
        traffic::draw(network, view, look, state.notice.as_deref(), [list, details], buf);
        return;
    }
    if !list.is_empty() {
        list::draw_list(state, look, list, spinner, now, buf);
    }
    if !details.is_empty() {
        details::draw_details(network, look, details, buf);
    }
}

fn frame(network: &Network, look: &Look<'_>, rect: Rect, title: &str, focused: bool, buf: &mut Buffer) -> Rect {
    let theme = look.theme;
    let styles = &theme.styles;
    Clear.render(rect, buf);
    let controls = crate::results_view::controls(rect, network.expanded);
    let room = controls.first().map_or(rect.width.saturating_sub(2), |(r, _, _)| r.x.saturating_sub(rect.x + 2));
    let block = Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(if focused { styles.border_active } else { styles.border })
        .title(Line::styled(
            format::truncate_end(&format!(" {title} "), usize::from(room)),
            if focused { styles.title_active } else { styles.title },
        ));
    let inner = block.inner(rect);
    block.render(rect, buf);
    for (r, label, _) in controls {
        text(buf, r, label, styles.menu);
    }
    inner
}

fn page_label(page: NetworkPage) -> &'static str {
    match page {
        NetworkPage::Ports => tr!(network_ports_tab),
        NetworkPage::Processes => tr!(network_processes_tab),
        NetworkPage::Connections => tr!(network_connections_tab),
        NetworkPage::Interfaces => tr!(network_interfaces_tab),
    }
}

/// The page tabs on the list pane's first row, each with the digit that opens it.
fn tabs(area: Rect) -> Vec<(Rect, String, NetworkPage)> {
    let labels = NetworkPage::ALL.map(|page| (format!("{} {}", page.key(), page_label(page)), page));
    tab_cells(labels, area)
}

/// Tabs side by side from the row's start, each as wide as its label and a cell either side.
fn tab_cells<T>(labels: impl IntoIterator<Item = (String, T)>, area: Rect) -> Vec<(Rect, String, T)> {
    let mut x = area.x;
    labels
        .into_iter()
        .map(|(label, page)| {
            let width = (label.width() as u16 + 2).min(area.right().saturating_sub(x));
            let rect = Rect { x, width, ..area };
            x += width;
            (rect, label, page)
        })
        .collect()
}

/// What a list column shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Port,
    Protocol,
    Reach,
    Process,
    Pid,
    Connections,
    Remote,
    State,
    Down,
    Up,
    Interface,
    Kind,
    Address,
}

impl Field {
    fn title(self) -> &'static str {
        match self {
            Self::Port => tr!(network_port_column),
            Self::Protocol => tr!(network_protocol_column),
            Self::Reach => tr!(network_reach_column),
            Self::Process => tr!(network_process_column),
            Self::Pid => tr!(activity_pid),
            Self::Connections => tr!(network_connections_column),
            Self::Remote => tr!(network_remote_column),
            Self::State => tr!(network_state_column),
            Self::Down => tr!(network_down_column),
            Self::Up => tr!(network_up_column),
            Self::Interface => tr!(network_interface_column),
            Self::Kind => tr!(network_kind_column),
            Self::Address => tr!(network_address_column),
        }
    }
}

/// A list column: what it shows, its width (none for the one that takes what is left), and
/// the sort a click on its title picks.
struct Column {
    field: Field,
    width: Option<u16>,
    sort: Option<NetworkSort>,
}

const fn column(field: Field, width: u16, sort: Option<NetworkSort>) -> Column {
    Column { field, width: Some(width), sort }
}

const fn open(field: Field, sort: Option<NetworkSort>) -> Column {
    Column { field, width: None, sort }
}

/// The page's columns that fit a pane `width` cells wide.
fn columns(page: NetworkPage, width: u16) -> Vec<Column> {
    let wide = width >= WIDE_FROM;
    let all = match page {
        NetworkPage::Ports => vec![
            Some(column(Field::Port, PORT_WIDTH, Some(NetworkSort::Port))),
            Some(column(Field::Protocol, PROTOCOL_WIDTH, None)),
            (width >= REACH_FROM).then_some(column(Field::Reach, REACH_WIDTH, None)),
            Some(open(Field::Process, Some(NetworkSort::Name))),
            Some(column(Field::Pid, PID_WIDTH, None)),
            Some(column(Field::Connections, COUNT_WIDTH, Some(NetworkSort::Connections))),
        ],
        NetworkPage::Processes => vec![
            Some(open(Field::Process, Some(NetworkSort::Name))),
            Some(column(Field::Pid, PID_WIDTH, None)),
            Some(column(Field::Down, RATE_WIDTH, Some(NetworkSort::Traffic))),
            Some(column(Field::Up, RATE_WIDTH, Some(NetworkSort::Traffic))),
            Some(column(Field::Connections, COUNT_WIDTH, Some(NetworkSort::Connections))),
        ],
        NetworkPage::Connections => vec![
            Some(column(Field::Process, PROCESS_WIDTH, Some(NetworkSort::Name))),
            Some(open(Field::Remote, Some(NetworkSort::Port))),
            wide.then_some(column(Field::Protocol, PROTOCOL_WIDTH, None)),
            wide.then_some(column(Field::State, STATE_WIDTH, None)),
            Some(column(Field::Down, RATE_WIDTH, Some(NetworkSort::Traffic))),
            Some(column(Field::Up, RATE_WIDTH, Some(NetworkSort::Traffic))),
        ],
        NetworkPage::Interfaces => vec![
            Some(column(Field::Interface, INTERFACE_WIDTH, Some(NetworkSort::Name))),
            Some(column(Field::Kind, KIND_WIDTH, None)),
            Some(open(Field::Address, None)),
            Some(column(Field::Down, RATE_WIDTH, Some(NetworkSort::Traffic))),
            Some(column(Field::Up, RATE_WIDTH, Some(NetworkSort::Traffic))),
        ],
    };
    all.into_iter().flatten().collect()
}

/// Each column's cell in a row.
fn cells(columns: &[Column], area: Rect) -> Vec<Rect> {
    spread(&columns.iter().map(|column| column.width).collect::<Vec<_>>(), area)
}

/// Cells of these widths side by side, one cell apart, with the open ones (no width) sharing
/// what is left.
fn spread(widths: &[Option<u16>], area: Rect) -> Vec<Rect> {
    let fixed: u16 = widths.iter().flatten().sum::<u16>() + widths.len().saturating_sub(1) as u16;
    let opened = widths.iter().filter(|width| width.is_none()).count().max(1) as u16;
    let share = area.width.saturating_sub(fixed) / opened;
    let mut x = area.x;
    widths
        .iter()
        .map(|width| {
            let width = width.unwrap_or(share);
            let cell = Rect { x, width: width.min(area.right().saturating_sub(x)), ..area };
            x = x.saturating_add(width + 1);
            cell
        })
        .collect()
}

/// Where a connection goes: the name its address resolves to, or the address, and the port.
fn endpoint(network: &Network, address: SocketAddr) -> String {
    match network.host(address.ip()) {
        Some(host) => format!("{host}:{}", address.port()),
        None => address.to_string(),
    }
}

/// A rate, or what the screen shows when the system does not count the bytes.
fn rate(value: Option<u64>) -> String {
    value.map_or_else(|| UNKNOWN.into(), format::rate)
}

fn socket_rate(socket: &Socket, value: u64) -> String {
    rate(socket.received.map(|_| value))
}

/// An interface's address to show first: IPv4 if it has one.
fn first_address(addresses: &[(IpAddr, u8)]) -> Option<IpAddr> {
    addresses.iter().find(|(address, _)| address.is_ipv4()).or(addresses.first()).map(|(address, _)| *address)
}

#[cfg(test)]
mod tests;
