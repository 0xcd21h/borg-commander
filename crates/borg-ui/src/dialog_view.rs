use borg_core::{Confirm, Prompt};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Position, Rect},
    text::{Line, Span},
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{Theme, format};

const MAX_WIDTH: u16 = 64;
const MARGIN: u16 = 4;
const PROMPT_HEIGHT: u16 = 6;
const CONFIRM_HEIGHT: u16 = 6;
const PADDING: u16 = 1;
const YES: &str = "[ Yes ]";
const NO: &str = "[ No ]";
const BUTTON_GAP: &str = "   ";
const PROMPT_HINT: &str = " Enter ok · Esc cancel ";

pub fn draw_prompt(prompt: &Prompt, theme: &Theme, screen: Rect, buf: &mut Buffer) -> (Rect, Position) {
    let styles = &theme.styles;
    let rect = centered(screen, dialog_width(screen), PROMPT_HEIGHT);
    let inner = frame(prompt.title, Some(PROMPT_HINT), theme, rect, buf);
    let [label, field] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(inner);

    let width = usize::from(field.width);
    Line::styled(format::truncate_end(&prompt.label, width), styles.overlay).render(label, buf);
    buf.set_style(field, styles.input);

    let text = prompt.input.text();
    let cursor = prompt.input.cursor_chars();
    let before: String = text.chars().take(cursor).collect();
    let shift = before.width().saturating_sub(width.saturating_sub(1));
    Line::styled(skip_columns(text, shift), styles.input).render(field, buf);

    let column = u16::try_from(before.width() - shift).unwrap_or(0);
    (rect, Position::new(field.x + column, field.y))
}

pub fn draw_confirm(confirm: &Confirm, theme: &Theme, screen: Rect, buf: &mut Buffer) -> Rect {
    let styles = &theme.styles;
    let rect = centered(screen, dialog_width(screen), CONFIRM_HEIGHT);
    let inner = frame(confirm.title, None, theme, rect, buf);
    let [message, _, buttons] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)]).areas(inner);

    Line::styled(format::truncate_end(&confirm.message, usize::from(message.width)), styles.overlay)
        .centered()
        .render(message, buf);
    let (yes, no) = if confirm.accept {
        (styles.overlay_selected, styles.overlay)
    } else {
        (styles.overlay, styles.overlay_selected)
    };
    Line::from(vec![Span::styled(YES, yes), Span::styled(BUTTON_GAP, styles.overlay), Span::styled(NO, no)])
        .centered()
        .render(buttons, buf);
    rect
}

fn frame(title: &str, hint: Option<&str>, theme: &Theme, rect: Rect, buf: &mut Buffer) -> Rect {
    let styles = &theme.styles;
    Clear.render(rect, buf);
    let block = Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(format!(" {title} "), styles.overlay_border).centered());
    let block = match hint {
        Some(hint) => block.title_bottom(Line::styled(hint.to_owned(), styles.overlay).centered()),
        None => block,
    };
    let inner = block.inner(rect);
    block.render(rect, buf);
    Rect {
        x: inner.x + PADDING,
        y: inner.y + PADDING,
        width: inner.width.saturating_sub(PADDING * 2),
        height: inner.height.saturating_sub(PADDING),
    }
}

fn dialog_width(screen: Rect) -> u16 {
    screen.width.saturating_sub(MARGIN).min(MAX_WIDTH)
}

pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [row] = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center).areas(area);
    let [rect] = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center).areas(row);
    rect
}

pub(crate) fn skip_columns(text: &str, columns: usize) -> String {
    let mut skipped = 0;
    text.chars()
        .skip_while(|c| {
            let skip = skipped < columns;
            skipped += unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0);
            skip
        })
        .collect()
}
