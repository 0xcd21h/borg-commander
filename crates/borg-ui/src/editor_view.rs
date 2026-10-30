use borg_core::{Editor, editor_status};
use borg_i18n::tr;
use borg_view::Paint;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Style},
    text::Line,
    widgets::Widget,
};

use crate::{Palette, Theme, format};

/// Draws the editor over `area`, returning where the terminal cursor belongs.
pub(crate) fn draw_editor(editor: &Editor, theme: &Theme, area: Rect, buf: &mut Buffer) -> Option<Position> {
    let [title, body, status] =
        Layout::vertical([Constraint::Length(1), Constraint::Fill(1), Constraint::Length(1)]).areas(area);
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
    let prompt = draw_status(editor, styles.viewer_status, crate::branding::content(status), buf);
    prompt.or(cursor.filter(|cursor| text.contains(*cursor)))
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

/// The prompt, note or keys, and the cursor position; returns where a prompt's cursor is.
fn draw_status(editor: &Editor, style: Style, area: Rect, buf: &mut Buffer) -> Option<Position> {
    let (line, column) = editor.buffer.position();
    let position = format!("{} ", tr!(editor_position, line = line + 1, column = column + 1));
    match editor_status(editor) {
        Some((label, Some(input))) => {
            let shown = format!(" {label} {}", input.text());
            Line::styled(shown, style).render(area, buf);
            let before = unicode_width::UnicodeWidthStr::width(format!(" {label} ").as_str());
            let typed: String = input.text().chars().take(input.cursor_chars()).collect();
            let x = before + unicode_width::UnicodeWidthStr::width(typed.as_str());
            return Some(Position { x: area.x.saturating_add(x as u16), y: area.y }).filter(|p| area.contains(*p));
        }
        Some((text, None)) => Line::styled(format!(" {text}"), style).render(area, buf),
        None => {
            let room = usize::from(area.width).saturating_sub(position.chars().count() + 2);
            Line::styled(format!(" {}", format::truncate_end(tr!(editor_hint), room)), style).render(area, buf);
            Line::styled(position, style).right_aligned().render(area, buf);
        }
    }
    None
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
