use borg_core::{AppState, Editor, EditorDialog, EditorSlot, Msg, Pointer, editor_dialog, editor_status};
use borg_i18n::tr;
use borg_view::Paint;
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Style},
    text::{Line, Span},
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

/// Where the editor's title, text and status go: rows across the screen, short of the badge in
/// its corner, or a panel's frame, whose edges carry the title and status.
fn parts(editor: &Editor, area: Rect) -> [Rect; 3] {
    if editor.slot == EditorSlot::Screen {
        let [title, body, status] = rows(area);
        return [title, body, crate::branding::content(status)];
    }
    let body = Block::bordered().inner(area);
    let edge = |y| Rect { y, height: area.height.min(1), ..body };
    [edge(area.y), body, edge(area.bottom().saturating_sub(1))]
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
/// cursor is there. A panel too narrow for its buttons and keys lends it room from the `screen`.
fn dialog_layout(editor: &Editor, dialog: &EditorDialog<'_>, area: Rect, screen: Rect) -> DialogLayout {
    let [_, body, _] = parts(editor, area);
    let fields = dialog.fields.len() as u16;
    let height = 5 + u16::from(dialog.message.is_some()) + fields;
    let widths: Vec<u16> = dialog.buttons.iter().map(|label| label.width() as u16 + 4).collect();
    let total = widths.iter().sum::<u16>() + BUTTON_GAP * widths.len().saturating_sub(1) as u16;
    let width = area.width.saturating_sub(4).min(DIALOG_WIDTH);
    let low = body.bottom().saturating_sub(height + 1).max(body.y);
    let cursor = body.y as usize + editor.buffer.position().0.saturating_sub(editor.top);
    let y = if (low as usize..(low + height) as usize).contains(&cursor) { body.y + 1 } else { low };
    let mut rect = Rect { y, height: height.min(body.height), ..centered(area, width, height) };
    let needed = (total + 4).max(dialog.keys.width() as u16 + 2);
    if needed > width && editor.slot != EditorSlot::Screen {
        rect.width = needed.min(screen.width);
        let middle = area.x + area.width / 2;
        rect.x = middle.saturating_sub(rect.width / 2).clamp(screen.x, screen.right().saturating_sub(rect.width));
    }
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
pub(crate) fn draw_dialog(editor: &Editor, theme: &Theme, area: Rect, buf: &mut Buffer) -> Option<Option<Position>> {
    let dialog = editor_dialog(editor)?;
    let layout = dialog_layout(editor, &dialog, area, buf.area);
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

/// What a mouse event in the editor on `area` does: in its dialog, a field or button; in the
/// text, a click, drag or double click; the wheel scrolls; and the keys named in the status line
/// or the dialog's border take clicks.
pub(crate) fn mouse(
    editor: &Editor,
    event: MouseEvent,
    double: bool,
    (area, screen): (Rect, Rect),
    wheel: isize,
) -> Option<Msg> {
    let point = Position::new(event.column, event.row);
    let left = |kind| matches!(kind, MouseEventKind::Down(MouseButton::Left));
    let slot = editor.slot;
    match event.kind {
        MouseEventKind::ScrollUp => return Some(Msg::Pointer(Pointer::EditorScroll(slot, -wheel))),
        MouseEventKind::ScrollDown => return Some(Msg::Pointer(Pointer::EditorScroll(slot, wheel))),
        _ => {}
    }
    if let Some(dialog) = editor_dialog(editor) {
        if !left(event.kind) {
            return None;
        }
        let layout = dialog_layout(editor, &dialog, area, screen);
        let fields = layout.fields.iter().chain(&layout.buttons);
        if let Some(control) = fields.into_iter().position(|rect| rect.contains(point)) {
            return Some(Msg::Pointer(Pointer::EditorControl(slot, control)));
        }
        return crate::hints::bottom(layout.rect, dialog.keys, true, point).map(Msg::Key);
    }
    let [_, body, status] = parts(editor, area);
    let drag = matches!(event.kind, MouseEventKind::Drag(MouseButton::Left));
    if (left(event.kind) || drag) && body.contains(point) {
        let text_x = body.x + editor.gutter().min(body.width);
        return Some(Msg::Pointer(Pointer::EditorClick {
            slot,
            line: editor.top + usize::from(point.y - body.y),
            column: editor.left + usize::from(point.x.saturating_sub(text_x)),
            select: drag || event.modifiers.contains(KeyModifiers::SHIFT),
            word: double && !drag,
        }));
    }
    if editor.find.open {
        return left(event.kind).then(|| crate::find_view::click(&editor.find, status, point)).flatten().map(Msg::Key);
    }
    // A panel's editor names its keys on the key bar instead.
    (left(event.kind) && slot == EditorSlot::Screen && status.contains(point) && editor_status(editor).is_none())
        .then(|| crate::hints::key_at(tr!(editor_hint), status.x + 1, point.x))
        .flatten()
        .map(Msg::Key)
}

/// Draws the editor over `area`, returning where the terminal cursor belongs.
pub(crate) fn draw_editor(editor: &Editor, theme: &Theme, area: Rect, buf: &mut Buffer) -> Option<Position> {
    let [title, _, status] = rows(area);
    let styles = &theme.styles;
    buf.set_style(title, styles.viewer_status);
    buf.set_style(status, styles.viewer_status);
    draw_title(editor, styles.viewer_status, title, buf);
    let looks = (styles.viewer_status, styles.viewer_status);
    draw_inside(editor, theme, area, looks, Some(tr!(editor_hint)), buf)
}

/// Draws the editor a panel holds in the panel's frame, with its file's name on the top edge
/// and where the cursor stands on the bottom one, returning where the terminal cursor belongs.
pub(crate) fn draw_docked(
    editor: &Editor,
    theme: &Theme,
    active: bool,
    area: Rect,
    buf: &mut Buffer,
) -> Option<Position> {
    let styles = &theme.styles;
    let (border, title) =
        if active { (styles.border_active, styles.title_active) } else { (styles.border, styles.title) };
    Block::bordered().border_type(theme.border.into()).style(styles.panel).border_style(border).render(area, buf);
    let [top, _, _] = parts(editor, area);
    let modified = if editor.buffer.modified() { format!(" · {}", tr!(editor_modified)) } else { String::new() };
    let facts = format!(" {} ", facts(editor));
    let room = usize::from(top.width).saturating_sub(facts.width() + modified.width() + 3);
    let path = format::truncate_start(&editor.path.display().to_string(), room);
    Line::from(Span::styled(format!(" {path}{modified} "), title)).render(top, buf);
    Line::from(Span::styled(facts, border)).right_aligned().render(top, buf);
    draw_inside(editor, theme, area, (title, border), None, buf)
}

/// Draws the editor's text, its status or find bar and its dialog: the status's note in the
/// first of `looks` and where the cursor stands in the second, after the `hint` if any.
fn draw_inside(
    editor: &Editor,
    theme: &Theme,
    area: Rect,
    looks: (Style, Style),
    hint: Option<&str>,
    buf: &mut Buffer,
) -> Option<Position> {
    let [_, body, status] = parts(editor, area);
    let styles = &theme.styles;
    let base = styles.viewer;
    buf.set_style(body, base);

    let background = Palette::default().background_of(base.bg.unwrap_or(Color::Reset));
    let light = u32::from(background.r) + u32::from(background.g) + u32::from(background.b) > 3 * 128;
    let gutter = editor.gutter().min(body.width);
    let numbers = base.fg(styles.meta.fg.unwrap_or(Color::Reset));
    let text = Rect { x: body.x + gutter, width: body.width - gutter, ..body };
    let buffer = &editor.buffer;
    let selection = buffer.selection();
    let (found, current) = crate::find_view::match_styles(theme);
    let look = LineLook { base, selection: styles.cursor, found, current, light };
    for (y, line) in (body.top()..body.bottom()).zip(editor.top..buffer.lines()) {
        let number = format!("{:>width$} ", line + 1, width = usize::from(gutter.saturating_sub(2)));
        buf.set_stringn(body.x, y, number, usize::from(gutter), numbers);
        let start = buffer.line_start(line);
        let selected =
            selection.clone().map(|range| range.start.saturating_sub(start)..range.end.saturating_sub(start));
        let window = buffer.window(line, editor.left, usize::from(text.width));
        let found = editor
            .matches_on(&window.text)
            .into_iter()
            .map(|found| window.start + found.start..window.start + found.end);
        let marks = Marks { selected, found: found.collect(), length: buffer.line_len(line) };
        draw_line(&window, editor.runs(line), &marks, editor.left, (text, y), look, buf);
    }

    let (line, _) = buffer.position();
    let column = buffer.screen_column();
    let cursor = (line >= editor.top && column >= editor.left).then(|| Position {
        x: text.x.saturating_add((column - editor.left) as u16),
        y: body.y.saturating_add((line - editor.top) as u16),
    });
    let bar = (editor.find.open && editor.prompt.is_none())
        .then(|| crate::find_view::draw_bar(&editor.find, theme, status, buf))
        .filter(|cursor| status.contains(*cursor));
    if bar.is_none() {
        draw_status(editor, looks, hint, status, buf);
    }
    match draw_dialog(editor, theme, area, buf) {
        Some(field) => field,
        None => bar.or(cursor.filter(|cursor| text.contains(*cursor))),
    }
}

/// The file's line ends, indentation and language.
fn facts(editor: &Editor) -> String {
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
    format!("{eol} · {indent}{language}")
}

fn draw_title(editor: &Editor, style: Style, area: Rect, buf: &mut Buffer) {
    let facts = format!("{} ", facts(editor));
    let modified = if editor.buffer.modified() { format!(" · {}", tr!(editor_modified)) } else { String::new() };
    let room = usize::from(area.width).saturating_sub(facts.chars().count() + modified.chars().count() + 2);
    let path = format::truncate_start(&editor.path.display().to_string(), room);
    Line::styled(format!(" {path}{modified}"), style).render(area, buf);
    Line::styled(facts, style).right_aligned().render(area, buf);
}

/// The note, or the keys of the `hint` and where the cursor stands.
fn draw_status(editor: &Editor, (note_look, look): (Style, Style), hint: Option<&str>, area: Rect, buf: &mut Buffer) {
    let (line, column) = editor.buffer.position();
    let position = format!("{} ", tr!(editor_position, line = line + 1, column = column + 1));
    if let Some(note) = editor_status(editor) {
        Line::from(Span::styled(format!(" {note} "), note_look)).render(area, buf);
        return;
    }
    if let Some(hint) = hint {
        let room = usize::from(area.width).saturating_sub(position.chars().count() + 2);
        Line::from(Span::styled(format!(" {}", format::truncate_end(hint, room)), look)).render(area, buf);
    }
    Line::from(Span::styled(format!(" {position}"), look)).right_aligned().render(area, buf);
}

/// Where each of the keys of a panel's editor shows on the key bar: the function keys spread
/// across the first row as the panels' are, and the others along the second.
fn keys(area: Rect) -> Vec<(Rect, &'static str)> {
    let (primary, context) = crate::keybar::rows(area);
    let functions: Vec<&'static str> = tr!(dock_function_keys).split(" · ").collect();
    let slot = primary.width / u16::try_from(functions.len().max(1)).unwrap_or(u16::MAX);
    let spread = functions
        .into_iter()
        .zip(0u16..)
        .map(|(part, at)| (Rect { x: primary.x + slot * at, width: slot, ..primary }, part));
    let mut x = context.x;
    let others = tr!(dock_keys).split(" · ").filter_map(|part| {
        let width = part.width() as u16 + 2;
        let rect = Rect { x, width, ..context };
        x += width;
        (rect.right() <= context.right()).then_some((rect, part))
    });
    spread.chain(others).filter(|(rect, _)| !rect.is_empty()).collect()
}

/// The key bar while a panel's editor has focus: its keys in place of the panels', and a notice
/// over the second row.
pub(crate) fn keybar(state: &AppState, area: Rect, theme: &Theme, buf: &mut Buffer) {
    let styles = &theme.styles;
    buf.set_style(area, styles.key_label);
    for (rect, part) in keys(area) {
        let (key, label) = part.split_once(' ').unwrap_or((part, ""));
        // The function keys run into their labels, as the panels' do.
        let gap = if rect.y == area.y { "" } else { " " };
        Line::from(vec![Span::styled(key, styles.key), Span::styled(format!("{gap}{label}"), styles.key_label)])
            .render(rect, buf);
    }
    let (_, context) = crate::keybar::rows(area);
    if let Some(notice) = state.notice.as_deref().filter(|_| !context.is_empty()) {
        Clear.render(context, buf);
        buf.set_style(context, styles.notice);
        Line::styled(format::truncate_end(notice, usize::from(context.width)), styles.notice).render(context, buf);
    }
}

/// What a click on the key bar of a panel's editor does: it presses the key it names, or puts
/// the notice away.
pub(crate) fn keybar_click(state: &AppState, area: Rect, point: Position) -> Option<Msg> {
    let (_, context) = crate::keybar::rows(area);
    if state.notice.is_some() && context.contains(point) {
        return Some(Msg::Pointer(Pointer::DismissNotice));
    }
    let (_, part) = keys(area).into_iter().find(|(rect, _)| rect.contains(point))?;
    crate::hints::key(part.split_whitespace().next()?).map(Msg::Key)
}

/// How text, the selection and matches show, and whether the background is light for syntax
/// colours.
#[derive(Clone, Copy)]
struct LineLook {
    base: Style,
    selection: Style,
    found: Style,
    /// A match that is selected: the one stepped to.
    current: Style,
    light: bool,
}

/// What of a line is selected and which parts match the find bar's query, by character, and
/// how many characters it has.
struct Marks {
    selected: Option<std::ops::Range<usize>>,
    found: Vec<std::ops::Range<usize>>,
    length: usize,
}

/// One line of text from screen column `left`, of which `window` holds what shows: syntax
/// colours, matches and the selection over them, and a selected line break as one marked cell
/// after the text.
fn draw_line(
    window: &borg_edit::Window,
    runs: Option<&[borg_view::Run]>,
    marks: &Marks,
    left: usize,
    (area, y): (Rect, u16),
    look: LineLook,
    buf: &mut Buffer,
) {
    let LineLook { base, selection, found, current, light } = look;
    let paints: Option<Vec<Paint>> = runs
        .map(|runs| runs.iter().flat_map(|run| std::iter::repeat_n(run.paint, run.text.chars().count())).collect())
        .filter(|paints: &Vec<Paint>| paints.len() == marks.length);
    let selected = |at: usize| marks.selected.as_ref().is_some_and(|range| range.contains(&at));
    let width = usize::from(area.width);
    let mut column = window.column;
    let mut next = 0;
    for (at, shown, cells) in borg_edit::cells(&window.text, window.column) {
        let at = window.start + at;
        while marks.found.get(next).is_some_and(|range| range.end <= at) {
            next += 1;
        }
        let matched = marks.found.get(next).is_some_and(|range| range.contains(&at));
        let style = if selected(at) {
            if matched { current } else { selection }
        } else if matched {
            found
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
    let to_end = window.start + window.text.chars().count() == marks.length;
    if to_end && selected(marks.length) && column >= left && column < left + width {
        buf.set_stringn(area.x + (column - left) as u16, y, " ", 1, selection);
    }
}
