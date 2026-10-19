use borg_core::{Picker, ThemeChoice};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{Theme, dialog_view::centered};

const TITLE: &str = " Theme ";
const HINT: &str = " Enter keep · Esc cancel ";
const MIN_WIDTH: u16 = 28;
const SIDE_PADDING: u16 = 6;
const BORDER_ROWS: u16 = 2;

pub fn draw_picker(picker: &Picker, choices: &[ThemeChoice], theme: &Theme, area: Rect, buf: &mut Buffer) -> Rect {
    let styles = &theme.styles;
    let names: Vec<&str> = choices.iter().map(|choice| choice.name.as_str()).collect();
    let widest = names.iter().map(|name| name.width()).max().unwrap_or_default();
    let width = u16::try_from(widest).unwrap_or(u16::MAX).saturating_add(SIDE_PADDING).max(MIN_WIDTH);
    let height = u16::try_from(names.len()).unwrap_or(u16::MAX).saturating_add(BORDER_ROWS);
    let rect = centered(area, width, height);

    Clear.render(rect, buf);
    let block = Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(TITLE, styles.overlay_border).centered())
        .title_bottom(Line::styled(HINT, styles.overlay).centered());
    let inner = block.inner(rect);
    block.render(rect, buf);

    for (index, name) in names.iter().enumerate() {
        let Ok(offset) = u16::try_from(index) else { break };
        if offset >= inner.height {
            break;
        }
        let row = Rect { y: inner.y + offset, height: 1, ..inner };
        let style = if index == picker.selected { styles.overlay_selected } else { styles.overlay };
        buf.set_style(row, style);
        Line::styled(format!("  {name}"), style).render(row, buf);
    }
    rect
}
