//! The scan form: the network S scans, and what scanning it sends where.

use crate::{
    Theme,
    field_dialog::{self, FieldDialog},
};
use borg_core::{Network, ScanForm, ScanProblem, scanned_ports};
use borg_i18n::tr;
use ratatui::{buffer::Buffer, layout::Rect};

fn problem(problem: ScanProblem) -> &'static str {
    match problem {
        ScanProblem::Unreadable => tr!(scan_form_unreadable),
        ScanProblem::TooWide => tr!(scan_form_too_wide),
        ScanProblem::NoRoute => tr!(scan_form_no_route),
    }
}

/// What the form says under the network: why Enter refused it, else what the scan would send.
fn note(network: &Network, form: &ScanForm) -> (String, bool) {
    if let Some(refused) = form.problem {
        return (problem(refused).to_owned(), true);
    }
    match network.typed_scan(form.network.text()) {
        Ok(scan) if scan.on_link => (
            tr!(scan_form_on_link, count = scan.addresses.len(), interface = scan.interface, ports = scanned_ports()),
            false,
        ),
        Ok(scan) => (
            tr!(scan_form_routed, count = scan.addresses.len(), interface = scan.interface, ports = scanned_ports()),
            false,
        ),
        Err(waiting) => (problem(waiting).to_owned(), false),
    }
}

pub(crate) fn draw(network: &Network, form: &ScanForm, theme: &Theme, screen: Rect, buf: &mut Buffer) -> Rect {
    let dialog = FieldDialog {
        title: tr!(scan_form_title),
        keys: tr!(scan_form_keys),
        fields: vec![(tr!(scan_form_network_label), true)],
        notes: vec![note(network, form)],
    };
    field_dialog::draw(&dialog, |_, room| field_dialog::input(&form.network, true, room), theme, screen, buf)
}
