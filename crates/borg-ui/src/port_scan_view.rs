//! The port scan's dialogs: what to scan, the notice that a scan finished, and the open ports it
//! found.

use std::time::{Duration, UNIX_EPOCH};

use crate::{
    Theme,
    dialog_view::message_lines,
    field_dialog::{self, FieldDialog},
    format,
    install_view::{Part, draw_offer, layout},
};
use borg_core::{
    AppState, Ending, FIRST_PORT, FormField, LAST_PORT, PortChoice, PortScanForm, PortScanRecord, PortScanView,
    PortsKind, ScanSpeed, port_name,
};
use borg_i18n::{Count, count, tr};
use jiff::Zoned;
use ratatui::{buffer::Buffer, layout::Rect};
use unicode_width::UnicodeWidthStr;

const SEPARATOR: &str = " · ";
const LIST_SEPARATOR: &str = ", ";
/// How many open ports the result dialog lists at once.
const RESULT_ROWS: usize = 12;
/// The column the service name starts at in the result's rows.
const PORT_COLUMN: usize = 7;

fn label(field: FormField) -> &'static str {
    match field {
        FormField::Target => tr!(port_scan_host_label),
        FormField::Ports => tr!(port_scan_ports_label),
        FormField::List => tr!(port_scan_list_label),
        FormField::Speed => tr!(port_scan_mode_label),
    }
}

/// How many ports the choice tries, without listing every one of them.
fn port_count(form: &PortScanForm) -> Option<usize> {
    match form.choice() {
        PortChoice::All => Some(usize::from(LAST_PORT - FIRST_PORT) + 1),
        choice => choice.ports().ok().map(|ports| ports.len()),
    }
}

fn ports_value(form: &PortScanForm) -> String {
    match (form.kind, port_count(form)) {
        (PortsKind::List, _) | (_, None) => form.kind.label().to_owned(),
        (kind, Some(ports)) => format!("{}{SEPARATOR}{}", kind.label(), count(ports, Count::Port)),
    }
}

fn value(form: &PortScanForm, field: FormField, width: usize) -> String {
    let selected = form.field == field;
    match field {
        FormField::Target => field_dialog::input(&form.target, selected, width),
        FormField::List => field_dialog::input(&form.list, selected, width),
        FormField::Ports => field_dialog::choice(&ports_value(form), selected),
        FormField::Speed => field_dialog::choice(form.speed.label(), selected),
    }
}

fn speed_note(speed: ScanSpeed) -> String {
    let pace = speed.pace();
    let seconds = pace.timeout.as_secs();
    match speed {
        ScanSpeed::Fast => tr!(port_scan_fast_note, count = pace.in_flight, seconds = seconds),
        ScanSpeed::Slow => tr!(
            port_scan_slow_note,
            least = pace.gap.as_millis(),
            most = (pace.gap + pace.spread).as_millis(),
            count = pace.in_flight,
            seconds = seconds
        ),
    }
}

pub(crate) fn when(at: u64, clock: &Zoned) -> String {
    format::modified(Some(UNIX_EPOCH + Duration::from_secs(at)), clock)
}

fn open_list(record: &PortScanRecord) -> String {
    if record.open.is_empty() {
        return tr!(port_scan_none).to_owned();
    }
    record.open.iter().map(ToString::to_string).collect::<Vec<_>>().join(LIST_SEPARATOR)
}

/// What the form says under its fields: what is wrong, what the mode does, and what the last
/// scan of the target found.
fn notes(state: &AppState, form: &PortScanForm, clock: &Zoned) -> Vec<(String, bool)> {
    let problem = form.error.clone().map(|error| (error, true));
    let resolving = form.resolving.as_ref().map(|name| (tr!(port_scan_resolving, name = name), false));
    let last = state
        .port_scans
        .memory
        .last(form.target.text())
        .map(|record| (tr!(port_scan_last, when = when(record.at, clock), ports = open_list(record)), false));
    [problem, resolving, Some((speed_note(form.speed), false)), last].into_iter().flatten().collect()
}

pub(crate) fn draw_form(
    state: &AppState,
    form: &PortScanForm,
    theme: &Theme,
    clock: &Zoned,
    screen: Rect,
    buf: &mut Buffer,
) -> Rect {
    let fields = form.fields();
    let dialog = FieldDialog {
        title: tr!(port_scan_form_title),
        keys: tr!(port_scan_form_keys),
        fields: fields.iter().map(|&field| (label(field), field == form.field)).collect(),
        notes: notes(state, form, clock),
    };
    field_dialog::draw(&dialog, |index, room| value(form, fields[index], room), theme, screen, buf)
}

fn summary(record: &PortScanRecord) -> String {
    tr!(port_scan_summary, open = record.open.len(), closed = record.closed, filtered = record.filtered)
}

fn scanned(record: &PortScanRecord) -> String {
    tr!(
        port_scan_done_target,
        target = record.label(),
        mode = record.speed.label(),
        ports = count(record.count, Count::Port)
    )
}

/// Why the scan stopped early, when the host seemed to limit it.
pub(crate) fn limited(record: &PortScanRecord) -> Option<String> {
    let Ending::RateLimited { port } = record.ending else { return None };
    Some(tr!(port_scan_limited, port = port, count = count(record.skipped(), Count::Port)))
}

fn done_parts(record: &PortScanRecord, width: u16) -> Vec<Part> {
    let mut parts = vec![Part::Text(message_lines(&format!("{} {}", scanned(record), summary(record)), width))];
    parts.extend(limited(record).map(|note| Part::Note(message_lines(&note, width))));
    parts
}

pub(crate) fn draw_done(record: &PortScanRecord, theme: &Theme, screen: Rect, buf: &mut Buffer) -> Rect {
    let (rect, parts) = layout(screen, |width| done_parts(record, width));
    draw_offer(tr!(port_scan_done_title), tr!(port_scan_done_keys), rect, parts, theme, buf);
    rect
}

/// An open port as the result lists it: the number, and the name IANA gives it.
pub(crate) fn port_row(port: u16) -> String {
    let number = port.to_string();
    match port_name(port) {
        Some(name) => format!("{number}{}{name}", " ".repeat(PORT_COLUMN.saturating_sub(number.width()))),
        None => number,
    }
}

fn result_parts(view: &PortScanView, clock: &Zoned, width: u16) -> Vec<Part> {
    let record = view.record();
    let scanned = tr!(
        port_scan_result_when,
        when = when(record.at, clock),
        mode = record.speed.label(),
        ports = count(record.count, Count::Port)
    );
    let mut parts = vec![Part::Text(message_lines(&format!("{scanned} {}", summary(record)), width))];
    if view.scans.len() > 1 {
        let position = tr!(port_scan_history_position, index = view.shown + 1, count = view.scans.len());
        parts.push(Part::Note(message_lines(&position, width)));
    }
    let rows: Vec<String> = record.open.iter().skip(view.top).take(RESULT_ROWS).map(|port| port_row(*port)).collect();
    if rows.is_empty() {
        parts.push(Part::Text(message_lines(tr!(port_scan_no_open), width)));
    } else {
        parts.push(Part::Command(rows));
    }
    parts.extend(limited(record).map(|note| Part::Note(message_lines(&note, width))));
    parts
}

pub(crate) fn draw_result(view: &PortScanView, theme: &Theme, clock: &Zoned, screen: Rect, buf: &mut Buffer) -> Rect {
    let (rect, parts) = layout(screen, |width| result_parts(view, clock, width));
    let title = tr!(port_scan_result_title, target = view.record().label());
    let keys = if view.scans.len() > 1 { tr!(port_scan_history_keys) } else { tr!(port_scan_result_keys) };
    draw_offer(&title, keys, rect, parts, theme, buf);
    rect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_ports_line_up_their_names_and_unnamed_ones_stand_alone() {
        assert_eq!(port_row(22), "22     ssh");
        assert_eq!(port_row(8080), "8080   http-alt");
        assert_eq!(port_row(62078), "62078");
    }
}
