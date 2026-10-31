use borg_core::{Picker, ThemeChoice};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{Theme, dialog_view::centered};

const MIN_WIDTH: u16 = 28;
const SIDE_PADDING: u16 = 6;
const BORDER_ROWS: u16 = 2;

pub fn draw_picker(picker: &Picker, choices: &[ThemeChoice], theme: &Theme, area: Rect, buf: &mut Buffer) -> Rect {
    let names: Vec<&str> = choices.iter().map(|choice| choice.name.as_str()).collect();
    draw_names(picker, &names, tr!(picker_view_theme), theme, area, buf)
}

pub fn draw_languages(picker: &Picker, theme: &Theme, area: Rect, buf: &mut Buffer) -> Rect {
    let names: Vec<&str> = borg_i18n::Locale::ALL.iter().map(|language| language.name()).collect();
    draw_names(picker, &names, tr!(language), theme, area, buf)
}

/// The names a picker lists: the themes, or the languages when `choices` is `None`.
pub(crate) fn names(choices: Option<&[ThemeChoice]>) -> Vec<&str> {
    match choices {
        Some(choices) => choices.iter().map(|choice| choice.name.as_str()).collect(),
        None => borg_i18n::Locale::ALL.iter().map(|language| language.name()).collect(),
    }
}

/// The picker listing `names`, and the row of the name at `point`.
pub(crate) fn name_at(names: &[&str], area: Rect, point: ratatui::layout::Position) -> (Rect, Option<usize>) {
    let rect = rect(names, area);
    let inner = Rect {
        x: rect.x + 1,
        y: rect.y + 1,
        width: rect.width.saturating_sub(2),
        height: rect.height.saturating_sub(2),
    };
    let row = inner.contains(point).then(|| usize::from(point.y - inner.y)).filter(|row| *row < names.len());
    (rect, row)
}

fn rect(names: &[&str], area: Rect) -> Rect {
    let widest = names.iter().map(|name| name.width()).max().unwrap_or_default();
    let width = u16::try_from(widest)
        .unwrap_or(u16::MAX)
        .saturating_add(SIDE_PADDING)
        .max(MIN_WIDTH)
        .max(u16::try_from(tr!(enter_keep_esc_cancel).width()).unwrap_or(u16::MAX).saturating_add(SIDE_PADDING));
    let height = u16::try_from(names.len()).unwrap_or(u16::MAX).saturating_add(BORDER_ROWS);
    centered(area, width, height)
}

fn draw_names(picker: &Picker, names: &[&str], title: &str, theme: &Theme, area: Rect, buf: &mut Buffer) -> Rect {
    let styles = &theme.styles;
    let rect = rect(names, area);

    Clear.render(rect, buf);
    let block = Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(title, styles.overlay_border).centered())
        .title_bottom(Line::styled(tr!(enter_keep_esc_cancel), styles.overlay).centered());
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
