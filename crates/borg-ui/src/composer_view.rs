use borg_core::Composer;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    text::{Line, Span},
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{dialog_view::skip_columns, format, look::Look};

const MAX_VISIBLE_LINES: usize = 12;
const BORDER_ROWS: u16 = 2;
const BORDER_COLUMNS: u16 = 2;
const BELOW_ANCHOR: u16 = 2;
const GUTTER_GAP: usize = 1;
const LINE_NOUN: &str = "line";
const HINT: &str = " Enter new line · Alt-Enter run · Esc back to prompt ";
const SHORT_HINT: &str = " M-Enter run · Esc ";

pub fn draw_composer(
    composer: &Composer,
    look: &Look<'_>,
    anchor: Rect,
    screen: Rect,
    buf: &mut Buffer,
) -> (Rect, Position) {
    let styles = look.styles();
    let lines: Vec<&str> = composer.lines().collect();
    let bottom = (anchor.y + BELOW_ANCHOR).min(screen.bottom());
    let room = usize::from(bottom.saturating_sub(screen.y).saturating_sub(BORDER_ROWS));
    let visible = lines.len().clamp(1, MAX_VISIBLE_LINES.min(room.max(1)));
    let height = u16::try_from(visible).unwrap_or(u16::MAX) + BORDER_ROWS;
    let rect = Rect { x: anchor.x, y: bottom.saturating_sub(height).max(screen.y), width: anchor.width, height };

    let hint = if usize::from(rect.width) > HINT.width() + BORDER_COLUMNS as usize { HINT } else { SHORT_HINT };
    let title = format!(" {} ", format::count(lines.len(), LINE_NOUN));
    Clear.render(rect, buf);
    let block = Block::bordered()
        .border_type(look.theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(title, styles.overlay_border))
        .title_bottom(Line::styled(hint, styles.overlay).right_aligned());
    let inner = block.inner(rect);
    block.render(rect, buf);

    let (row, column) = composer.caret();
    let first = row.saturating_sub(visible - 1);
    let gutter = lines.len().to_string().len() + GUTTER_GAP;
    let text_width = usize::from(inner.width).saturating_sub(gutter);
    let before: String = lines.get(row).map(|line| line.chars().take(column).collect()).unwrap_or_default();
    let shift = before.width().saturating_sub(text_width.saturating_sub(1));

    for (offset, (number, line)) in lines.iter().enumerate().skip(first).take(visible).enumerate() {
        let Ok(y) = u16::try_from(offset).map(|offset| inner.y + offset) else { break };
        let label = format!("{:>width$}{}", number + 1, " ".repeat(GUTTER_GAP), width = gutter - GUTTER_GAP);
        let shown = format::truncate_end(&skip_columns(line, shift), text_width);
        Line::from(vec![Span::styled(label, styles.meta), Span::styled(shown, styles.overlay)])
            .render(Rect { y, height: 1, ..inner }, buf);
    }

    let x = inner.x + u16::try_from(gutter + before.width() - shift).unwrap_or_default();
    let y = inner.y + u16::try_from(row - first).unwrap_or_default();
    (rect, Position::new(x.min(inner.right().saturating_sub(1)), y))
}
