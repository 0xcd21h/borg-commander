use borg_core::{ContextMenu, Menu, MenuEntry};
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

pub fn draw_dropdown<A>(
    menu: &Menu<A>,
    selected: Option<usize>,
    x: u16,
    top: u16,
    theme: &Theme,
    screen: Rect,
    buf: &mut Buffer,
) -> Rect {
    let rect = dropdown_rect(menu, x, top, screen);
    draw_entries(menu, selected, rect, theme, buf);
    rect
}

/// The context menu, under the cell a right-click opened it at or mid-screen, with the name of
/// what it acts on in its top border.
pub fn draw_context_menu(context: &ContextMenu, theme: &Theme, screen: Rect, buf: &mut Buffer) -> Rect {
    let rect = context_rect(context, screen);
    draw_entries(&context.menu, context.selected, rect, theme, buf);
    let border = Rect { x: rect.x + 1, height: 1, width: rect.width.saturating_sub(BORDER), ..rect };
    Line::styled(format!(" {} ", context.menu.title), theme.styles.menu).render(border, buf);
    rect
}

/// Where the context menu shows: below the cell it was opened at, or above it when there is no
/// room below, and in the middle of the screen when a key opened it.
pub(crate) fn context_rect(context: &ContextMenu, screen: Rect) -> Rect {
    let size = dropdown_rect(&context.menu, screen.x, screen.y, screen);
    let (width, height) = (size.width, size.height);
    let (x, y) = match context.at {
        Some((column, row)) => {
            let below = row.saturating_add(1);
            let fits = below.saturating_add(height) <= screen.bottom();
            let y = if fits { below } else { row.saturating_sub(height).max(screen.y) };
            (column.min(screen.right().saturating_sub(width)), y)
        }
        None => {
            (screen.x + screen.width.saturating_sub(width) / 2, screen.y + screen.height.saturating_sub(height) / 2)
        }
    };
    Rect { x, y, width, height }.intersection(screen)
}

/// The rows inside a menu's frame, where each row is an entry.
pub(crate) fn entries_area(rect: Rect) -> Rect {
    Rect {
        x: rect.x + 1,
        y: rect.y + 1,
        width: rect.width.saturating_sub(BORDER),
        height: rect.height.saturating_sub(BORDER),
    }
}

fn draw_entries<A>(menu: &Menu<A>, selected: Option<usize>, rect: Rect, theme: &Theme, buf: &mut Buffer) {
    let styles = &theme.styles;
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

pub(crate) fn dropdown_rect<A>(menu: &Menu<A>, x: u16, top: u16, screen: Rect) -> Rect {
    let label_width = menu.entries.iter().map(entry_width).max().unwrap_or_default();
    let width = u16::try_from(label_width).unwrap_or(u16::MAX).saturating_add(BORDER);
    let height = u16::try_from(menu.entries.len()).unwrap_or(u16::MAX).saturating_add(BORDER);
    let x = x.min(screen.width.saturating_sub(width));
    Rect { x, y: top, width, height }.intersection(screen)
}

fn entry_width<A>(entry: &MenuEntry<A>) -> usize {
    match entry {
        MenuEntry::Item(item) => NO_CHECK.width() + item.label.width() + HINT_GAP + item.hint.width() + 1,
        MenuEntry::Separator => 0,
    }
}
