//! Dialogs made of labeled fields over notes: the port scan form and the report save dialog.

use crate::{
    Theme,
    activity_view::{dim, text},
    dialog_view::{centered, message_lines, skip_columns},
    format,
};
use borg_core::LineInput;
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

/// A dialog of fields. Each field has its label and whether it takes the keys, and each note
/// says whether it warns.
pub(crate) struct FieldDialog<'a> {
    pub title: &'a str,
    pub keys: &'a str,
    pub fields: Vec<(&'static str, bool)>,
    pub notes: Vec<(String, bool)>,
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

/// Typed text in `room` cells, with the cursor when the field takes the keys.
pub(crate) fn input(input: &LineInput, selected: bool, room: usize) -> String {
    format!("{PLAIN}{}", input_text(input, selected, room))
}

/// One of a few choices, with arrows around it when the field takes the keys.
pub(crate) fn choice(shown: &str, selected: bool) -> String {
    if selected { format!("{PREVIOUS}{shown}{NEXT}") } else { format!("{PLAIN}{shown}") }
}

fn note_lines(notes: &[(String, bool)], width: u16) -> Vec<(Line<'static>, bool)> {
    notes.iter().flat_map(|(note, warned)| message_lines(note, width).into_iter().map(|line| (line, *warned))).collect()
}

/// Draws the dialog in the middle of `screen`. `value` gives each field's text for the room it has.
pub(crate) fn draw(
    dialog: &FieldDialog<'_>,
    value: impl Fn(usize, usize) -> String,
    theme: &Theme,
    screen: Rect,
    buf: &mut Buffer,
) -> Rect {
    let styles = &theme.styles;
    let inner_width = WIDTH.min(screen.width.saturating_sub(BORDER + SIDE_PADDING * 2));
    let notes = note_lines(&dialog.notes, inner_width);
    let fields = u16::try_from(dialog.fields.len()).unwrap_or(u16::MAX);
    let rows = u16::try_from(notes.len()).unwrap_or(u16::MAX);
    let height = BORDER + TOP_ROWS + fields + GAP_ROWS + rows;
    let rect = centered(screen, inner_width + BORDER + SIDE_PADDING * 2, height.min(screen.height));
    Clear.render(rect, buf);
    let block = Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(dialog.title, styles.overlay_border).centered())
        .title_bottom(Line::styled(dialog.keys, styles.overlay).centered());
    let inner = block.inner(rect);
    block.render(rect, buf);
    let padded = Rect { x: inner.x + SIDE_PADDING, width: inner.width.saturating_sub(SIDE_PADDING * 2), ..inner };
    let line = |offset: u16| Rect { y: inner.y + offset, height: u16::from(offset < inner.height), ..padded };
    let label_width = dialog.fields.iter().map(|(label, _)| label.width()).max().unwrap_or_default() + LABEL_GAP;
    let labels = u16::try_from(label_width).unwrap_or(u16::MAX);
    for (index, (offset, &(label, selected))) in (TOP_ROWS..).zip(&dialog.fields).enumerate() {
        let area = line(offset);
        if area.is_empty() {
            break;
        }
        let style = if selected { styles.overlay_selected } else { styles.overlay };
        buf.set_style(Rect { x: inner.x, width: inner.width, ..area }, style);
        text(buf, Rect { width: labels.min(area.width), ..area }, label, if selected { style } else { dim(theme) });
        let rest = Rect { x: area.x + labels.min(area.width), width: area.width.saturating_sub(labels), ..area };
        let room = usize::from(rest.width).saturating_sub(PLAIN.width() + NEXT.width());
        text(buf, rest, format::truncate_end(&value(index, room), usize::from(rest.width)), style);
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
