//! The dialogs of an agent connected through `borg mcp`: the approval of a command or a
//! change to a file, the log of what agents did, and how to connect Claude to this borg.

use borg_core::{
    AppState, Approval, AskKind, Change, Connect, LineInput, approval_layout, approval_rows, log_layout, log_rows,
};
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{Styles, Theme, dialog_view::centered};

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
        AskKind::Pair => tr!(agent_wants_pair),
        AskKind::Read => tr!(agent_wants_read),
        AskKind::Upload => tr!(agent_wants_upload),
        AskKind::ApprovalMode => tr!(agent_wants_approve_all),
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
        AskKind::Pair | AskKind::Read | AskKind::Upload | AskKind::ApprovalMode => place.clone(),
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
    Line::from(vec![Span::styled(tr!(yes), yes), Span::styled(BUTTON_GAP, styles.overlay), Span::styled(tr!(no), no)])
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
    let width = (tr!(yes).width() + BUTTON_GAP.width() + tr!(no).width()) as u16;
    let start = rect.x + 2 + rect.width.saturating_sub(4).saturating_sub(width) / 2;
    let no_start = start + (tr!(yes).width() + BUTTON_GAP.width()) as u16;
    if (start..start + tr!(yes).width() as u16).contains(&point.x) {
        Some(true)
    } else if (no_start..no_start + tr!(no).width() as u16).contains(&point.x) {
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
    let note = styles.overlay.fg(styles.meta.fg.unwrap_or(Color::DarkGray));
    let info = &state.agent.connect;
    let remote = info.ssh.is_some();
    let width = screen.width.saturating_sub(MARGIN).min(CONNECT_WIDTH);
    let inner_width = width.saturating_sub(4);
    let [start, keep] = borg_core::connect_commands(info, connect.target.text().trim());
    // Through borg cloud: the command that pairs, or why there is none yet.
    let (cloud, cloud_style, cloud_note) = match &state.agent.pairing {
        Some(Ok(link)) => (borg_core::pair_command(link), styles.input, tr!(agent_connect_cloud_note)),
        Some(Err(reason)) => (reason.clone(), note, ""),
        None => (tr!(agent_connect_cloud_wait).to_owned(), note, ""),
    };
    let blocks = [
        (
            borg_core::CONNECT_CLOUD,
            tr!(agent_connect_cloud),
            command_rows(&cloud, inner_width),
            cloud_style,
            cloud_note,
        ),
        (borg_core::CONNECT_START, tr!(agent_connect_start), command_rows(&start, inner_width), styles.input, ""),
        (borg_core::CONNECT_KEEP, tr!(agent_connect_keep), command_rows(&keep, inner_width), styles.input, ""),
    ];
    // A relay typed in, which Enter saves.
    let relay_changed = connect.relay.text().trim() != info.relay;
    // The intro and a gap, each block with a gap after, the relay field with its note, the
    // SSH target with its note, and the paired computers.
    let mut height: u16 = 2 + 2;
    for (_, _, rows, _, extra) in &blocks {
        height += 1 + rows.len() as u16 + 1 + u16::from(!extra.is_empty());
    }
    height += 1 + u16::from(relay_changed);
    height += if remote { 2 } else { 0 } + u16::from(!state.agent.computers.is_empty());
    let rect = centered(screen, width, height.min(screen.height));
    frame(tr!(agent_connect_title), Some(tr!(agent_connect_hint).to_owned()), theme, rect, buf);

    let mut y = rect.y + 1;
    let mut cursor = None;
    put(buf, rect, &mut y, tr!(agent_connect_intro), styles.overlay, 0);
    y += 1;
    for (pick, label, rows, style, extra) in &blocks {
        let picked = connect.pick == *pick;
        let marker = if picked { "▸ " } else { "  " };
        let label_style = if picked { styles.overlay_selected } else { styles.overlay };
        put(buf, rect, &mut y, &format!("{marker}{label}"), label_style, 0);
        for text in rows {
            put(buf, rect, &mut y, text, *style, INDENT);
        }
        if !extra.is_empty() {
            put(buf, rect, &mut y, extra, note, INDENT);
        }
        let mut field_cursor = None;
        if *pick == borg_core::CONNECT_CLOUD {
            field_cursor = Some(field(buf, rect, &mut y, tr!(agent_connect_relay), &connect.relay, styles, INDENT));
            if relay_changed {
                put(buf, rect, &mut y, tr!(agent_connect_relay_note), note, INDENT);
            }
        }
        if picked {
            cursor = field_cursor;
        }
        y += 1;
    }
    if remote {
        let target = field(buf, rect, &mut y, tr!(agent_connect_target), &connect.target, styles, 0);
        if connect.pick != borg_core::CONNECT_CLOUD {
            cursor = Some(target);
        }
        put(buf, rect, &mut y, tr!(agent_connect_ssh), note, 0);
    }
    if !state.agent.computers.is_empty() {
        let names = state.agent.computers.join(", ");
        put(buf, rect, &mut y, &tr!(agent_connect_paired).replace("{names}", &names), note, 0);
    }
    (rect, cursor)
}

/// Draws a labelled text field of the Connect dialog at `y`, `indent` columns in, moves `y`
/// down, and returns where the field's cursor goes.
fn field(
    buf: &mut Buffer,
    rect: Rect,
    y: &mut u16,
    label: &str,
    input: &LineInput,
    styles: &Styles<Style>,
    indent: u16,
) -> Position {
    let label = format!("{label}: ");
    let area = row(rect, *y);
    let area = Rect { x: area.x + indent, width: area.width.saturating_sub(indent), ..area };
    Line::styled(label.clone(), styles.overlay).render(area, buf);
    let width = label.width() as u16;
    let field = Rect { x: area.x + width, width: area.width.saturating_sub(width), ..area };
    buf.set_style(field, styles.input);
    Line::styled(input.text().to_owned(), styles.input).render(field, buf);
    let before: String = input.text().chars().take(input.cursor_chars()).collect();
    *y += 1;
    Position::new((field.x + before.width() as u16).min(field.right().saturating_sub(1)), field.y)
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
