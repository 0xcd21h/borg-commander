//! The list pane: the page tabs, the filter, the column titles and the page's rows, with the
//! machine's traffic in the footer.

use super::{
    DOWN, FILTER_ROW, Field, HEADER_ROW, TABS_ROW, UNKNOWN, UP, cells, columns, endpoint, first_address, frame, rate,
    socket_rate, tabs,
};
use crate::{
    activity_view::{FOOTER_ROWS, body, dim, heading, readable, row, text, wrap},
    format,
    look::Look,
    spinner::Spinner,
};
use borg_core::{AppState, Exposure, Network, NetworkPage, PortRow, exposure_label, kind_label};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use unicode_width::UnicodeWidthStr;

const SORTED_DOWN: &str = " ↓";
const SORTED_UP: &str = " ↑";

pub(super) fn draw_list(state: &AppState, look: &Look<'_>, rect: Rect, spinner: Option<Spinner>, buf: &mut Buffer) {
    let network = &state.network;
    let theme = look.theme;
    let styles = &theme.styles;
    let focused = !network.details_focused;
    let status = if network.paused { tr!(activity_paused) } else { tr!(activity_live) };
    let inner = frame(network, look, rect, &format!("{} · {status}", tr!(network_title)), focused, buf);
    if inner.is_empty() {
        return;
    }
    for (cell, label, page) in tabs(row(inner, TABS_ROW)) {
        let style = if page == network.page { styles.cursor } else { styles.menu };
        buf.set_style(cell, style);
        text(buf, Rect { x: cell.x + 1, width: cell.width.saturating_sub(1), ..cell }, label, style);
    }
    filter_row(network, look, row(inner, FILTER_ROW), buf);
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
        let note = if network.filter.is_empty() { empty_note(network.page) } else { tr!(network_no_matches) };
        text(buf, row(body, 0), note, dim(theme));
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
            let (value, style) = value(network, index, column.field, look, base, selected);
            Line::from(Span::styled(format::truncate_end(&value, usize::from(cell.width)), style)).render(cell, buf);
        }
    }
    let footer = Rect {
        y: inner.bottom().saturating_sub(FOOTER_ROWS.min(inner.height)),
        height: FOOTER_ROWS.min(inner.height),
        ..inner
    };
    let total = network.total();
    let summary = tr!(network_total, down = format::rate(total.down), up = format::rate(total.up));
    let note = state.notice.clone().filter(|_| network.loaded).unwrap_or(summary);
    text(buf, row(footer, 0), note, dim(theme));
}

fn empty_note(page: NetworkPage) -> &'static str {
    match page {
        NetworkPage::Ports => tr!(network_no_ports),
        NetworkPage::Processes => tr!(network_no_processes),
        NetworkPage::Connections => tr!(network_no_connections),
        NetworkPage::Interfaces => tr!(network_no_interfaces),
    }
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
    }
}

/// The filter: an input with a cursor while typing, a badge while a filter applies, and a
/// hint otherwise.
fn filter_row(network: &Network, look: &Look<'_>, area: Rect, buf: &mut Buffer) {
    let theme = look.theme;
    let styles = &theme.styles;
    let label = tr!(activity_filter_label);
    let shown = network.len(network.page);
    let total = network.totals[network.page.index()];
    if network.filtering {
        buf.set_style(area, styles.input);
        let typed = format!(" {}", network.filter);
        Line::from(vec![Span::styled(label, styles.cursor), Span::styled(typed.clone(), styles.input)])
            .render(area, buf);
        let x = area.x.saturating_add((label.width() + typed.width()) as u16);
        if x < area.right() {
            buf[(x, area.y)].set_style(styles.input.add_modifier(Modifier::REVERSED));
        }
        let hint = tr!(activity_filter_typing, shown = shown, total = total);
        let start = area.right().saturating_sub(hint.width() as u16 + 1).max(x.saturating_add(2));
        text(buf, Rect { x: start, width: area.right().saturating_sub(start), ..area }, hint, styles.input);
    } else if !network.filter.is_empty() {
        let hint = tr!(activity_filter_applied, shown = shown, total = total);
        Line::from(vec![
            Span::styled(label, styles.cursor),
            Span::styled(format!(" {} ", network.filter), heading(theme)),
            Span::styled(hint, dim(theme)),
        ])
        .render(area, buf);
    } else {
        text(buf, area, tr!(network_filter_placeholder), dim(theme));
    }
}
