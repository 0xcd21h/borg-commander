use crate::{
    Theme,
    dialog_view::message_lines,
    install_view::{Part, draw_offer, layout},
};
use borg_core::VaultKey;
use borg_i18n::tr;
use ratatui::{buffer::Buffer, layout::Rect};

fn parts(key: &VaultKey, width: u16) -> Vec<Part> {
    vec![
        Part::Text(message_lines(&tr!(vault_key_message, name = key.name.as_str()), width)),
        Part::Command(key.lines()),
    ]
}

pub(crate) fn rect(key: &VaultKey, screen: Rect) -> Rect {
    layout(screen, |width| parts(key, width)).0
}

pub(crate) fn draw(key: &VaultKey, theme: &Theme, screen: Rect, buf: &mut Buffer) -> Rect {
    let (rect, parts) = layout(screen, |width| parts(key, width));
    draw_offer(tr!(vault_key_title), tr!(vault_key_keys), rect, parts, theme, buf);
    rect
}
