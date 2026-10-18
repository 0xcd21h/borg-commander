use borg_core::KEYMAP;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Widget,
};
use unicode_width::UnicodeWidthStr;

use crate::{format, theme::Styles};

const CONTROL_PREFIX: char = '^';
const ALT_PREFIX: &str = "M-";

pub fn draw_keybar(area: Rect, styles: &Styles<Style>, notice: Option<&str>, buf: &mut Buffer) {
    let items: Vec<(String, &str)> =
        KEYMAP.iter().filter_map(|binding| Some((key_name(binding.code, binding.modifiers), binding.label?))).collect();
    let count = u16::try_from(items.len().max(1)).unwrap_or(u16::MAX);
    let slot = usize::from(area.width / count);

    let spans: Vec<Span<'_>> = items
        .iter()
        .flat_map(|(key, label)| {
            let room = slot.saturating_sub(key.width());
            [
                Span::styled(key.clone(), styles.key),
                Span::styled(format!("{:<room$}", format::truncate_end(label, room)), styles.key_label),
            ]
        })
        .collect();
    Line::from(spans).render(area, buf);

    if let Some(notice) = notice {
        let width = usize::from(area.width);
        Line::styled(format::truncate_end(notice, width), styles.notice).right_aligned().render(area, buf);
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
