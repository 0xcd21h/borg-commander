//! The dialog that offers to install or start the background service: what the service does,
//! the exact command, what running it does, and what to do once it runs.

use crate::{
    Theme,
    dialog_view::message_lines,
    install_view::{Part, draw_offer, layout},
};
use borg_core::{ServiceOffer, ServiceStep};
use borg_i18n::tr;
use ratatui::{buffer::Buffer, layout::Rect};

fn question(step: ServiceStep) -> &'static str {
    match step {
        ServiceStep::Install => tr!(service_install_question),
        ServiceStep::Start => tr!(service_start_question),
    }
}

fn consequence(offer: &ServiceOffer) -> &'static str {
    if offer.root { tr!(service_as_root) } else { tr!(service_with_password) }
}

/// The keys at the bottom of the dialog, which a click on one presses.
pub(crate) fn keys(offer: &ServiceOffer) -> &'static str {
    match offer.step {
        ServiceStep::Install => tr!(install_keys),
        ServiceStep::Start => tr!(service_start_keys),
    }
}

fn parts(offer: &ServiceOffer, width: u16) -> Vec<Part> {
    vec![
        Part::Text(message_lines(question(offer.step), width)),
        Part::Command(offer.rows()),
        Part::Text(message_lines(consequence(offer), width)),
        Part::Note(message_lines(tr!(service_note), width)),
    ]
}

pub(crate) fn rect(offer: &ServiceOffer, screen: Rect) -> Rect {
    layout(screen, |width| parts(offer, width)).0
}

pub(crate) fn draw(offer: &ServiceOffer, theme: &Theme, screen: Rect, buf: &mut Buffer) -> Rect {
    let (rect, parts) = layout(screen, |width| parts(offer, width));
    draw_offer(tr!(service_title), keys(offer), rect, parts, theme, buf);
    rect
}
