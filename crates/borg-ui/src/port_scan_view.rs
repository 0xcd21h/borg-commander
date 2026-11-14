//! The port scan's dialogs: what to scan, the notice that a scan finished, and the open ports it
//! found.

use std::time::{Duration, UNIX_EPOCH};

use crate::{
    Theme,
    activity_view::{dim, text},
    dialog_view::{centered, message_lines, skip_columns},
    format,
    install_view::{Part, draw_offer, layout},
};
use borg_core::{
    AppState, Ending, FIRST_PORT, FormField, LAST_PORT, LineInput, PortChoice, PortScanForm, PortScanRecord,
    PortScanView, PortsKind, ScanSpeed, port_name,
};
use borg_i18n::{Count, count, tr};
use jiff::Zoned;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

const LABEL_GAP: usize = 2;
const SIDE_PADDING: u16 = 2;
const BORDER: u16 = 2;
const TOP_ROWS: u16 = 1;
const GAP_ROWS: u16 = 1;
const WIDTH: u16 = 64;
const PREVIOUS: &str = "‹ ";
const NEXT: &str = " ›";
const PLAIN: &str = "  ";
const CURSOR: &str = "▏";
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

fn label_width(form: &PortScanForm) -> usize {
    form.fields().iter().map(|&field| label(field).width()).max().unwrap_or_default() + LABEL_GAP
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

fn input_text(input: &LineInput, focused: bool, width: usize) -> String {
    if !focused {
        return format::truncate_end(input.text(), width);
    }
    let before: String = input.text().chars().take(input.cursor_chars()).collect();
    let after: String = input.text().chars().skip(input.cursor_chars()).collect();
    let shift = before.width().saturating_sub(width.saturating_sub(CURSOR.width()));
    skip_columns(&format!("{before}{CURSOR}{after}"), shift)
}

fn value(form: &PortScanForm, field: FormField, width: usize) -> String {
    let selected = form.field == field;
    let choice = |shown: String| if selected { format!("{PREVIOUS}{shown}{NEXT}") } else { format!("{PLAIN}{shown}") };
    match field {
        FormField::Target => format!("{PLAIN}{}", input_text(&form.target, selected, width)),
        FormField::List => format!("{PLAIN}{}", input_text(&form.list, selected, width)),
        FormField::Ports => choice(ports_value(form)),
        FormField::Speed => choice(form.speed.label().to_owned()),
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

fn note_lines(notes: &[(String, bool)], width: u16) -> Vec<(Line<'static>, bool)> {
    notes.iter().flat_map(|(note, warned)| message_lines(note, width).into_iter().map(|line| (line, *warned))).collect()
}

pub(crate) fn draw_form(
    state: &AppState,
    form: &PortScanForm,
    theme: &Theme,
    clock: &Zoned,
    screen: Rect,
    buf: &mut Buffer,
) -> Rect {
    let styles = &theme.styles;
    let inner_width = WIDTH.min(screen.width.saturating_sub(BORDER + SIDE_PADDING * 2));
    let notes = note_lines(&notes(state, form, clock), inner_width);
    let fields = u16::try_from(form.fields().len()).unwrap_or(u16::MAX);
    let rows = u16::try_from(notes.len()).unwrap_or(u16::MAX);
    let height = BORDER + TOP_ROWS + fields + GAP_ROWS + rows;
    let rect = centered(screen, inner_width + BORDER + SIDE_PADDING * 2, height.min(screen.height));
    Clear.render(rect, buf);
    let block = Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(tr!(port_scan_form_title), styles.overlay_border).centered())
        .title_bottom(Line::styled(tr!(port_scan_form_keys), styles.overlay).centered());
    let inner = block.inner(rect);
    block.render(rect, buf);
    let padded = Rect { x: inner.x + SIDE_PADDING, width: inner.width.saturating_sub(SIDE_PADDING * 2), ..inner };
    let line = |offset: u16| Rect { y: inner.y + offset, height: u16::from(offset < inner.height), ..padded };
    let labels = u16::try_from(label_width(form)).unwrap_or(u16::MAX);
    for (offset, &field) in (TOP_ROWS..).zip(form.fields()) {
        let area = line(offset);
        if area.is_empty() {
            break;
        }
        let selected = field == form.field;
        let style = if selected { styles.overlay_selected } else { styles.overlay };
        buf.set_style(Rect { x: inner.x, width: inner.width, ..area }, style);
        text(
            buf,
            Rect { width: labels.min(area.width), ..area },
            label(field),
            if selected { style } else { dim(theme) },
        );
        let rest = Rect { x: area.x + labels.min(area.width), width: area.width.saturating_sub(labels), ..area };
        let room = usize::from(rest.width).saturating_sub(PLAIN.width() + NEXT.width());
        text(buf, rest, format::truncate_end(&value(form, field, room), usize::from(rest.width)), style);
    }
    for (offset, (note, warned)) in (TOP_ROWS + fields + GAP_ROWS..).zip(notes) {
        let area = line(offset);
        if area.is_empty() {
            break;
        }
        let style = if warned { styles.notice } else { dim(theme) };
        note.style(style).render(area, buf);
    }
    rect
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
    let record = &view.record;
    let scanned = tr!(
        port_scan_result_when,
        when = when(record.at, clock),
        mode = record.speed.label(),
        ports = count(record.count, Count::Port)
    );
    let mut parts = vec![Part::Text(message_lines(&format!("{scanned} {}", summary(record)), width))];
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
    let title = tr!(port_scan_result_title, target = view.record.label());
    draw_offer(&title, tr!(port_scan_result_keys), rect, parts, theme, buf);
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
