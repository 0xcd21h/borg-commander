use borg_core::{Game2048, GameStatus};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{dialog_view::centered, format, look::Look};

const WIDTH: u16 = 44;
const HEIGHT: u16 = 22;
const SIDE: u16 = 4;
const CELL_WIDTH: u16 = 8;
const CELL_HEIGHT: u16 = 3;
const MIN_CELL_WIDTH: u16 = 4;
const BOARD_MARGIN: u16 = 2;
const MAX_COLOR_EXPONENT: u8 = 11;
const TILE_TINT: f32 = 0.28;

pub fn draw(game: Option<&Game2048>, look: &Look<'_>, screen: Rect, buf: &mut Buffer) -> Rect {
    let rect = centered(screen, WIDTH.min(screen.width), HEIGHT.min(screen.height));
    let styles = look.styles();
    Clear.render(rect, buf);
    let hint = [tr!(game2048_hint), tr!(game2048_hint_compact), tr!(game2048_hint_tiny)]
        .into_iter()
        .find(|hint| hint.width() <= usize::from(rect.width.saturating_sub(2)))
        .unwrap_or_default();
    let block = Block::bordered()
        .border_type(look.theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(format!(" {} ", tr!(game2048_title)), styles.overlay_border).centered())
        .title_bottom(Line::styled(hint, styles.overlay).centered());
    let inner = block.inner(rect);
    block.render(rect, buf);
    let Some(game) = game else { return rect };
    if inner.height == 0 || inner.width == 0 {
        return rect;
    }
    let score = format!(tr!(game2048_score), score = game.score(), best = game.best());
    Line::styled(format::truncate_end(&score, usize::from(inner.width)), styles.overlay)
        .centered()
        .render(Rect { height: 1, ..inner }, buf);
    let roomy = inner.height >= SIDE * CELL_HEIGHT + SIDE - 1 + BOARD_MARGIN * 2;
    let margin = if roomy { BOARD_MARGIN } else { 1 };
    let body = Rect {
        y: inner.y.saturating_add(margin).min(inner.bottom()),
        height: inner.height.saturating_sub(margin * 2),
        ..inner
    };
    if body.width < SIDE * MIN_CELL_WIDTH || body.height < SIDE {
        let message = if body.height == 0 { inner } else { body };
        Line::styled(tr!(game2048_resize), styles.overlay_border).centered().render(message, buf);
        return rect;
    }
    let gap_y = u16::from(body.height >= SIDE * 2 - 1);
    let gap_x = u16::from(body.width >= SIDE * MIN_CELL_WIDTH + SIDE - 1);
    let cell_width = ((body.width - gap_x * (SIDE - 1)) / SIDE).min(CELL_WIDTH);
    let cell_height = ((body.height - gap_y * (SIDE - 1)) / SIDE).min(CELL_HEIGHT);
    let board = centered(body, cell_width * SIDE + gap_x * (SIDE - 1), cell_height * SIDE + gap_y * (SIDE - 1));
    for (index, &exponent) in game.cells().iter().enumerate() {
        let row = index as u16 / SIDE;
        let column = index as u16 % SIDE;
        let cell = Rect {
            x: board.x + column * (cell_width + gap_x),
            y: board.y + row * (cell_height + gap_y),
            width: cell_width,
            height: cell_height,
        };
        let style = if exponent == 0 {
            styles.input.bg(look.bg(styles.input).into())
        } else {
            let color = look.accent().at(f32::from(exponent.min(MAX_COLOR_EXPONENT)) / f32::from(MAX_COLOR_EXPONENT));
            let background = look.bg(styles.overlay).mix(color, TILE_TINT);
            Style::new().fg(look.fg(styles.overlay).into()).bg(background.into()).add_modifier(Modifier::BOLD)
        };
        buf.set_style(cell, style);
        if exponent > 0 {
            let value = (1u64 << exponent).to_string();
            let text = if value.width() <= usize::from(cell_width) { value } else { format!("2^{exponent}") };
            Line::styled(text, style).centered().render(Rect { y: cell.y + cell.height / 2, height: 1, ..cell }, buf);
        }
    }
    let status = match game.status() {
        GameStatus::Playing => tr!(game2048_goal),
        GameStatus::Won => tr!(game2048_won),
        GameStatus::Over => tr!(game2048_over),
    };
    Line::styled(format::truncate_end(status, usize::from(inner.width)), styles.overlay_border)
        .centered()
        .render(Rect { y: inner.bottom() - 1, height: 1, ..inner }, buf);
    rect
}
