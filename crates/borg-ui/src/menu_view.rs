use borg_core::{ContextMenu, Menu, MenuEntry};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::Theme;

#[cfg(test)]
mod tests;

const TITLE_PADDING: u16 = 2;
const CHECK: &str = "✓ ";
const NO_CHECK: &str = "  ";
const HINT_GAP: usize = 3;
const BORDER: u16 = 2;
const SEPARATOR: char = '─';
const MORE_LEFT: &str = "◀";
const MORE_RIGHT: &str = "▶";
const MORE_ABOVE: &str = "▲";
const MORE_BELOW: &str = "▼";
/// The columns a mark of more menus takes at an end of the menu bar.
const MARK: u16 = 1;
/// How far from a menu's right edge the marks of more entries sit in its border.
const MARK_INSET: u16 = 3;

/// Where each menu's title starts on the menu bar, before the bar moves.
pub fn title_positions(menus: &[Menu]) -> Vec<u16> {
    menus
        .iter()
        .scan(TITLE_PADDING, |x, menu| {
            let start = *x;
            *x += span(menu);
            Some(start)
        })
        .collect()
}

/// The columns a menu's title takes on the menu bar, with the padding around it.
fn span(menu: &Menu) -> u16 {
    u16::try_from(menu.title.width()).unwrap_or(u16::MAX).saturating_add(TITLE_PADDING * 2)
}

/// Where the last title ends on the menu bar, before the bar moves.
fn titles_end(menus: &[Menu]) -> u16 {
    title_positions(menus).last().zip(menus.last()).map_or(0, |(&start, menu)| start.saturating_add(span(menu)))
}

/// How many columns the titles move left on a bar `width` columns wide, so the open menu's title
/// shows whole with a mark of more menus after it. They stay put while all of them fit.
pub fn bar_shift(menus: &[Menu], open: Option<usize>, width: u16) -> u16 {
    if titles_end(menus) <= width {
        return 0;
    }
    let end = open.and_then(|open| Some(title_positions(menus).get(open)?.saturating_add(span(menus.get(open)?))));
    end.map_or(0, |end| end.saturating_add(MARK).saturating_sub(width))
}

/// The menu whose title is at `column` of the menu bar, after the bar moved by `shift`.
pub(crate) fn title_at(menus: &[Menu], column: u16, shift: u16) -> Option<usize> {
    let column = column.saturating_add(shift);
    title_positions(menus).iter().zip(menus).position(|(&x, menu)| column >= x && column < x.saturating_add(span(menu)))
}

/// The column the open menu's entries drop down from, under its title.
pub(crate) fn dropdown_left(menus: &[Menu], open: usize, width: u16) -> u16 {
    let start = title_positions(menus).get(open).copied().unwrap_or_default();
    start.saturating_sub(bar_shift(menus, Some(open), width))
}

/// The menu bar, moved so the open menu's title shows, with marks at the ends that have more
/// menus past them.
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
    let shift = bar_shift(menus, open, area.width);
    let left = if shift > 0 { MARK } else { 0 };
    let right = if titles_end(menus).saturating_sub(shift) > area.width { MARK } else { 0 };
    let titles = Rect { x: area.x + left, width: area.width.saturating_sub(left + right), ..area };
    Paragraph::new(Line::from(spans)).scroll((0, shift + left)).render(titles, buf);
    if left > 0 {
        buf.set_string(area.x, area.y, MORE_LEFT, styles.menu);
    }
    if right > 0 {
        buf.set_string(area.right().saturating_sub(MARK), area.y, MORE_RIGHT, styles.menu);
    }
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
    mark_hidden(menu, selected, rect, theme.styles.menu, buf);
    rect
}

/// The context menu, under the cell a right-click opened it at or mid-screen, with the name of
/// what it acts on in its top border.
pub fn draw_context_menu(context: &ContextMenu, theme: &Theme, screen: Rect, buf: &mut Buffer) -> Rect {
    let rect = context_rect(context, screen);
    draw_entries(&context.menu, context.selected, rect, theme, buf);
    let border = Rect { x: rect.x + 1, height: 1, width: rect.width.saturating_sub(BORDER), ..rect };
    Line::styled(format!(" {} ", context.menu.title), theme.styles.menu).render(border, buf);
    mark_hidden(&context.menu, context.selected, rect, theme.styles.menu, buf);
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

/// The first entry a menu shows in `rows` rows: the first while all of them fit, otherwise the
/// one that keeps the chosen entry mid-menu, as far as the ends allow.
pub(crate) fn first_entry(count: usize, rows: usize, selected: Option<usize>) -> usize {
    selected.map_or(0, |at| at.saturating_sub(rows / 2)).min(count.saturating_sub(rows))
}

/// The entry of a menu drawn in `rect` that `point` is on.
pub(crate) fn entry_at<A>(menu: &Menu<A>, selected: Option<usize>, rect: Rect, point: Position) -> Option<usize> {
    let inner = entries_area(rect);
    let first = first_entry(menu.entries.len(), usize::from(inner.height), selected);
    inner.contains(point).then(|| first + usize::from(point.y - inner.y))
}

/// Marks in the border of a menu drawn in `rect` where it has entries above or below the rows
/// it shows.
fn mark_hidden<A>(menu: &Menu<A>, selected: Option<usize>, rect: Rect, style: Style, buf: &mut Buffer) {
    if rect.width <= MARK_INSET {
        return;
    }
    let rows = usize::from(entries_area(rect).height);
    let count = menu.entries.len();
    let first = first_entry(count, rows, selected);
    let x = rect.right() - MARK_INSET;
    if first > 0 {
        buf.set_string(x, rect.y, MORE_ABOVE, style);
    }
    if first + rows < count {
        buf.set_string(x, rect.bottom().saturating_sub(1), MORE_BELOW, style);
    }
}

fn draw_entries<A>(menu: &Menu<A>, selected: Option<usize>, rect: Rect, theme: &Theme, buf: &mut Buffer) {
    let styles = &theme.styles;
    Clear.render(rect, buf);
    let block = Block::bordered().border_type(theme.border.into()).style(styles.menu).border_style(styles.menu);
    let inner = block.inner(rect);
    block.render(rect, buf);

    let first = first_entry(menu.entries.len(), usize::from(inner.height), selected);
    for (y, (index, entry)) in (inner.top()..inner.bottom()).zip(menu.entries.iter().enumerate().skip(first)) {
        let row = Rect { y, height: 1, ..inner };
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
