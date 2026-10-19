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
const ALIAS_HINT: &str = " Esc then 1–9/0 = F1–F10 ";
const HINT_FROM: u16 = 150;

pub(crate) fn actions_area(area: Rect) -> Rect {
    let hint = if area.width >= HINT_FROM { ALIAS_HINT.width() as u16 } else { 0 };
    Rect { width: area.width.saturating_sub(hint), ..area }
}

pub fn draw_keybar(area: Rect, styles: &Styles<Style>, notice: Option<&str>, buf: &mut Buffer) {
    let items: Vec<(String, &str)> =
        KEYMAP.iter().filter_map(|binding| Some((key_name(binding.code, binding.modifiers), binding.label?))).collect();
    let actions = actions_area(area);
    let count = u16::try_from(items.len().max(1)).unwrap_or(u16::MAX);
    let slot = usize::from(actions.width / count);

    let spans: Vec<Span<'_>> = items
        .iter()
        .flat_map(|(key, label)| {
            let room = slot.saturating_sub(key.width());
            [
                Span::styled(format::truncate_end(key, slot), styles.key),
                Span::styled(format!("{:<room$}", format::truncate_end(label, room)), styles.key_label),
            ]
        })
        .collect();
    Line::from(spans).render(actions, buf);
    if actions.width < area.width {
        Line::styled(ALIAS_HINT, styles.key_label).right_aligned().render(area, buf);
    }

    if let Some(notice) = notice {
        let width = usize::from(area.width);
        Line::styled(format::truncate_end(notice, width), styles.notice).right_aligned().render(area, buf);
    }
}

pub fn key_name(code: KeyCode, modifiers: KeyModifiers) -> String {
    let base = match code {
        KeyCode::F(n) => format!("F{n}"),
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
