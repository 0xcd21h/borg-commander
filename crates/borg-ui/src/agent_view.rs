//! The dialogs of an agent connected through `borg mcp`: the approval of a command or a
//! change to a file, the log of what agents did, and how to connect Claude to this borg.

use borg_core::{AppState, Approval, AskKind, Change, Connect, approval_layout, approval_rows, log_layout, log_rows};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{Theme, dialog_view::centered};

const YES: &str = tr!(yes);
const NO: &str = tr!(no);
const BUTTON_GAP: &str = "   ";
const MARGIN: u16 = 4;
const CONNECT_WIDTH: u16 = 100;
/// Columns a command in the Connect dialog is indented by.
const INDENT: u16 = 4;

fn frame(title: &str, hint: Option<String>, theme: &Theme, rect: Rect, buf: &mut Buffer) {
    let styles = &theme.styles;
    Clear.render(rect, buf);
    let block = Block::bordered()
        .border_type(theme.border.into())
        .style(styles.overlay)
        .border_style(styles.overlay_border)
        .title(Line::styled(format!(" {title} "), styles.overlay_border).centered());
    let block = match hint {
        Some(hint) => block.title_bottom(Line::styled(hint, styles.overlay).centered()),
        None => block,
    };
    block.render(rect, buf);
}

fn row(rect: Rect, y: u16) -> Rect {
    Rect { x: rect.x + 2, y, width: rect.width.saturating_sub(4), height: 1 }
}

pub fn draw_approval(approval: &Approval, theme: &Theme, screen: Rect, buf: &mut Buffer) -> Rect {
    let styles = &theme.styles;
    let (rect, body) = approval_layout(approval, screen);
    let rows = approval_rows(&approval.ask.body, body.width);
    let shown = usize::from(body.height);
    let hint = if rows.len() > shown {
        let from = approval.scroll + 1;
        let to = (approval.scroll + shown).min(rows.len());
        tr!(agent_approval_scroll)
            .replace("{from}", &from.to_string())
            .replace("{to}", &to.to_string())
            .replace("{total}", &rows.len().to_string())
    } else {
        tr!(agent_approval_full).to_owned()
    };
    frame(tr!(agent_title).trim(), Some(hint), theme, rect, buf);

    let heading = match approval.ask.kind {
        AskKind::Run => tr!(agent_wants_run),
        AskKind::Create => tr!(agent_wants_create),
        AskKind::Edit => tr!(agent_wants_edit),
    };
    Line::styled(heading, styles.overlay).centered().render(row(rect, rect.y + 1), buf);

    buf.set_style(body, styles.input);
    for (offset, (change, text)) in (0u16..).zip(rows.iter().skip(approval.scroll).take(shown)) {
        let style = match change {
            Change::Same => styles.input,
            Change::Added => styles.input.fg(Color::Green),
            Change::Removed => styles.input.fg(Color::Red),
            Change::Gap => styles.input.fg(styles.meta.fg.unwrap_or(Color::DarkGray)),
        };
        Line::styled(text.clone(), style).render(Rect { y: body.y + offset, height: 1, ..body }, buf);
    }

    // Where it happens; a long path loses its start, so the file's name always shows.
    let (ask, place) = (&approval.ask, &approval.ask.place);
    let place = match ask.kind {
        AskKind::Run => tr!(agent_in_place).replace("{place}", place),
        _ if place.is_empty() => ask.summary.clone(),
        _ => tr!(agent_file_place).replace("{path}", &ask.summary).replace("{place}", place),
    };
    let place = crate::format::truncate_start(&place, usize::from(body.width));
    Line::styled(place, styles.overlay).centered().render(row(rect, rect.bottom().saturating_sub(4)), buf);
    let (yes, no) = if approval.accept {
        (styles.overlay_selected, styles.overlay)
    } else {
        (styles.overlay, styles.overlay_selected)
    };
    Line::from(vec![Span::styled(YES, yes), Span::styled(BUTTON_GAP, styles.overlay), Span::styled(NO, no)])
        .centered()
        .render(row(rect, rect.bottom().saturating_sub(2)), buf);
    rect
}

/// Which button of the approval dialog is at `point`: Yes, No, or neither.
pub(crate) fn approval_button_at(approval: &Approval, screen: Rect, point: Position) -> Option<bool> {
    let (rect, _) = approval_layout(approval, screen);
    if point.y != rect.bottom().saturating_sub(2) {
        return None;
    }
    let width = (YES.width() + BUTTON_GAP.width() + NO.width()) as u16;
    let start = rect.x + 2 + rect.width.saturating_sub(4).saturating_sub(width) / 2;
    let no_start = start + (YES.width() + BUTTON_GAP.width()) as u16;
    if (start..start + YES.width() as u16).contains(&point.x) {
        Some(true)
    } else if (no_start..no_start + NO.width() as u16).contains(&point.x) {
        Some(false)
    } else {
        None
    }
}

pub fn draw_log(state: &AppState, scroll: Option<usize>, theme: &Theme, screen: Rect, buf: &mut Buffer) -> Rect {
    let (rect, body) = log_layout(screen);
    frame(tr!(agent_log_title), Some(tr!(agent_log_hint).to_owned()), theme, rect, buf);
    let rows = log_rows(&state.agent.log, body.width);
    let last = rows.len().saturating_sub(usize::from(body.height));
    let start = scroll.unwrap_or(last).min(last);
    for (offset, text) in (0u16..).zip(rows.iter().skip(start).take(usize::from(body.height))) {
        Line::styled(text.clone(), theme.styles.overlay).render(Rect { y: body.y + offset, height: 1, ..body }, buf);
    }
    rect
}

/// One block of the Connect dialog: a label and the command it copies, wrapped.
fn command_rows(command: &str, width: u16) -> Vec<String> {
    let width = usize::from(width.saturating_sub(INDENT)).max(1);
    let chars: Vec<char> = command.chars().collect();
    chars.chunks(width).map(|piece| piece.iter().collect()).collect()
}

pub fn draw_connect(
    state: &AppState,
    connect: &Connect,
    theme: &Theme,
    screen: Rect,
    buf: &mut Buffer,
) -> (Rect, Option<Position>) {
    let styles = &theme.styles;
    let info = &state.agent.connect;
    let remote = info.ssh.is_some();
    let width = screen.width.saturating_sub(MARGIN).min(CONNECT_WIDTH);
    let inner_width = width.saturating_sub(4);
    let [start, keep] = borg_core::connect_commands(info, connect.target.text().trim());
    let (start, keep) = (command_rows(&start, inner_width), command_rows(&keep, inner_width));
    // Intro, a gap, each label and command with a gap after, and the target field with its note.
    let mut height = 2 + 2 + (1 + start.len() as u16 + 1) + (1 + keep.len() as u16) + 2;
    if remote {
        height += 3;
    }
    let rect = centered(screen, width, height.min(screen.height));
    frame(tr!(agent_connect_title), Some(tr!(agent_connect_hint).to_owned()), theme, rect, buf);

    let mut y = rect.y + 1;
    put(buf, rect, &mut y, tr!(agent_connect_intro), styles.overlay, 0);
    y += 1;
    for (keep_block, label, rows) in [(false, tr!(agent_connect_start), &start), (true, tr!(agent_connect_keep), &keep)]
    {
        let picked = connect.keep == keep_block;
        let marker = if picked { "▸ " } else { "  " };
        let style = if picked { styles.overlay_selected } else { styles.overlay };
        put(buf, rect, &mut y, &format!("{marker}{label}"), style, 0);
        for text in rows {
            put(buf, rect, &mut y, text, styles.input, INDENT);
        }
        y += 1;
    }
    let mut cursor = None;
    if remote {
        let label = format!("{}: ", tr!(agent_connect_target));
        let area = row(rect, y);
        Line::styled(label.clone(), styles.overlay).render(area, buf);
        let field =
            Rect { x: area.x + label.width() as u16, width: area.width.saturating_sub(label.width() as u16), ..area };
        buf.set_style(field, styles.input);
        Line::styled(connect.target.text().to_owned(), styles.input).render(field, buf);
        let before: String = connect.target.text().chars().take(connect.target.cursor_chars()).collect();
        cursor = Some(Position::new((field.x + before.width() as u16).min(field.right().saturating_sub(1)), field.y));
        y += 1;
        let note = styles.overlay.fg(styles.meta.fg.unwrap_or(Color::DarkGray));
        put(buf, rect, &mut y, tr!(agent_connect_ssh), note, 0);
    }
    (rect, cursor)
}

/// Writes a line of the Connect dialog at `y`, `indent` columns in, and moves `y` down.
fn put(buf: &mut Buffer, rect: Rect, y: &mut u16, text: &str, style: Style, indent: u16) {
    if *y + 1 < rect.bottom() {
        let area = row(rect, *y);
        let area = Rect { x: area.x + indent, width: area.width.saturating_sub(indent), ..area };
        Line::styled(text.to_owned(), style).render(area, buf);
    }
    *y += 1;
}
