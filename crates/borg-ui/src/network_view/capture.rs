//! The dialog R opens on the network screen: what to capture, on which interface, for how
//! long, and how much of each packet to keep.

use crate::{
    Theme,
    activity_view::{dim, text},
    dialog_view::centered,
    format,
};
use borg_core::{CAPTURE_FIELDS, CaptureDialog, CaptureField, CaptureScope, DURATIONS, HEADER_BYTES, Network};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    text::Line,
    widgets::{Block, Clear, Widget},
};
use std::path::Path;
use unicode_width::UnicodeWidthStr;

const LABEL_GAP: usize = 2;
const SIDE_PADDING: u16 = 2;
const BORDER: u16 = 2;
/// A blank row above the fields, and a blank row and the note below them.
const TOP_ROWS: u16 = 1;
const NOTE_ROWS: u16 = 2;
const PREVIOUS: &str = "‹ ";
const NEXT: &str = " ›";
/// What stands in for `PREVIOUS` beside a field that is not selected.
const PLAIN: &str = "  ";
/// The widest the note under the fields makes the dialog.
const WIDEST_NOTE: usize = 72;

fn label(field: CaptureField) -> &'static str {
    match field {
        CaptureField::Scope => tr!(capture_scope_field),
        CaptureField::Interface => tr!(network_interface_column),
        CaptureField::Duration => tr!(capture_duration_field),
        CaptureField::Content => tr!(capture_content_field),
    }
}

fn scope_label(network: &Network, scope: CaptureScope) -> String {
    scope.label(
        |pid| tr!(process_control_subject, name = network.name(Some(pid)), pid = pid),
        |remote| super::endpoint(network, remote),
    )
}

fn duration_label(seconds: Option<u64>) -> String {
    seconds.map_or_else(|| tr!(capture_until_stopped).to_owned(), |seconds| tr!(capture_seconds, seconds = seconds))
}

fn value(dialog: &CaptureDialog, network: &Network, field: CaptureField) -> String {
    match field {
        CaptureField::Scope => dialog.scopes.get(dialog.scope).map_or_else(String::new, |s| scope_label(network, *s)),
        CaptureField::Interface => dialog.interfaces.get(dialog.interface).cloned().unwrap_or_default(),
        CaptureField::Duration => duration_label(DURATIONS[dialog.duration]),
        CaptureField::Content if dialog.headers_only => tr!(capture_headers_only, bytes = HEADER_BYTES),
        CaptureField::Content => tr!(capture_whole_packets).to_owned(),
    }
}

/// The widest each field's value can be, so the dialog keeps its size while values change.
fn widest_value(dialog: &CaptureDialog, network: &Network) -> usize {
    let scopes = dialog.scopes.iter().map(|scope| scope_label(network, *scope).width());
    let interfaces = dialog.interfaces.iter().map(|name| name.width());
    let durations = DURATIONS.iter().map(|seconds| duration_label(*seconds).width());
    let contents = [tr!(capture_headers_only, bytes = HEADER_BYTES).width(), tr!(capture_whole_packets).width()];
    scopes.chain(interfaces).chain(durations).chain(contents).max().unwrap_or_default()
}

fn label_width() -> usize {
    CAPTURE_FIELDS.iter().map(|&field| label(field).width()).max().unwrap_or_default() + LABEL_GAP
}

/// Where the capture goes, in at most `width` columns: a long folder loses its start first.
fn note(folder: &Path, width: usize) -> String {
    let around = tr!(capture_saves_to, folder = "").width();
    let folder = format::truncate_start(&folder.display().to_string(), width.saturating_sub(around));
    format::truncate_end(&tr!(capture_saves_to, folder = folder), width)
}

fn dialog_rect(dialog: &CaptureDialog, network: &Network, area: Rect) -> Rect {
    let fields = label_width() + PREVIOUS.width() + widest_value(dialog, network) + NEXT.width();
    let widest =
        [fields, note(dialog.folder(), WIDEST_NOTE).width(), tr!(capture_keys).width(), tr!(capture_title).width()]
            .into_iter()
            .max()
            .unwrap_or_default();
    let width = u16::try_from(widest).unwrap_or(u16::MAX).saturating_add(BORDER + SIDE_PADDING * 2);
    let rows = u16::try_from(CAPTURE_FIELDS.len()).unwrap_or(u16::MAX);
    let height = rows.saturating_add(BORDER + TOP_ROWS + NOTE_ROWS);
    centered(area, width.min(area.width), height.min(area.height))
}

/// The field's row under `point`, when it is on one.
pub(crate) fn field_at(
    dialog: &CaptureDialog,
    network: &Network,
    area: Rect,
    point: Position,
) -> (Rect, Option<usize>) {
    let rect = dialog_rect(dialog, network, area);
    let top = rect.y + 1 + TOP_ROWS;
    let inside = point.x > rect.x && point.x < rect.right().saturating_sub(1) && point.y >= top;
    let row = inside.then(|| usize::from(point.y - top)).filter(|row| *row < CAPTURE_FIELDS.len());
    (rect, row)
}

pub(crate) fn draw_dialog(
    dialog: &CaptureDialog,
    network: &Network,
    theme: &Theme,
    area: Rect,
    buf: &mut Buffer,
) -> Rect {
    let styles = &theme.styles;
    let rect = dialog_rect(dialog, network, area);
    Clear.render(rect, buf);
    let block = Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(tr!(capture_title), styles.overlay_border).centered())
        .title_bottom(Line::styled(tr!(capture_keys), styles.overlay).centered());
    let inner = block.inner(rect);
    block.render(rect, buf);
    let padded = Rect { x: inner.x + SIDE_PADDING, width: inner.width.saturating_sub(SIDE_PADDING * 2), ..inner };
    let line = |offset: u16| Rect { y: inner.y + offset, height: u16::from(offset < inner.height), ..padded };
    let labels = u16::try_from(label_width()).unwrap_or(u16::MAX);
    for (index, &field) in CAPTURE_FIELDS.iter().enumerate() {
        let Ok(offset) = u16::try_from(index) else { break };
        let area = line(TOP_ROWS + offset);
        if area.is_empty() {
            break;
        }
        let selected = index == dialog.field;
        let style = if selected { styles.overlay_selected } else { styles.overlay };
        buf.set_style(Rect { x: inner.x, width: inner.width, ..area }, style);
        let label_style = if selected { style } else { dim(theme) };
        text(buf, Rect { width: labels.min(area.width), ..area }, label(field), label_style);
        let shown = value(dialog, network, field);
        let shown = if selected { format!("{PREVIOUS}{shown}{NEXT}") } else { format!("{PLAIN}{shown}") };
        let rest = Rect { x: area.x + labels.min(area.width), width: area.width.saturating_sub(labels), ..area };
        text(buf, rest, format::truncate_end(&shown, usize::from(rest.width)), style);
    }
    let rows = u16::try_from(CAPTURE_FIELDS.len()).unwrap_or(u16::MAX);
    let below = line(TOP_ROWS + rows + 1);
    text(buf, below, note(dialog.folder(), usize::from(below.width)), dim(theme));
    rect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_folder_loses_its_start_and_keeps_the_sentence() {
        let folder = Path::new("/home/me/projects/borg/captures/today");
        assert_eq!(note(folder, 80), "Saved as a .pcap file in /home/me/projects/borg/captures/today");
        assert_eq!(note(folder, 40), "Saved as a .pcap file in …captures/today");
        assert_eq!(note(folder, 40).width(), 40);
    }
}
