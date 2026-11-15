//! The form H opens on the Neighbors page: the interface to listen on, for how long, and whether
//! to run as administrator, with what listening can hear below.

use crate::{
    Theme,
    activity_view::{dim, text},
    dialog_view::centered,
    format,
};
use borg_core::{LISTEN_DURATIONS, ListenField, ListenForm};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    text::Line,
    widgets::{Block, Clear, Widget},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const LABEL_GAP: usize = 2;
const SIDE_PADDING: u16 = 2;
const BORDER: u16 = 2;
/// A blank row above the fields, and a blank row between them and the note.
const TOP_ROWS: u16 = 1;
const GAP_ROWS: u16 = 1;
const PREVIOUS: &str = "‹ ";
const NEXT: &str = " ›";
const PLAIN: &str = "  ";
/// How wide the note under the fields lets the form grow.
const NOTE_WIDTH: usize = 56;
const SECONDS_PER_MINUTE: u64 = 60;

fn label(field: ListenField) -> &'static str {
    match field {
        ListenField::Interface => tr!(listen_interface_label),
        ListenField::Duration => tr!(listen_duration_label),
        ListenField::Access => tr!(capture_access_field),
    }
}

fn access_label(administrator: bool) -> &'static str {
    if administrator { tr!(capture_as_administrator) } else { tr!(capture_as_you) }
}

fn duration_label(seconds: Option<u64>) -> String {
    match seconds {
        None => tr!(capture_until_stopped).to_owned(),
        Some(seconds) if seconds < SECONDS_PER_MINUTE => tr!(capture_seconds, seconds = seconds),
        Some(seconds) => tr!(listen_minutes, minutes = seconds / SECONDS_PER_MINUTE),
    }
}

fn value(form: &ListenForm, field: ListenField) -> String {
    match field {
        ListenField::Interface => form.interfaces.get(form.interface).cloned().unwrap_or_default(),
        ListenField::Duration => duration_label(LISTEN_DURATIONS[form.duration]),
        ListenField::Access => access_label(form.administrator).to_owned(),
    }
}

/// The widest each field's value can be, so the form keeps its size while values change.
fn widest_value(form: &ListenForm) -> usize {
    let interfaces = form.interfaces.iter().map(|name| name.width());
    let durations = LISTEN_DURATIONS.iter().map(|seconds| duration_label(*seconds).width());
    let shown = form.fields().contains(&ListenField::Access);
    let access = [false, true].into_iter().filter(|_| shown).map(|administrator| access_label(administrator).width());
    interfaces.chain(durations).chain(access).max().unwrap_or_default()
}

fn label_width(form: &ListenForm) -> usize {
    form.fields().iter().map(|&field| label(field).width()).max().unwrap_or_default() + LABEL_GAP
}

/// `text` in lines of at most `width` columns, broken between words, and inside a word wider
/// than what is left of the line, as Chinese text is.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = vec![String::new()];
    for word in text.split_whitespace() {
        let last = lines.last_mut().expect("there is a line");
        if !last.is_empty() {
            let fits = last.width() + 1 + word.width() <= width;
            let breaks = word.width() > width && last.width() + 1 < width;
            if fits || breaks {
                last.push(' ');
            } else {
                lines.push(String::new());
            }
        }
        for c in word.chars() {
            let last = lines.last_mut().expect("there is a line");
            if !last.is_empty() && last.width() + c.width().unwrap_or_default() > width {
                lines.push(String::new());
            }
            lines.last_mut().expect("there is a line").push(c);
        }
    }
    lines
}

fn inner_width(form: &ListenForm) -> usize {
    let fields = label_width(form) + PREVIOUS.width() + widest_value(form) + NEXT.width();
    [fields, tr!(listen_keys).width(), tr!(listen_title).width(), NOTE_WIDTH].into_iter().max().unwrap_or_default()
}

fn form_rect(form: &ListenForm, area: Rect) -> Rect {
    let inner = inner_width(form);
    let width = u16::try_from(inner).unwrap_or(u16::MAX).saturating_add(BORDER + SIDE_PADDING * 2);
    let note = u16::try_from(wrap(tr!(listen_note), inner).len()).unwrap_or(u16::MAX);
    let rows = u16::try_from(form.fields().len()).unwrap_or(u16::MAX);
    let height = rows.saturating_add(BORDER + TOP_ROWS + GAP_ROWS).saturating_add(note);
    centered(area, width.min(area.width), height.min(area.height))
}

/// The field's row under `point`, when it is on one.
pub(crate) fn field_at(form: &ListenForm, area: Rect, point: Position) -> (Rect, Option<usize>) {
    let rect = form_rect(form, area);
    let top = rect.y + 1 + TOP_ROWS;
    let inside = point.x > rect.x && point.x < rect.right().saturating_sub(1) && point.y >= top;
    let row = inside.then(|| usize::from(point.y - top)).filter(|row| *row < form.fields().len());
    (rect, row)
}

pub(crate) fn draw(form: &ListenForm, theme: &Theme, area: Rect, buf: &mut Buffer) -> Rect {
    let styles = &theme.styles;
    let rect = form_rect(form, area);
    Clear.render(rect, buf);
    let block = Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(tr!(listen_title), styles.overlay_border).centered())
        .title_bottom(Line::styled(tr!(listen_keys), styles.overlay).centered());
    let inner = block.inner(rect);
    block.render(rect, buf);
    let padded = Rect { x: inner.x + SIDE_PADDING, width: inner.width.saturating_sub(SIDE_PADDING * 2), ..inner };
    let line = |offset: u16| Rect { y: inner.y + offset, height: u16::from(offset < inner.height), ..padded };
    let labels = u16::try_from(label_width(form)).unwrap_or(u16::MAX);
    for (index, &field) in form.fields().iter().enumerate() {
        let Ok(offset) = u16::try_from(index) else { break };
        let area = line(TOP_ROWS + offset);
        if area.is_empty() {
            break;
        }
        let selected = index == form.field;
        let style = if selected { styles.overlay_selected } else { styles.overlay };
        buf.set_style(Rect { x: inner.x, width: inner.width, ..area }, style);
        let label_style = if selected { style } else { dim(theme) };
        text(buf, Rect { width: labels.min(area.width), ..area }, label(field), label_style);
        let shown = value(form, field);
        let shown = if selected { format!("{PREVIOUS}{shown}{NEXT}") } else { format!("{PLAIN}{shown}") };
        let rest = Rect { x: area.x + labels.min(area.width), width: area.width.saturating_sub(labels), ..area };
        text(buf, rest, format::truncate_end(&shown, usize::from(rest.width)), style);
    }
    let rows = u16::try_from(form.fields().len()).unwrap_or(u16::MAX);
    for (index, note) in wrap(tr!(listen_note), usize::from(padded.width)).iter().enumerate() {
        let Ok(offset) = u16::try_from(index) else { break };
        let area = line(TOP_ROWS + rows + GAP_ROWS + offset);
        if area.is_empty() {
            break;
        }
        text(buf, area, note, dim(theme));
    }
    rect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_note_breaks_between_words_and_inside_text_without_spaces() {
        assert_eq!(wrap("Borg sends nothing while it listens.", 16), ["Borg sends", "nothing while it", "listens."]);
        assert_eq!(wrap("监听时不发送任何数据", 8), ["监听时不", "发送任何", "数据"]);
        assert!(wrap("Borg 监听时不发送任何数据。", 10).iter().all(|line| line.width() <= 10));
    }
}
