//! The dialog R opens on the network screen: what to capture, on which interface, for how
//! long, how much of each packet to keep, whether to record TLS keys, and whether to run as
//! administrator.

use crate::{
    Theme,
    activity_view::{dim, text},
    dialog_view::centered,
    format,
};
use borg_core::{
    Authority, CaptureDialog, CaptureField, CaptureScope, DURATIONS, KEEP_CHOICES, Network, TlsKeys, capture_extension,
};
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
        CaptureField::TlsKeys => tr!(capture_tls_field),
        CaptureField::Access => tr!(capture_access_field),
    }
}

fn access_label(dialog: &CaptureDialog, administrator: bool) -> &'static str {
    match (administrator, dialog.access.tags) {
        (false, _) => tr!(capture_as_you),
        (true, false) => tr!(capture_as_administrator),
        (true, true) => tr!(capture_as_administrator_tagged),
    }
}

fn tls_label(tls_keys: &TlsKeys) -> String {
    match tls_keys {
        TlsKeys::Off => tr!(capture_tls_off).to_owned(),
        TlsKeys::Shell => tr!(capture_tls_on).to_owned(),
        TlsKeys::Proxy(Authority::Temporary) => tr!(capture_tls_proxy).to_owned(),
        TlsKeys::Proxy(Authority::Folder(folder)) => tr!(capture_tls_proxy_folder, folder = folder.display()),
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
        CaptureField::Content => dialog.keep.label(),
        CaptureField::TlsKeys => tls_label(&dialog.tls_keys),
        CaptureField::Access => access_label(dialog, dialog.administrator).to_owned(),
    }
}

/// The widest each field's value can be, so the dialog keeps its size while values change.
fn widest_value(dialog: &CaptureDialog, network: &Network) -> usize {
    let scopes = dialog.scopes.iter().map(|scope| scope_label(network, *scope).width());
    let interfaces = dialog.interfaces.iter().map(|name| name.width());
    let durations = DURATIONS.iter().map(|seconds| duration_label(*seconds).width());
    let contents = KEEP_CHOICES.map(|keep| keep.label().width());
    let tls_keys = dialog.tls_choices().map(|tls_keys| tls_label(&tls_keys).width());
    let shown = dialog.fields().contains(&CaptureField::Access);
    let access =
        [false, true].into_iter().filter(|_| shown).map(|administrator| access_label(dialog, administrator).width());
    scopes.chain(interfaces).chain(durations).chain(contents).chain(tls_keys).chain(access).max().unwrap_or_default()
}

fn label_width(dialog: &CaptureDialog) -> usize {
    dialog.fields().iter().map(|&field| label(field).width()).max().unwrap_or_default() + LABEL_GAP
}

/// Where the capture goes, in at most `width` columns: a long folder loses its start first.
fn note(folder: &Path, extension: &str, width: usize) -> String {
    let around = tr!(capture_saves_to, extension = extension, folder = "").width();
    let folder = format::truncate_start(&folder.display().to_string(), width.saturating_sub(around));
    format::truncate_end(&tr!(capture_saves_to, extension = extension, folder = folder), width)
}

fn dialog_rect(dialog: &CaptureDialog, network: &Network, area: Rect) -> Rect {
    let fields = label_width(dialog) + PREVIOUS.width() + widest_value(dialog, network) + NEXT.width();
    let notes = [false, true]
        .into_iter()
        .filter(|&tags| !tags || dialog.access.tags)
        .map(|tags| note(dialog.folder(), capture_extension(tags), WIDEST_NOTE).width());
    let widest = [fields, tr!(capture_keys).width(), tr!(capture_title).width()]
        .into_iter()
        .chain(notes)
        .max()
        .unwrap_or_default();
    let width = u16::try_from(widest).unwrap_or(u16::MAX).saturating_add(BORDER + SIDE_PADDING * 2);
    let rows = u16::try_from(dialog.fields().len()).unwrap_or(u16::MAX);
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
    let row = inside.then(|| usize::from(point.y - top)).filter(|row| *row < dialog.fields().len());
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
    let labels = u16::try_from(label_width(dialog)).unwrap_or(u16::MAX);
    for (index, &field) in dialog.fields().iter().enumerate() {
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
    let rows = u16::try_from(dialog.fields().len()).unwrap_or(u16::MAX);
    let below = line(TOP_ROWS + rows + 1);
    let extension = capture_extension(dialog.tags_processes());
    text(buf, below, note(dialog.folder(), extension, usize::from(below.width)), dim(theme));
    rect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_folder_loses_its_start_and_keeps_the_sentence() {
        let folder = Path::new("/home/me/projects/borg/captures/today");
        assert_eq!(note(folder, "pcap", 80), "Saved as a .pcap file in /home/me/projects/borg/captures/today");
        assert_eq!(note(folder, "pcap", 40), "Saved as a .pcap file in …captures/today");
        assert_eq!(note(folder, "pcapng", 40), "Saved as a .pcapng file in …ptures/today");
        assert_eq!(note(folder, "pcap", 40).width(), 40);
    }
}
