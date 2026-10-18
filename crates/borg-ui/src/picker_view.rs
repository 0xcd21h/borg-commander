use borg_core::Picker;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    text::Line,
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{Theme, ThemeSet};

const TITLE: &str = " Theme ";
const HINT: &str = " Enter keep · Esc cancel ";
const MIN_WIDTH: u16 = 28;
const SIDE_PADDING: u16 = 6;
const BORDER_ROWS: u16 = 2;

pub fn draw_picker(picker: &Picker, themes: &ThemeSet, theme: &Theme, area: Rect, buf: &mut Buffer) {
    let styles = &theme.styles;
    let names: Vec<&str> = themes.themes().iter().map(|theme| theme.name.as_str()).collect();
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
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [row] = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center).areas(area);
    let [rect] = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center).areas(row);
    rect
}
