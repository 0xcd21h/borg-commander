use borg_core::{Menu, MenuEntry};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::Theme;

const TITLE_PADDING: u16 = 2;
const CHECK: &str = "✓ ";
const NO_CHECK: &str = "  ";
const HINT_GAP: usize = 3;
const BORDER: u16 = 2;
const SEPARATOR: char = '─';

pub fn title_positions(menus: &[Menu]) -> Vec<u16> {
    menus
        .iter()
        .scan(TITLE_PADDING, |x, menu| {
            let start = *x;
            *x += u16::try_from(menu.title.width()).unwrap_or(0) + TITLE_PADDING * 2;
            Some(start)
        })
        .collect()
}

pub fn draw_menu_bar(menus: &[Menu], open: Option<usize>, area: Rect, theme: &Theme, buf: &mut Buffer) {
    let styles = &theme.styles;
    Clear.render(area, buf);
    buf.set_style(area, styles.menu);
    let pad = " ".repeat(usize::from(TITLE_PADDING));
    let spans: Vec<Span<'_>> = std::iter::once(Span::styled(pad.clone(), styles.menu))
        .chain(menus.iter().enumerate().map(|(i, menu)| {
            let style = if Some(i) == open { styles.menu_selected } else { styles.menu };
            Span::styled(format!("{pad}{}{pad}", menu.title), style)
        }))
        .collect();
    Line::from(spans).render(area, buf);
}

pub fn draw_dropdown(
    menu: &Menu,
    selected: Option<usize>,
    x: u16,
    top: u16,
    theme: &Theme,
    screen: Rect,
    buf: &mut Buffer,
) {
    let styles = &theme.styles;
    let label_width = menu.entries.iter().map(entry_width).max().unwrap_or_default();
    let width = u16::try_from(label_width).unwrap_or(u16::MAX).saturating_add(BORDER);
    let height = u16::try_from(menu.entries.len()).unwrap_or(u16::MAX).saturating_add(BORDER);
    let x = x.min(screen.width.saturating_sub(width));
    let rect = Rect { x, y: top, width, height }.intersection(screen);

    Clear.render(rect, buf);
    let block = Block::bordered().border_type(theme.border.into()).style(styles.menu).border_style(styles.menu);
    let inner = block.inner(rect);
    block.render(rect, buf);

    for (index, entry) in menu.entries.iter().enumerate() {
        let Ok(offset) = u16::try_from(index) else { break };
        if offset >= inner.height {
            break;
        }
        let row = Rect { y: inner.y + offset, height: 1, ..inner };
        match entry {
            MenuEntry::Separator => {
                let line: String = std::iter::repeat_n(SEPARATOR, usize::from(row.width)).collect();
                Line::styled(line, styles.menu).render(row, buf);
            }
            MenuEntry::Item(item) => {
                let style = match (item.enabled, Some(index) == selected) {
                    (false, _) => styles.menu_disabled,
                    (true, true) => styles.menu_selected,
                    (true, false) => styles.menu,
                };
                buf.set_style(row, style);
                let check = if item.checked == Some(true) { CHECK } else { NO_CHECK };
                Line::styled(format!("{check}{}", item.label), style).render(row, buf);
                Line::styled(format!("{} ", item.hint), style).right_aligned().render(row, buf);
            }
        }
    }
}

fn entry_width(entry: &MenuEntry) -> usize {
    match entry {
        MenuEntry::Item(item) => NO_CHECK.width() + item.label.width() + HINT_GAP + item.hint.width() + 1,
        MenuEntry::Separator => 0,
    }
}
