//! The monitor's dialogs that pick one of a few choices by letter: what to send a process, and
//! what the entropy scan reads.

use super::{dim, text};
use crate::{Theme, dialog_view::centered};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    text::Line,
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

const SIDE_PADDING: u16 = 2;
const BORDER: u16 = 2;
const SUBJECT_ROWS: u16 = 2;

/// A dialog's text: what it acts on, then a row per choice, its start and what it does, and
/// whether it can be chosen now.
pub(crate) struct Choices<'a> {
    pub title: String,
    pub subject: String,
    pub keys: &'a str,
    pub rows: Vec<(String, String, bool)>,
    pub selected: usize,
}

pub(crate) fn rect(choices: &Choices<'_>, area: Rect) -> Rect {
    let widest = choices
        .rows
        .iter()
        .map(|(head, about, _)| head.width() + about.width())
        .chain([choices.subject.width(), choices.keys.width(), choices.title.width()])
        .max()
        .unwrap_or_default();
    let width = u16::try_from(widest).unwrap_or(u16::MAX).saturating_add(BORDER + SIDE_PADDING * 2);
    let height = u16::try_from(choices.rows.len()).unwrap_or(u16::MAX).saturating_add(BORDER + SUBJECT_ROWS);
    centered(area, width.min(area.width), height.min(area.height))
}

/// The choice's row under `point`, when it is on one.
pub(crate) fn choice_at(choices: &Choices<'_>, area: Rect, point: Position) -> (Rect, Option<usize>) {
    let rect = rect(choices, area);
    let top = rect.y + 1 + SUBJECT_ROWS;
    let inside = point.x > rect.x && point.x < rect.right().saturating_sub(1) && point.y >= top;
    let row = inside.then(|| usize::from(point.y - top)).filter(|row| *row < choices.rows.len());
    (rect, row)
}

pub(crate) fn draw(choices: &Choices<'_>, theme: &Theme, area: Rect, buf: &mut Buffer) -> Rect {
    let styles = &theme.styles;
    let rect = rect(choices, area);
    Clear.render(rect, buf);
    let block = Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(choices.title.as_str(), styles.overlay_border).centered())
        .title_bottom(Line::styled(choices.keys, styles.overlay).centered());
    let inner = block.inner(rect);
    block.render(rect, buf);
    let padded = Rect { x: inner.x + SIDE_PADDING, width: inner.width.saturating_sub(SIDE_PADDING * 2), ..inner };
    let line = |offset: u16| Rect { y: inner.y + offset, height: u16::from(offset < inner.height), ..padded };
    text(buf, line(0), choices.subject.clone(), styles.overlay);
    for (index, (head, about, enabled)) in choices.rows.iter().enumerate() {
        let Ok(offset) = u16::try_from(index) else { break };
        let area = line(SUBJECT_ROWS + offset);
        if area.is_empty() {
            break;
        }
        let selected = index == choices.selected;
        let style = if selected { styles.overlay_selected } else { styles.overlay };
        buf.set_style(Rect { x: inner.x, width: inner.width, ..area }, style);
        let head_style = if *enabled || selected { style } else { dim(theme) };
        let about_style = if selected { style } else { dim(theme) };
        let head_width = u16::try_from(head.width()).unwrap_or(u16::MAX).min(area.width);
        text(buf, Rect { width: head_width, ..area }, head.clone(), head_style);
        text(buf, Rect { x: area.x + head_width, width: area.width - head_width, ..area }, about.clone(), about_style);
    }
    rect
}
