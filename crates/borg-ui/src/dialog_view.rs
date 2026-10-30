use borg_core::{Confirm, Prompt, PromptPurpose};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Position, Rect},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{Theme, format};

const MAX_WIDTH: u16 = 64;
const MARGIN: u16 = 4;
const PROMPT_HEIGHT: u16 = 6;
/// A prompt with a checkbox below its field.
const OPTION_HEIGHT: u16 = 8;
const CONFIRM_HEIGHT: u16 = 6;
const PADDING: u16 = 1;
const BUTTON_GAP: &str = "   ";

pub fn draw_prompt(prompt: &Prompt, theme: &Theme, screen: Rect, buf: &mut Buffer) -> (Rect, Position) {
    let styles = &theme.styles;
    let rect = prompt_rect(prompt, screen);
    let checkbox = prompt.purpose.option();
    let hint = match prompt.purpose {
        PromptPurpose::Copy { .. } => tr!(copy_dialog_hint),
        PromptPurpose::BackupAdd { .. } => tr!(backup_add_hint),
        _ => tr!(enter_ok_esc_cancel),
    };
    let hint = if checkbox.is_some() && hint.width() > usize::from(rect.width.saturating_sub(2)) {
        tr!(copy_dialog_hint_compact)
    } else {
        hint
    };
    frame(prompt.title, Some(hint), theme, rect, buf);
    let [label, field, _, option, _] = prompt_rows(rect, checkbox.is_some());

    let width = usize::from(field.width);
    Line::styled(format::truncate_end(&prompt.label, width), styles.overlay).render(label, buf);
    buf.set_style(field, styles.input);

    let text = prompt.input.text();
    let cursor = prompt.input.cursor_chars();
    let before: String = text.chars().take(cursor).collect();
    let shift = before.width().saturating_sub(width.saturating_sub(1));
    Line::styled(skip_columns(text, shift), styles.input).render(field, buf);

    if let Some((caption, checked)) = checkbox {
        let style = if prompt.option_focused { styles.overlay_selected } else { styles.overlay };
        let checked = if checked { 'x' } else { ' ' };
        let caption = format::truncate_end(&format!("[{checked}] {caption}"), usize::from(option.width));
        Line::styled(caption, style).render(option, buf);
        if prompt.option_focused && option.width > 0 && option.height > 0 {
            return (rect, Position::new(option.x + u16::from(option.width > 1), option.y));
        }
    }

    let column = u16::try_from(before.width() - shift).unwrap_or(0);
    (rect, Position::new(field.x + column, field.y))
}

fn prompt_rect(prompt: &Prompt, screen: Rect) -> Rect {
    let height = if prompt.purpose.option().is_some() { OPTION_HEIGHT } else { PROMPT_HEIGHT };
    centered(screen, dialog_width(screen), height)
}

fn prompt_rows(rect: Rect, checkbox: bool) -> [Rect; 5] {
    let inner = padded(Block::bordered().inner(rect));
    Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(u16::from(checkbox)),
        Constraint::Length(u16::from(checkbox)),
        Constraint::Fill(1),
    ])
    .areas(inner)
}

pub(crate) fn prompt_field_at(prompt: &Prompt, screen: Rect, point: Position) -> Option<bool> {
    let checkbox = prompt.purpose.option().is_some();
    let [_, field, _, option, _] = prompt_rows(prompt_rect(prompt, screen), checkbox);
    if field.contains(point) {
        Some(false)
    } else if checkbox && option.contains(point) {
        Some(true)
    } else {
        None
    }
}

pub fn draw_confirm(confirm: &Confirm, theme: &Theme, screen: Rect, buf: &mut Buffer) -> Rect {
    let styles = &theme.styles;
    let rect = confirm_rect(confirm, screen);
    let inner = frame(confirm.title, None, theme, rect, buf);
    let [message, _, buttons] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1), Constraint::Length(1)]).areas(inner);

    Paragraph::new(message_lines(&confirm.message, message.width))
        .style(styles.overlay)
        .centered()
        .render(message, buf);
    let (yes, no) = if confirm.accept {
        (styles.overlay_selected, styles.overlay)
    } else {
        (styles.overlay, styles.overlay_selected)
    };
    Line::from(vec![Span::styled(tr!(yes), yes), Span::styled(BUTTON_GAP, styles.overlay), Span::styled(tr!(no), no)])
        .centered()
        .render(buttons, buf);
    rect
}

/// Wraps a message between words; only a word wider than the dialog, such as a long
/// path, is broken inside.
fn message_lines(message: &str, width: u16) -> Vec<Line<'static>> {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    let width = usize::from(width.max(1));
    let mut lines = Vec::new();
    for paragraph in message.split('\n') {
        let mut text = String::new();
        for word in paragraph.split(' ') {
            let gap = usize::from(!text.is_empty());
            if text.width() + gap + word.width() <= width {
                if gap == 1 {
                    text.push(' ');
                }
                text.push_str(word);
                continue;
            }
            if !text.is_empty() {
                lines.push(Line::raw(std::mem::take(&mut text)));
            }
            for character in word.chars() {
                if text.width() + character.width().unwrap_or(0) > width {
                    lines.push(Line::raw(std::mem::take(&mut text)));
                }
                text.push(character);
            }
        }
        lines.push(Line::raw(text));
    }
    lines
}

fn confirm_rect(confirm: &Confirm, screen: Rect) -> Rect {
    let width = dialog_width(screen);
    let lines = message_lines(&confirm.message, width.saturating_sub(4)).len();
    let height =
        CONFIRM_HEIGHT.saturating_add(u16::try_from(lines.saturating_sub(1)).unwrap_or(u16::MAX)).min(screen.height);
    centered(screen, width, height)
}

pub(crate) fn confirm_at(confirm: &Confirm, screen: Rect, point: Position) -> Option<bool> {
    let rect = confirm_rect(confirm, screen);
    if !rect.contains(point)
        || rect.width < (tr!(yes).width() + BUTTON_GAP.len() + tr!(no).width()) as u16
        || rect.height < CONFIRM_HEIGHT
    {
        return None;
    }
    let start = rect.x + rect.width.saturating_sub((tr!(yes).width() + BUTTON_GAP.len() + tr!(no).width()) as u16) / 2;
    if point.y != rect.bottom().saturating_sub(2) {
        return None;
    }
    if (start..start + tr!(yes).width() as u16).contains(&point.x) {
        Some(true)
    } else if (start + (tr!(yes).width() + BUTTON_GAP.len()) as u16
        ..start + (tr!(yes).width() + BUTTON_GAP.len() + tr!(no).width()) as u16)
        .contains(&point.x)
    {
        Some(false)
    } else {
        None
    }
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
    padded(inner)
}

fn padded(inner: Rect) -> Rect {
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

#[cfg(test)]
mod tests {
    use super::message_lines;

    fn wrapped(message: &str, width: u16) -> Vec<String> {
        message_lines(message, width).iter().map(ToString::to_string).collect()
    }

    #[test]
    fn messages_wrap_between_words_and_break_only_words_wider_than_the_dialog() {
        assert_eq!(
            wrapped("Delete \"/tmp/a/node_modules\" permanently?", 30),
            ["Delete \"/tmp/a/node_modules\"", "permanently?"]
        );
        assert_eq!(
            wrapped("Delete /very/long/path/name permanently?", 10),
            ["Delete", "/very/long", "/path/name", "permanentl", "y?"]
        );
        assert_eq!(wrapped("First\nsecond line", 40), ["First", "second line"]);
    }
}

#[cfg(test)]
mod localized_button_tests {
    use super::*;

    #[test]
    fn confirmation_hit_areas_use_display_columns_for_every_language() {
        for language in borg_i18n::Locale::ALL {
            borg_i18n::with_locale(language, || {
                let confirm = Confirm {
                    title: tr!(quit),
                    message: tr!(quit_borg_commander).into(),
                    purpose: borg_core::ConfirmPurpose::Quit,
                    accept: false,
                };
                let screen = Rect::new(0, 0, 80, 24);
                let rect = confirm_rect(&confirm, screen);
                let yes = tr!(yes).width() as u16;
                let gap = BUTTON_GAP.width() as u16;
                let no = tr!(no).width() as u16;
                let start = rect.x + (rect.width - yes - gap - no) / 2;
                let y = rect.bottom() - 2;
                assert_eq!(confirm_at(&confirm, screen, Position::new(start, y)), Some(true));
                assert_eq!(confirm_at(&confirm, screen, Position::new(start + yes - 1, y)), Some(true));
                assert_eq!(confirm_at(&confirm, screen, Position::new(start + yes, y)), None);
                assert_eq!(confirm_at(&confirm, screen, Position::new(start + yes + gap, y)), Some(false));
                assert_eq!(confirm_at(&confirm, screen, Position::new(start + yes + gap + no - 1, y)), Some(false));
            });
        }
    }
}
