//! The folders to jump to: the picker above the command line while `cd ` is typed, and the
//! dialog of the hotlist and the recent folders.

use borg_core::{AppState, FolderChoice, FoldersDialog, Layout};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::{Margin, Position, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{Theme, dialog_view::centered, format};

const PINNED: &str = "★ ";
const UNPINNED: &str = "  ";
/// The picker's border and the space on either side of a folder.
const PICKER_CHROME: u16 = 4;
const DIALOG_WIDTH: u16 = 76;
/// The dialog's rows besides its list: borders, the filter and the gap under it.
const DIALOG_CHROME: u16 = 4;
const DIALOG_ROWS: u16 = 16;
const MIN_DIALOG_ROWS: u16 = 4;
const SCREEN_MARGIN: u16 = 2;
const LIST_ROW: u16 = 2;

fn choice_text(state: &AppState, choice: &FolderChoice, width: usize) -> String {
    let mark = if choice.pinned { PINNED } else { UNPINNED };
    let room = width.saturating_sub(mark.width());
    format!("{mark}{}", format::truncate_start(&state.folders.display(&choice.path), room))
}

/// Where the picker sits: above the command line from its left edge, its best match in the
/// bottom row, as wide as its longest folder and as tall as the panels allow.
pub(crate) fn picker_rect(state: &AppState, layout: &Layout, choices: &[FolderChoice]) -> Rect {
    let prompt = layout.prompt;
    let room = prompt.y.saturating_sub(layout.panels[0].y);
    let rows = (choices.len() as u16).saturating_add(2).min(room);
    let widest = choices.iter().map(|c| state.folders.display(&c.path).width() + PINNED.width()).max().unwrap_or(0);
    let wanted = (widest as u16).max(tr!(cd_picker_hint).width() as u16).saturating_add(PICKER_CHROME);
    let width = wanted.min(prompt.width);
    Rect { x: prompt.x, y: prompt.y.saturating_sub(rows), width, height: rows }
}

/// Which choice a row of the picker shows, counted from the bottom.
pub(crate) fn picker_choice_at(area: Rect, row: u16) -> Option<usize> {
    let inner = area.inner(Margin::new(1, 1));
    (row >= inner.y && row < inner.bottom()).then(|| usize::from(inner.bottom() - 1 - row))
}

pub(crate) fn draw_picker(state: &AppState, layout: &Layout, theme: &Theme, buf: &mut Buffer) {
    let Some(choices) = borg_core::cd_choices(state) else { return };
    let area = picker_rect(state, layout, &choices);
    if area.height < 3 {
        return;
    }
    Clear.render(area, buf);
    Block::bordered()
        .style(theme.styles.overlay)
        .border_style(theme.styles.overlay_border)
        .title(tr!(cd_picker_hint))
        .render(area, buf);
    let inner = area.inner(Margin::new(1, 1));
    let width = usize::from(inner.width.saturating_sub(2));
    for (index, choice) in choices.iter().enumerate().take(usize::from(inner.height)) {
        let row = Rect { y: inner.bottom() - 1 - index as u16, height: 1, ..inner };
        let style =
            if state.cd_picker.selected == Some(index) { theme.styles.overlay_selected } else { theme.styles.overlay };
        buf.set_style(row, style);
        Line::styled(format!(" {}", choice_text(state, choice, width)), style).render(row, buf);
    }
}

/// The dialog, as tall as all the folders need, so it keeps its size while typing narrows it.
pub(crate) fn dialog_area(state: &AppState, screen: Rect) -> Rect {
    let folders = borg_core::folder_choices(state, "").len() as u16;
    centered(
        screen,
        screen.width.saturating_sub(SCREEN_MARGIN * 2).min(DIALOG_WIDTH),
        screen.height.saturating_sub(SCREEN_MARGIN).min(folders.clamp(MIN_DIALOG_ROWS, DIALOG_ROWS) + DIALOG_CHROME),
    )
}

pub(crate) fn dialog_list(state: &AppState, screen: Rect) -> Rect {
    let inner = dialog_area(state, screen).inner(Margin::new(2, 1));
    Rect { y: inner.y + LIST_ROW, height: inner.height.saturating_sub(LIST_ROW), ..inner }
}

/// The first folder the list shows, keeping the chosen one in view.
pub(crate) fn dialog_start(state: &AppState, dialog: &FoldersDialog, screen: Rect) -> usize {
    dialog.selected.saturating_sub(usize::from(dialog_list(state, screen).height).saturating_sub(1))
}

/// Draws the dialog, returning its area and where the filter's cursor belongs.
pub(crate) fn draw_dialog(
    state: &AppState,
    dialog: &FoldersDialog,
    theme: &Theme,
    screen: Rect,
    buf: &mut Buffer,
) -> (Rect, Position) {
    let area = dialog_area(state, screen);
    Clear.render(area, buf);
    Block::bordered()
        .style(theme.styles.overlay)
        .border_style(theme.styles.overlay_border)
        .title(tr!(folders_title))
        .title_bottom(tr!(folders_keys))
        .render(area, buf);
    let inner = area.inner(Margin::new(2, 1));
    let cursor = filter(dialog, theme, Rect { height: 1.min(inner.height), ..inner }, buf);

    let list = dialog_list(state, screen);
    let choices = borg_core::folder_choices(state, dialog.filter.text());
    let start = dialog_start(state, dialog, screen);
    for (offset, (index, choice)) in choices.iter().enumerate().skip(start).take(usize::from(list.height)).enumerate() {
        let row = Rect { y: list.y + offset as u16, height: 1, ..list };
        let style = if index == dialog.selected { theme.styles.overlay_selected } else { theme.styles.overlay };
        buf.set_style(row, style);
        Line::styled(choice_text(state, choice, usize::from(row.width)), style).render(row, buf);
    }
    if choices.is_empty() && !list.is_empty() {
        let empty = if dialog.filter.text().is_empty() { tr!(folders_empty) } else { tr!(folders_no_match) };
        Line::styled(empty, theme.styles.overlay).render(Rect { height: 1, ..list }, buf);
    }
    (area, cursor)
}

fn filter(dialog: &FoldersDialog, theme: &Theme, row: Rect, buf: &mut Buffer) -> Position {
    let label = format!("{} ", tr!(folders_filter));
    let label_width = (label.width() as u16).min(row.width);
    Line::styled(label, theme.styles.overlay).render(Rect { width: label_width, ..row }, buf);
    let field = Rect { x: row.x + label_width, width: row.width - label_width, ..row };
    buf.set_style(field, theme.styles.input);
    let input = &dialog.filter;
    let room = usize::from(field.width.saturating_sub(1));
    let before: String = input.text().chars().take(input.cursor_chars()).collect();
    let shift = before.width().saturating_sub(room);
    if input.text().is_empty() {
        let hint = theme.styles.input.add_modifier(Modifier::DIM);
        Line::styled(format::truncate_end(tr!(folders_filter_hint), room), hint).render(field, buf);
    } else {
        let shown = crate::dialog_view::skip_columns(input.text(), shift);
        Line::styled(format::truncate_end(&shown, room), Style::default()).render(field, buf);
    }
    Position::new(field.x + (before.width() - shift) as u16, field.y)
}
