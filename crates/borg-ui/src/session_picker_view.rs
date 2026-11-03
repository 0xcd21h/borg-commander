//! The window Borg opens on start to choose a session: what a session is, the sessions with when
//! each was last used and what runs it now, a field for a new one's name, and the question before
//! removing one.

use borg_core::{PickerInput, PickerStep, SessionEntry, SessionPicker, SessionUse};
use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use jiff::Zoned;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Position, Rect},
    text::{Line, Span},
    widgets::{Block, Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{Palette, Theme, branding, dialog_view, format, fx, hints};

const MAX_WIDTH: u16 = 84;
const MARGIN: u16 = 4;
const GAP: &str = "  ";
const MARK: &str = "● ";
/// The fewest cells a session's name keeps when the columns do not all fit.
const NAME_MIN: usize = 12;
/// The bottom row, which holds Borg's badge.
const BADGE_ROWS: u16 = 1;
/// Rows the list keeps when there are fewer sessions.
const MIN_LIST_ROWS: u16 = 3;
/// The borders and the blank row under the top one.
const CHROME_ROWS: u16 = 3;
/// The blank row between the explanation and the list.
const ABOUT_GAP: u16 = 1;
/// The column headings.
const HEADING_ROWS: u16 = 1;
/// A blank row, then two for the notice or what the chosen session's state means.
const FOOTER_ROWS: u16 = 3;
/// The field for a new session's name: its label, then the field.
const NAMING_ROWS: u16 = 2;
const YES_KEY: char = 'y';
const NO_KEY: char = 'n';

/// What a screen looks like in the colors Borg uses: the theme, the terminal's palette, and
/// whether the terminal shows full color.
pub struct StartLook<'a> {
    pub theme: &'a Theme,
    pub palette: &'a Palette,
    pub truecolor: bool,
}

/// Where the window and its parts are on a screen. The explanation gives way on a screen too
/// short for it and a few sessions.
struct Areas {
    window: Rect,
    about: Rect,
    heading: Rect,
    list: Rect,
    footer: Rect,
    naming: Rect,
}

fn areas(picker: &SessionPicker, screen: Rect) -> Areas {
    let screen = Rect { height: screen.height.saturating_sub(BADGE_ROWS), ..screen };
    let width = screen.width.saturating_sub(MARGIN).min(MAX_WIDTH);
    let text = dialog_view::content(Rect { width, ..screen }).width;
    let naming = if matches!(picker.step, PickerStep::Naming(_)) { NAMING_ROWS } else { 0 };
    let rows = u16::try_from(picker.sessions.len()).unwrap_or(u16::MAX).max(MIN_LIST_ROWS);
    let sum = |rows: [u16; 5]| rows.into_iter().fold(0_u16, u16::saturating_add);
    let needed = sum([CHROME_ROWS, HEADING_ROWS, rows.min(MIN_LIST_ROWS), FOOTER_ROWS, naming]);
    let about = u16::try_from(dialog_view::message_lines(tr!(session_picker_about), text).len()).unwrap_or(u16::MAX);
    let fits = needed.saturating_add(about).saturating_add(ABOUT_GAP) <= screen.height;
    let (about, gap) = if fits { (about, ABOUT_GAP) } else { (0, 0) };
    let height =
        sum([CHROME_ROWS, about, gap, HEADING_ROWS, rows]).saturating_add(FOOTER_ROWS + naming).min(screen.height);
    let window = dialog_view::centered(screen, width, height);
    let [about, _, heading, list, footer, naming] = Layout::vertical([
        Constraint::Length(about),
        Constraint::Length(gap),
        Constraint::Length(HEADING_ROWS),
        Constraint::Min(1),
        Constraint::Length(FOOTER_ROWS),
        Constraint::Length(naming),
    ])
    .areas(dialog_view::content(window));
    Areas { window, about, heading, list, footer, naming }
}

/// The first session the list shows, so that the chosen one is in view.
fn first_row(picker: &SessionPicker, list: Rect) -> usize {
    picker.selected.saturating_sub(usize::from(list.height).saturating_sub(1))
}

fn row_at(picker: &SessionPicker, list: Rect, point: Position) -> Option<usize> {
    let row = list.contains(point).then(|| first_row(picker, list) + usize::from(point.y - list.y))?;
    (row < picker.sessions.len()).then_some(row)
}

/// The keys the bottom border lists while the window does what it does now.
fn keys(picker: &SessionPicker) -> &'static str {
    match picker.step {
        PickerStep::Naming(_) => tr!(enter_create_esc_return_to_list),
        PickerStep::Choosing | PickerStep::Removing { .. } => tr!(session_picker_keys),
    }
}

/// The question before removing the session `id`.
fn question(picker: &SessionPicker, id: &str) -> Option<String> {
    let entry = picker.sessions.iter().find(|entry| entry.id == id)?;
    Some(tr!(session_remove_confirm, name = entry.name))
}

/// What the chosen session's state means and what its keys do.
fn detail(entry: &SessionEntry) -> &'static str {
    match entry.in_use {
        Some(SessionUse::Terminal) => tr!(session_detail_terminal),
        Some(SessionUse::Service) => tr!(session_detail_service),
        None if entry.is_default() => tr!(session_detail_default),
        None => tr!(session_detail_idle),
    }
}

/// A session's name, when it was last used, and what runs it now.
fn cells(entry: &SessionEntry, clock: &Zoned) -> [String; 3] {
    let used = match entry.last_used {
        Some(_) => format::stamp(entry.last_used, clock),
        None => tr!(session_last_used_unknown).into(),
    };
    let status = match entry.in_use {
        Some(SessionUse::Terminal) => format!("{MARK}{}", tr!(session_status_terminal)),
        Some(SessionUse::Service) => format!("{MARK}{}", tr!(session_status_service)),
        None => String::new(),
    };
    [entry.name.clone(), used, status]
}

/// The widths of the columns in `width` cells: the last two as wide as their widest text and the
/// names in what is left, or when that is too little, the names first and the status cut short.
fn columns(rows: &[[String; 3]], headings: &[&str; 3], width: usize) -> [usize; 3] {
    let widest = |column: usize| {
        rows.iter().map(|row| row[column].width()).chain([headings[column].width()]).max().unwrap_or_default()
    };
    let (used, status, gaps) = (widest(1), widest(2), 2 * GAP.width());
    let name = width.saturating_sub(used + status + gaps).max(widest(0).min(NAME_MIN));
    [name, used, width.saturating_sub(name + used + gaps)]
}

fn fitted(text: &str, width: usize) -> String {
    format::pad_end(&format::truncate_end(text, width), width)
}

/// Draws the window on a screen of its own and gives the cursor's place while a name is typed.
pub fn draw_session_picker(
    picker: &SessionPicker,
    look: &StartLook<'_>,
    clock: &Zoned,
    buf: &mut Buffer,
) -> Option<Position> {
    let screen = buf.area;
    let theme = look.theme;
    let styles = &theme.styles;
    Block::new().style(styles.panel).render(screen, buf);
    let areas = areas(picker, screen);
    dialog_view::frame(tr!(session_picker_title), Some(keys(picker)), theme, areas.window, buf);
    Paragraph::new(dialog_view::message_lines(tr!(session_picker_about), areas.about.width))
        .style(styles.overlay)
        .render(areas.about, buf);

    let headings = [tr!(session_column_name), tr!(session_column_last_used), tr!(session_column_status)];
    let rows: Vec<[String; 3]> = picker.sessions.iter().map(|entry| cells(entry, clock)).collect();
    let widths = columns(&rows, &headings, usize::from(areas.list.width));
    let heading = headings.iter().zip(widths).map(|(text, width)| fitted(text, width)).collect::<Vec<_>>();
    Line::styled(heading.join(GAP), styles.overlay_border).render(areas.heading, buf);
    let first = first_row(picker, areas.list);
    for (offset, (at, row)) in rows.iter().enumerate().skip(first).take(usize::from(areas.list.height)).enumerate() {
        let area = Rect { y: areas.list.y + offset as u16, height: 1, ..areas.list };
        let chosen = at == picker.selected;
        let style = if chosen { styles.overlay_selected } else { styles.overlay };
        let status = if chosen { style } else { style.patch(styles.key) };
        buf.set_style(area, style);
        Line::from(vec![
            Span::styled(format!("{}{GAP}{}{GAP}", fitted(&row[0], widths[0]), fitted(&row[1], widths[1])), style),
            Span::styled(fitted(&row[2], widths[2]), status),
        ])
        .render(area, buf);
    }

    let said = picker.notice.as_deref().or_else(|| picker.chosen().map(detail));
    if let Some(said) = said {
        let lines = Rect { y: areas.footer.y + 1, height: areas.footer.height.saturating_sub(1), ..areas.footer };
        Paragraph::new(dialog_view::message_lines(said, lines.width)).style(styles.overlay).render(lines, buf);
    }
    let cursor = match &picker.step {
        PickerStep::Naming(input) => Some(draw_name(input, theme, areas.naming, buf)),
        PickerStep::Removing { id, accept } => {
            if let Some(message) = question(picker, id) {
                fx::fade_to_background(buf, screen, crate::draw::BACKDROP, look.palette);
                dialog_view::draw_yes_no(tr!(session_remove_title), &message, *accept, theme, screen, buf);
            }
            None
        }
        PickerStep::Choosing => None,
    };
    branding::draw(screen, theme, look.palette, buf);
    if !look.truecolor {
        fx::quantize(buf, look.palette);
    }
    cursor.filter(|position| screen.contains(*position))
}

/// The field for a new session's name under its label, scrolled to keep the cursor in view.
fn draw_name(input: &borg_core::LineInput, theme: &Theme, area: Rect, buf: &mut Buffer) -> Position {
    let styles = &theme.styles;
    let [label, field] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);
    Line::styled(tr!(session_new_name), styles.overlay).render(label, buf);
    buf.set_style(field, styles.input);
    let before: String = input.text().chars().take(input.cursor_chars()).collect();
    let shift = before.width().saturating_sub(usize::from(field.width).saturating_sub(1));
    Line::styled(dialog_view::skip_columns(input.text(), shift), styles.input).render(field, buf);
    Position::new(field.x + u16::try_from(before.width() - shift).unwrap_or_default(), field.y)
}

/// What the mouse does in the window: a click chooses a row and a click on the chosen one opens
/// it, the wheel moves the choice, and clicks on the keys in the border or on the question's
/// buttons press them.
pub fn session_picker_mouse(picker: &SessionPicker, event: MouseEvent, screen: Rect) -> Option<PickerInput> {
    let point = Position::new(event.column, event.row);
    match event.kind {
        MouseEventKind::ScrollUp => return Some(PickerInput::Scroll(-1)),
        MouseEventKind::ScrollDown => return Some(PickerInput::Scroll(1)),
        MouseEventKind::Down(MouseButton::Left) => {}
        _ => return None,
    }
    let areas = areas(picker, screen);
    let pressed = |key: KeyEvent| Some(PickerInput::Key(key));
    match &picker.step {
        PickerStep::Removing { id, .. } => {
            let accept = dialog_view::yes_no_at(&question(picker, id)?, screen, point)?;
            pressed(KeyCode::Char(if accept { YES_KEY } else { NO_KEY }).into())
        }
        PickerStep::Naming(_) => hints::bottom(areas.window, keys(picker), true, point).and_then(pressed),
        PickerStep::Choosing => match row_at(picker, areas.list, point) {
            Some(row) if row == picker.selected => pressed(KeyCode::Enter.into()),
            Some(row) => Some(PickerInput::Row(row)),
            None => hints::bottom(areas.window, keys(picker), true, point).and_then(pressed),
        },
    }
}

#[cfg(test)]
mod tests {
    use borg_core::{DEFAULT_SESSION, SessionChoice};
    use crossterm::event::KeyModifiers;

    use super::*;

    const SCREEN: Rect = Rect { x: 0, y: 0, width: 90, height: 24 };

    fn picker() -> SessionPicker {
        let entry =
            |id: &str, name: &str, in_use| SessionEntry { id: id.into(), name: name.into(), last_used: None, in_use };
        SessionPicker::new(vec![
            entry(DEFAULT_SESSION, "Default", None),
            entry("a1", "scratch", Some(SessionUse::Service)),
            entry("b2", "old", None),
        ])
    }

    fn click(x: u16, y: u16) -> MouseEvent {
        MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE }
    }

    fn word(picker: &SessionPicker, rect: Rect, text: &str) -> u16 {
        let hint = keys(picker);
        let room = rect.width - 2;
        let start = rect.x + 1 + (room - hint.width() as u16) / 2;
        start + hint[..hint.find(text).unwrap()].width() as u16
    }

    #[test]
    fn a_click_chooses_a_row_and_a_second_one_opens_it() {
        let mut picker = picker();
        let list = areas(&picker, SCREEN).list;
        let input = session_picker_mouse(&picker, click(list.x + 2, list.y + 2), SCREEN);
        assert_eq!(input, Some(PickerInput::Row(2)));
        picker.update(input.unwrap());
        let input = session_picker_mouse(&picker, click(list.x + 2, list.y + 2), SCREEN).unwrap();
        assert_eq!(picker.update(input), Some(SessionChoice::Open("b2".into())));
        assert_eq!(session_picker_mouse(&picker, click(list.x, list.y + 3), SCREEN), None, "below the last session");
    }

    #[test]
    fn the_keys_in_the_border_and_the_question_buttons_take_clicks() {
        let mut picker = picker();
        picker.update(PickerInput::Row(2));
        let window = areas(&picker, SCREEN).window;
        let remove = session_picker_mouse(&picker, click(word(&picker, window, "F8"), window.bottom() - 1), SCREEN);
        assert_eq!(remove, Some(PickerInput::Key(KeyCode::F(8).into())));
        picker.update(remove.unwrap());
        assert!(matches!(picker.step, PickerStep::Removing { .. }));
        let message = question(&picker, "b2").unwrap();
        let yes =
            SCREEN.positions().find(|point| dialog_view::yes_no_at(&message, SCREEN, *point) == Some(true)).unwrap();
        let input = session_picker_mouse(&picker, click(yes.x, yes.y), SCREEN).unwrap();
        assert_eq!(picker.update(input), Some(SessionChoice::Remove("b2".into())));
    }

    #[test]
    fn the_name_field_keeps_the_cursor_after_the_typed_text() {
        let mut picker = picker();
        picker.update(PickerInput::Key(KeyCode::F(7).into()));
        for letter in "日本".chars() {
            picker.update(PickerInput::Key(KeyCode::Char(letter).into()));
        }
        let mut buf = Buffer::empty(SCREEN);
        let themes = crate::ThemeSet::builtin().unwrap();
        let look = StartLook { theme: themes.find(None), palette: &Palette::default(), truecolor: true };
        let cursor = draw_session_picker(&picker, &look, &jiff::Zoned::now(), &mut buf).unwrap();
        let field = areas(&picker, SCREEN).naming;
        assert_eq!(cursor, Position::new(field.x + 4, field.y + 1));
        let window = areas(&picker, SCREEN).window;
        let cancel = session_picker_mouse(&picker, click(word(&picker, window, "Esc"), window.bottom() - 1), SCREEN);
        assert_eq!(cancel, Some(PickerInput::Key(KeyCode::Esc.into())));
    }
}
