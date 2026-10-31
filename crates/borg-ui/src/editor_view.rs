use borg_core::{Editor, EditorDialog, Msg, Pointer, editor_dialog, editor_status};
use borg_i18n::tr;
use borg_view::Paint;
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{Palette, Theme, dialog_view::centered, format};

const DIALOG_WIDTH: u16 = 64;
const BUTTON_GAP: u16 = 2;

/// The editor's title, text and status rows on `area`.
fn rows(area: Rect) -> [Rect; 3] {
    Layout::vertical([Constraint::Length(1), Constraint::Fill(1), Constraint::Length(1)]).areas(area)
}

/// Where a dialog's parts sit: its frame, its fields and its buttons.
struct DialogLayout {
    rect: Rect,
    message: Rect,
    labels: Vec<Rect>,
    fields: Vec<Rect>,
    buttons: Vec<Rect>,
}

/// The dialog over the text, kept off the cursor's line: low on the screen, or high when the
/// cursor is there.
fn dialog_layout(editor: &Editor, dialog: &EditorDialog<'_>, area: Rect) -> DialogLayout {
    let [_, body, _] = rows(area);
    let fields = dialog.fields.len() as u16;
    let height = 5 + u16::from(dialog.message.is_some()) + fields;
    let width = area.width.saturating_sub(4).min(DIALOG_WIDTH);
    let low = body.bottom().saturating_sub(height + 1).max(body.y);
    let cursor = body.y as usize + editor.buffer.position().0.saturating_sub(editor.top);
    let y = if (low as usize..(low + height) as usize).contains(&cursor) { body.y + 1 } else { low };
    let rect = Rect { y, height: height.min(body.height), ..centered(area, width, height) };
    let inner = Rect { x: rect.x + 2, y: rect.y + 1, width: rect.width.saturating_sub(4), height: 1 };
    let message = Rect { y: inner.y + 1, ..inner };
    let first = message.y + u16::from(dialog.message.is_some());
    let label_width =
        dialog.fields.iter().map(|(label, _)| label.width() as u16).max().unwrap_or(0).min(inner.width / 2);
    let labels = (0..fields).map(|row| Rect { y: first + row, width: label_width, ..inner }).collect();
    let field_x = inner.x + label_width + BUTTON_GAP;
    let fields = (0..fields)
        .map(|row| Rect { x: field_x, y: first + row, width: inner.right().saturating_sub(field_x), height: 1 })
        .collect();
    let widths: Vec<u16> = dialog.buttons.iter().map(|label| label.width() as u16 + 4).collect();
    let total = widths.iter().sum::<u16>() + BUTTON_GAP * widths.len().saturating_sub(1) as u16;
    let mut x = inner.x + inner.width.saturating_sub(total) / 2;
    let y = first + dialog.fields.len() as u16 + 1;
    let buttons = widths
        .into_iter()
        .map(|width| {
            let button = Rect { x, y, width, height: 1 };
            x += width + BUTTON_GAP;
            button
        })
        .collect();
    DialogLayout { rect, message, labels, fields, buttons }
}

/// Draws the dialog open over the editor, returning where its field's cursor belongs.
fn draw_dialog(editor: &Editor, theme: &Theme, area: Rect, buf: &mut Buffer) -> Option<Option<Position>> {
    let dialog = editor_dialog(editor)?;
    let layout = dialog_layout(editor, &dialog, area);
    let styles = &theme.styles;
    Clear.render(layout.rect, buf);
    Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(dialog.title, styles.overlay_border).centered())
        .title_bottom(Line::styled(dialog.keys, styles.overlay).centered())
        .render(layout.rect, buf);
    if let Some(message) = &dialog.message {
        let text = format::truncate_end(message, usize::from(layout.message.width));
        Line::styled(text, styles.overlay).render(layout.message, buf);
    }
    let mut cursor = None;
    for (index, ((label, input), (label_area, field))) in
        dialog.fields.iter().zip(layout.labels.iter().zip(&layout.fields)).enumerate()
    {
        Line::styled(*label, styles.overlay).render(*label_area, buf);
        let focused = dialog.focus == index;
        buf.set_style(*field, if focused { styles.overlay_selected } else { styles.input });
        let room = usize::from(field.width.saturating_sub(2));
        let before: String = input.text().chars().take(input.cursor_chars()).collect();
        let shift = before.width().saturating_sub(room.saturating_sub(1));
        let shown = crate::dialog_view::skip_columns(input.text(), shift);
        let style = if focused { styles.overlay_selected } else { styles.input };
        Line::styled(format!(" {}", format::truncate_end(&shown, room)), style).render(*field, buf);
        if focused {
            cursor = Some(Position::new(field.x + 1 + (before.width() - shift) as u16, field.y));
        }
    }
    for (index, (label, button)) in dialog.buttons.iter().zip(&layout.buttons).enumerate() {
        let focused = dialog.focus == dialog.fields.len() + index;
        let style = if focused { styles.overlay_selected } else { styles.menu };
        Line::styled(format!("[ {label} ]"), style).render(*button, buf);
    }
    Some(cursor.filter(|cursor| layout.rect.contains(*cursor)))
}

/// What a mouse event in the editor does: in its dialog, a field or button; in the text, a
/// click, drag or double click; the wheel scrolls; and the keys named in the status line or
/// the dialog's border take clicks.
pub(crate) fn mouse(editor: &Editor, event: MouseEvent, double: bool, area: Rect, wheel: isize) -> Option<Msg> {
    let point = Position::new(event.column, event.row);
    let left = |kind| matches!(kind, MouseEventKind::Down(MouseButton::Left));
    match event.kind {
        MouseEventKind::ScrollUp => return Some(Msg::Pointer(Pointer::EditorScroll(-wheel))),
        MouseEventKind::ScrollDown => return Some(Msg::Pointer(Pointer::EditorScroll(wheel))),
        _ => {}
    }
    if let Some(dialog) = editor_dialog(editor) {
        if !left(event.kind) {
            return None;
        }
        let layout = dialog_layout(editor, &dialog, area);
        let fields = layout.fields.iter().chain(&layout.buttons);
        if let Some(control) = fields.into_iter().position(|rect| rect.contains(point)) {
            return Some(Msg::Pointer(Pointer::EditorControl(control)));
        }
        return crate::hints::bottom(layout.rect, dialog.keys, true, point).map(Msg::Key);
    }
    let [_, body, status] = rows(area);
    let drag = matches!(event.kind, MouseEventKind::Drag(MouseButton::Left));
    if (left(event.kind) || drag) && body.contains(point) {
        let text_x = body.x + editor.gutter().min(body.width);
        return Some(Msg::Pointer(Pointer::EditorClick {
            line: editor.top + usize::from(point.y - body.y),
            column: editor.left + usize::from(point.x.saturating_sub(text_x)),
            select: drag || event.modifiers.contains(KeyModifiers::SHIFT),
            word: double && !drag,
        }));
    }
    let status = crate::branding::content(status);
    (left(event.kind) && status.contains(point) && editor_status(editor).is_none())
        .then(|| crate::hints::key_at(tr!(editor_hint), status.x + 1, point.x))
        .flatten()
        .map(Msg::Key)
}

/// Draws the editor over `area`, returning where the terminal cursor belongs.
pub(crate) fn draw_editor(editor: &Editor, theme: &Theme, area: Rect, buf: &mut Buffer) -> Option<Position> {
    let [title, body, status] = rows(area);
    let styles = &theme.styles;
    let base = styles.viewer;
    buf.set_style(body, base);
    buf.set_style(title, styles.viewer_status);
    buf.set_style(status, styles.viewer_status);
    draw_title(editor, styles.viewer_status, title, buf);

    let background = Palette::default().background_of(base.bg.unwrap_or(Color::Reset));
    let light = u32::from(background.r) + u32::from(background.g) + u32::from(background.b) > 3 * 128;
    let gutter = editor.gutter().min(body.width);
    let numbers = base.fg(styles.meta.fg.unwrap_or(Color::Reset));
    let text = Rect { x: body.x + gutter, width: body.width - gutter, ..body };
    let buffer = &editor.buffer;
    let selection = buffer.selection();
    for (y, line) in (body.top()..body.bottom()).zip(editor.top..buffer.lines()) {
        let number = format!("{:>width$} ", line + 1, width = usize::from(gutter.saturating_sub(2)));
        buf.set_stringn(body.x, y, number, usize::from(gutter), numbers);
        let start = buffer.line_start(line);
        let selected =
            selection.clone().map(|range| range.start.saturating_sub(start)..range.end.saturating_sub(start));
        let runs = editor.painted.as_ref().and_then(|(_, first, lines)| lines.get(line.checked_sub(*first)?));
        let look = LineLook { base, selection: styles.cursor, light };
        draw_line(&buffer.line(line), runs.map(Vec::as_slice), selected, editor.left, (text, y), look, buf);
    }

    let (line, _) = buffer.position();
    let cursor = (line >= editor.top && buffer.screen_column() >= editor.left).then(|| Position {
        x: text.x.saturating_add((buffer.screen_column() - editor.left) as u16),
        y: body.y.saturating_add((line - editor.top) as u16),
    });
    draw_status(editor, styles.viewer_status, crate::branding::content(status), buf);
    match draw_dialog(editor, theme, area, buf) {
        Some(field) => field,
        None => cursor.filter(|cursor| text.contains(*cursor)),
    }
}

fn draw_title(editor: &Editor, style: Style, area: Rect, buf: &mut Buffer) {
    let buffer = &editor.buffer;
    let eol = match buffer.eol {
        borg_edit::Eol::Lf => "LF",
        borg_edit::Eol::Crlf => "CRLF",
    };
    let indent = match buffer.indent {
        borg_edit::Indent::Tabs => "Tab".to_owned(),
        borg_edit::Indent::Spaces(width) => format!("{width}␣"),
    };
    let language = editor.language.as_deref().map(|language| format!(" · {language}")).unwrap_or_default();
    let facts = format!("{eol} · {indent}{language} ");
    let modified = if buffer.modified() { format!(" · {}", tr!(editor_modified)) } else { String::new() };
    let room = usize::from(area.width).saturating_sub(facts.chars().count() + modified.chars().count() + 2);
    let path = format::truncate_start(&editor.path.display().to_string(), room);
    Line::styled(format!(" {path}{modified}"), style).render(area, buf);
    Line::styled(facts, style).right_aligned().render(area, buf);
}

/// The note, or the keys and the cursor position.
fn draw_status(editor: &Editor, style: Style, area: Rect, buf: &mut Buffer) {
    let (line, column) = editor.buffer.position();
    let position = format!("{} ", tr!(editor_position, line = line + 1, column = column + 1));
    match editor_status(editor) {
        Some(note) => Line::styled(format!(" {note}"), style).render(area, buf),
        None => {
            let room = usize::from(area.width).saturating_sub(position.chars().count() + 2);
            Line::styled(format!(" {}", format::truncate_end(tr!(editor_hint), room)), style).render(area, buf);
            Line::styled(position, style).right_aligned().render(area, buf);
        }
    }
}

/// How text and the selection show, and whether the background is light for syntax colours.
#[derive(Clone, Copy)]
struct LineLook {
    base: Style,
    selection: Style,
    light: bool,
}

/// One line of text from screen column `left`: syntax colours, the selection over them, and a
/// selected line break as one marked cell after the text.
fn draw_line(
    text: &str,
    runs: Option<&[borg_view::Run]>,
    selected: Option<std::ops::Range<usize>>,
    left: usize,
    (area, y): (Rect, u16),
    look: LineLook,
    buf: &mut Buffer,
) {
    let LineLook { base, selection, light } = look;
    let chars = text.chars().count();
    let paints: Option<Vec<Paint>> = runs
        .map(|runs| runs.iter().flat_map(|run| std::iter::repeat_n(run.paint, run.text.chars().count())).collect())
        .filter(|paints: &Vec<Paint>| paints.len() == chars);
    let selected = |at: usize| selected.as_ref().is_some_and(|range| range.contains(&at));
    let width = usize::from(area.width);
    let mut column = 0;
    for (at, shown, cells) in borg_edit::cells(text) {
        let style = if selected(at) {
            selection
        } else {
            paints
                .as_ref()
                .and_then(|paints| paints.get(at))
                .map_or(base, |paint| crate::document_view::painted(base, *paint, light))
        };
        if column >= left && column + cells <= left + width {
            buf.set_stringn(area.x + (column - left) as u16, y, &shown, cells, style);
        }
        column += cells;
    }
    if selected(chars) && column >= left && column < left + width {
        buf.set_stringn(area.x + (column - left) as u16, y, " ", 1, selection);
    }
}
