use borg_core::{AppState, Overlay, settings_items};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{Block, Clear, Widget},
};

use crate::{Theme, dialog_view::centered};

const WIDTH: u16 = 56;
const PADDING: u16 = 2;

pub(crate) fn rect(state: &AppState, screen: Rect) -> Rect {
    centered(screen, WIDTH.min(screen.width), (settings_items(state).len() as u16 + PADDING * 2).min(screen.height))
}

pub(crate) fn item_at(state: &AppState, point: ratatui::layout::Position, screen: Rect) -> Option<usize> {
    let rect = rect(state, screen);
    let y = point.y.checked_sub(rect.y + PADDING)?;
    let selected = match state.overlay {
        Some(Overlay::Settings { selected }) => selected,
        _ => 0,
    };
    let rows = rect.height.saturating_sub(PADDING * 2);
    let first = selected.saturating_sub(usize::from(rows.saturating_sub(1)));
    (rect.contains(point) && y < rows && first + usize::from(y) < settings_items(state).len())
        .then_some(first + usize::from(y))
}

pub(crate) fn draw(state: &AppState, selected: usize, theme: &Theme, screen: Rect, buf: &mut Buffer) -> Rect {
    let rect = rect(state, screen);
    Clear.render(rect, buf);
    Block::bordered()
        .border_type(theme.border.into())
        .style(theme.styles.overlay)
        .border_style(theme.styles.overlay_border)
        .title(" Settings ")
        .title_bottom(" Space toggle · Esc close · saved immediately ")
        .render(rect, buf);
    let body = Rect {
        x: rect.x + PADDING,
        y: rect.y + PADDING,
        width: rect.width.saturating_sub(PADDING * 2),
        height: rect.height.saturating_sub(PADDING * 2),
    };
    let first = selected.saturating_sub(usize::from(body.height.saturating_sub(1)));
    for (index, (y, item)) in (body.top()..body.bottom()).zip(settings_items(state).into_iter().skip(first)).enumerate()
    {
        let index = first + index;
        let style = if index == selected { theme.styles.overlay_selected } else { theme.styles.overlay };
        let row = Rect { y, height: 1, ..body };
        buf.set_style(row, style);
        Line::styled(format!("[{}] {}", if item.checked == Some(true) { "x" } else { " " }, item.label), style)
            .render(row, buf);
    }
    rect
}
