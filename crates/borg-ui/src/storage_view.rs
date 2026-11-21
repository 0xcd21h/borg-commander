//! The Borg's files window: each place Borg keeps files in, its size and folder, the captures
//! Borg saved, what the chosen row holds, and what is left over.

use borg_core::{AppState, Cleanup, Place, PlaceUsage, SavedCapture, StorageReport, StorageRow, StorageWindow};
use borg_i18n::{Count, count, tr};
use jiff::{Timestamp, Zoned};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, Clear, Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    Theme,
    activity_view::dim,
    dialog_view::{centered, message_lines},
    format,
};

const MAX_WIDTH: u16 = 92;
const MARGIN: u16 = 4;
const BORDER: u16 = 2;
const SIDE_PADDING: u16 = 2;
/// A blank row above the list, and one between the list and the text under it.
const TOP_ROWS: u16 = 1;
const GAP_ROWS: u16 = 1;
/// Rows for what the chosen row holds, and for the leftovers and the last cleanup.
const ABOUT_ROWS: u16 = 3;
const SUMMARY_ROWS: u16 = 2;
const MIN_LIST_ROWS: u16 = 3;
const SIZE_WIDTH: usize = 7;
const COLUMN_GAP: &str = "  ";
const CAPTURE_INDENT: &str = "  ";
const TIME_FORMAT: &str = "%H:%M";

/// A row of the list: a place, the heading over the captures, or a capture.
enum Shown<'a> {
    Row(usize, StorageRow<'a>),
    Heading,
}

fn shown(window: &StorageWindow) -> Vec<Shown<'_>> {
    let mut rows = Vec::new();
    for (index, row) in window.rows().into_iter().enumerate() {
        if matches!(row, StorageRow::Capture(_)) && !rows.iter().any(|row| matches!(row, Shown::Heading)) {
            rows.push(Shown::Heading);
        }
        rows.push(Shown::Row(index, row));
    }
    rows
}

fn text_width(screen: Rect) -> u16 {
    screen.width.saturating_sub(MARGIN).min(MAX_WIDTH).saturating_sub(BORDER + SIDE_PADDING * 2)
}

fn chrome() -> u16 {
    BORDER + TOP_ROWS + GAP_ROWS + ABOUT_ROWS + GAP_ROWS + SUMMARY_ROWS
}

pub(crate) fn rect(window: &StorageWindow, screen: Rect) -> Rect {
    let wanted = u16::try_from(shown(window).len()).unwrap_or(u16::MAX).max(MIN_LIST_ROWS);
    let height = wanted.saturating_add(chrome()).min(screen.height);
    centered(screen, text_width(screen).saturating_add(BORDER + SIDE_PADDING * 2), height)
}

/// Where the list sits inside the window.
fn list_area(window: &StorageWindow, screen: Rect) -> Rect {
    let inner = rect(window, screen).inner(ratatui::layout::Margin::new(1, 1));
    let below = GAP_ROWS + ABOUT_ROWS + GAP_ROWS + SUMMARY_ROWS;
    Rect {
        x: inner.x + SIDE_PADDING,
        y: inner.y + TOP_ROWS,
        width: inner.width.saturating_sub(SIDE_PADDING * 2),
        height: inner.height.saturating_sub(TOP_ROWS + below),
    }
}

/// The first row of the list in view, keeping the chosen one there.
fn start(window: &StorageWindow, rows: &[Shown<'_>], list: Rect) -> usize {
    let chosen = rows.iter().position(|row| matches!(row, Shown::Row(index, _) if *index == window.selected));
    chosen.unwrap_or_default().saturating_sub(usize::from(list.height).saturating_sub(1))
}

/// The row of the window's list at `point`, by its place among the rows a key moves over.
pub(crate) fn row_at(window: &StorageWindow, screen: Rect, point: ratatui::layout::Position) -> Option<usize> {
    let list = list_area(window, screen);
    if !list.contains(point) {
        return None;
    }
    let rows = shown(window);
    match rows.get(start(window, &rows, list) + usize::from(point.y - list.y))? {
        Shown::Row(index, _) => Some(*index),
        Shown::Heading => None,
    }
}

fn label_width(report: &StorageReport) -> usize {
    report.places.iter().map(|usage| usage.place.label().width()).max().unwrap_or_default()
}

fn place_text(state: &AppState, usage: &PlaceUsage, label: usize, width: usize) -> String {
    let name = usage.place.label();
    let pad = " ".repeat(label.saturating_sub(name.width()));
    let size = format::size(usage.bytes);
    let lead = format!("{name}{pad}{COLUMN_GAP}{size:>SIZE_WIDTH$}{COLUMN_GAP}");
    let rest = match &usage.path {
        Some(path) => state.folders.display(path),
        None => count(usage.items, Count::File),
    };
    format!("{lead}{}", format::truncate_start(&rest, width.saturating_sub(lead.width())))
}

/// How wide the column that marks captures with a key log is: none when no capture has one.
fn keys_width(report: &StorageReport) -> usize {
    if report.captures.iter().any(|capture| capture.keys.is_some()) {
        COLUMN_GAP.width() + tr!(storage_with_keys).width()
    } else {
        0
    }
}

fn capture_text(state: &AppState, capture: &SavedCapture, clock: &Zoned, keys_width: usize, width: usize) -> String {
    let size = format::size(capture.bytes);
    let saved = format::modified(Some(capture.saved), clock);
    let marked = if capture.keys.is_some() { format!("{COLUMN_GAP}{}", tr!(storage_with_keys)) } else { String::new() };
    let keys = format!("{marked}{}", " ".repeat(keys_width.saturating_sub(marked.width())));
    let tail = format!("{COLUMN_GAP}{size:>SIZE_WIDTH$}{COLUMN_GAP}{saved}{keys}");
    let room = width.saturating_sub(CAPTURE_INDENT.width() + tail.width());
    format!("{CAPTURE_INDENT}{:<room$}{tail}", format::truncate_start(&state.folders.display(&capture.path), room))
}

/// What the chosen row holds and how long it lasts.
fn about(window: &StorageWindow) -> &'static str {
    match window.chosen() {
        Some(StorageRow::Place(usage)) => usage.place.about(),
        Some(StorageRow::Capture(_)) => Place::Captures.about(),
        None => "",
    }
}

fn time(at: std::time::SystemTime, clock: &Zoned) -> String {
    Timestamp::try_from(at)
        .map(|stamp| stamp.to_zoned(clock.time_zone().clone()).strftime(TIME_FORMAT).to_string())
        .unwrap_or_default()
}

fn summary(report: &StorageReport, clock: &Zoned) -> Vec<String> {
    let left = if report.leftovers.is_empty() {
        tr!(storage_no_leftovers).to_owned()
    } else {
        let items = count(report.leftovers.len(), Count::Item);
        tr!(storage_leftovers, items = items, size = format::size(report.leftover_bytes()))
    };
    let cleaned = report.cleaned.map(|Cleanup { at, removed, bytes, failed }| {
        let (time, items, size) = (time(at, clock), count(removed, Count::Item), format::size(bytes));
        if failed > 0 {
            tr!(
                storage_last_cleanup_failed,
                time = time,
                items = items,
                size = size,
                failed = count(failed, Count::Item)
            )
        } else {
            tr!(storage_last_cleanup, time = time, items = items, size = size)
        }
    });
    std::iter::once(left).chain(cleaned).collect()
}

pub(crate) fn draw(
    state: &AppState,
    window: &StorageWindow,
    clock: &Zoned,
    theme: &Theme,
    screen: Rect,
    buf: &mut Buffer,
) -> Rect {
    let styles = &theme.styles;
    let rect = rect(window, screen);
    Clear.render(rect, buf);
    let block = Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(tr!(storage_title), styles.overlay_border).centered())
        .title_bottom(Line::styled(tr!(storage_hints), styles.overlay).centered());
    let inner = block.inner(rect);
    block.render(rect, buf);
    let area = Rect {
        x: inner.x + SIDE_PADDING,
        y: inner.y + TOP_ROWS,
        width: inner.width.saturating_sub(SIDE_PADDING * 2),
        height: inner.height.saturating_sub(TOP_ROWS),
    };
    let list = list_area(window, screen);
    let Some(report) = &window.report else {
        Line::styled(tr!(storage_measuring), styles.overlay).render(Rect { height: 1.min(list.height), ..list }, buf);
        return rect;
    };
    draw_list(state, window, report, clock, theme, list, buf);
    let about_area = Rect {
        y: list.bottom() + GAP_ROWS,
        height: ABOUT_ROWS.min(area.bottom().saturating_sub(list.bottom())),
        ..area
    };
    Paragraph::new(message_lines(about(window), about_area.width)).style(dim(theme)).render(about_area, buf);
    let summary_area = Rect {
        y: about_area.bottom() + GAP_ROWS,
        height: SUMMARY_ROWS.min(area.bottom().saturating_sub(about_area.bottom() + GAP_ROWS)),
        ..area
    };
    let lines: Vec<Line> = summary(report, clock).into_iter().map(|text| Line::styled(text, styles.overlay)).collect();
    Paragraph::new(lines).render(summary_area, buf);
    rect
}

fn draw_list(
    state: &AppState,
    window: &StorageWindow,
    report: &StorageReport,
    clock: &Zoned,
    theme: &Theme,
    list: Rect,
    buf: &mut Buffer,
) {
    let styles = &theme.styles;
    let rows = shown(window);
    let (label, keys, width) = (label_width(report), keys_width(report), usize::from(list.width));
    for (offset, row) in rows.iter().skip(start(window, &rows, list)).take(usize::from(list.height)).enumerate() {
        let area = Rect { y: list.y + u16::try_from(offset).unwrap_or(u16::MAX), height: 1, ..list };
        let (text, style): (String, Style) = match row {
            Shown::Heading => (tr!(storage_saved_captures).to_owned(), dim(theme)),
            Shown::Row(index, row) => {
                let style = if *index == window.selected { styles.overlay_selected } else { styles.overlay };
                let text = match row {
                    StorageRow::Place(usage) => place_text(state, usage, label, width),
                    StorageRow::Capture(capture) => capture_text(state, capture, clock, keys, width),
                };
                (text, style)
            }
        };
        buf.set_style(area, style);
        Line::styled(format::truncate_end(&text, width), style).render(area, buf);
    }
}
