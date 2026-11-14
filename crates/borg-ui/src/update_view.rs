//! The dialog that offers a newer Borg: the version, the command that installs it and what
//! happens to open sessions.

use crate::{
    Theme,
    dialog_view::message_lines,
    install_view::{Part, draw_offer, layout},
};
use borg_core::UpdateOffer;
use borg_i18n::tr;
use ratatui::{buffer::Buffer, layout::Rect};

fn question(offer: &UpdateOffer) -> String {
    tr!(update_question, version = offer.version, current = offer.current)
}

fn consequence(offer: &UpdateOffer) -> &'static str {
    if offer.manager.is_some() { tr!(update_by_manager) } else { tr!(update_moves_sessions) }
}

fn parts(offer: &UpdateOffer, width: u16) -> Vec<Part> {
    vec![
        Part::Text(message_lines(&question(offer), width)),
        Part::Command(vec![offer.command()]),
        Part::Text(message_lines(consequence(offer), width)),
    ]
}

pub(crate) fn rect(offer: &UpdateOffer, screen: Rect) -> Rect {
    layout(screen, |width| parts(offer, width)).0
}

pub(crate) fn draw(offer: &UpdateOffer, theme: &Theme, screen: Rect, buf: &mut Buffer) -> Rect {
    let (rect, parts) = layout(screen, |width| parts(offer, width));
    draw_offer(tr!(update_title), tr!(update_keys), rect, parts, theme, buf);
    rect
}
