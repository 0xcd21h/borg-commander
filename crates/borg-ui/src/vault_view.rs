use crate::{
    Theme,
    dialog_view::message_lines,
    install_view::{COMMAND_INSET, Part, draw_offer, height, last_command_start, layout},
};
use borg_core::VaultKey;
use borg_i18n::tr;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
};

const BORDER_SIDE: u16 = 1;

fn parts(key: &VaultKey, width: u16, rows: u16) -> Vec<Part> {
    let mut parts = vec![
        Part::Text(message_lines(&tr!(vault_key_message, name = key.name.as_str()), width)),
        Part::Command(key.lines(usize::from(width.saturating_sub(COMMAND_INSET)))),
        Part::Command(vec![key.typed.text().to_owned()]),
    ];
    if height(&parts) > rows {
        parts.remove(0);
    }
    parts
}

pub(crate) fn rect(key: &VaultKey, screen: Rect) -> Rect {
    layout(screen, |width| parts(key, width, screen.height)).0
}

pub(crate) fn draw(key: &VaultKey, theme: &Theme, screen: Rect, buf: &mut Buffer) -> (Rect, Position) {
    let (rect, parts) = layout(screen, |width| parts(key, width, screen.height));
    draw_offer(tr!(vault_key_title), tr!(vault_key_keys), rect, parts, theme, buf);
    let start = last_command_start(rect);
    let typed = u16::try_from(key.typed.cursor_chars()).unwrap_or(u16::MAX);
    let x = start.x.saturating_add(typed).min(rect.right().saturating_sub(BORDER_SIDE + 1));
    (rect, Position::new(x, start.y))
}
