use borg_core::KEYMAP;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Widget,
};

use crate::theme::Styles;

const CONTROL_PREFIX: char = '^';
const ALT_PREFIX: &str = "M-";
const LABEL_WIDTH: usize = 7;

pub fn draw_keybar(area: Rect, styles: &Styles<Style>, notice: Option<&str>, buf: &mut Buffer) {
    let keys = KEYMAP.iter().filter_map(|binding| {
        let label = binding.label?;
        Some([
            Span::styled(key_name(binding.code, binding.modifiers), styles.key),
            Span::styled(format!("{label:<LABEL_WIDTH$}"), styles.key_label),
            Span::raw(" "),
        ])
    });
    Line::from(keys.flatten().collect::<Vec<_>>()).render(area, buf);

    if let Some(notice) = notice {
        Line::styled(notice.to_owned(), styles.notice).right_aligned().render(area, buf);
    }
}

fn key_name(code: KeyCode, modifiers: KeyModifiers) -> String {
    let base = match code {
        KeyCode::F(n) => n.to_string(),
        KeyCode::Char(c) => c.to_ascii_uppercase().to_string(),
        other => other.to_string(),
    };
    if modifiers.contains(KeyModifiers::CONTROL) {
        format!("{CONTROL_PREFIX}{base}")
    } else if modifiers.contains(KeyModifiers::ALT) {
        format!("{ALT_PREFIX}{base}")
    } else {
        base
    }
}
