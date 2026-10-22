use borg_core::{AppState, KEYMAP, Pointer};
use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{format, theme::Styles};

const CONTROL_PREFIX: char = '^';
const ALT_PREFIX: &str = "M-";
const ALIAS_HINT: &str = tr!(esc_then_1_9_0_f1_f10);
const HINT_FROM: u16 = 150;

pub(crate) fn actions_area(area: Rect) -> Rect {
    let hint = if area.width >= HINT_FROM { ALIAS_HINT.width() as u16 } else { 0 };
    Rect { width: area.width.saturating_sub(hint), ..area }
}

pub(crate) fn rows(area: Rect) -> (Rect, Rect) {
    let primary = Rect { height: area.height.min(1), ..area };
    let context = Rect { y: primary.bottom(), height: area.height.saturating_sub(primary.height).min(1), ..area };
    let context = crate::branding::content(context);
    (primary, context)
}

pub fn draw_keybar(
    state: &AppState,
    area: Rect,
    styles: &Styles<Style>,
    labels: (&str, &str),
    now: std::time::Instant,
    buf: &mut Buffer,
) {
    let (primary, context) = rows(area);
    if !primary.is_empty() {
        buf.set_style(primary, styles.key_label);
        draw_functions(primary, styles, labels, state.function_prefix(now), buf);
    }
    crate::shortcuts::draw(state, context, styles, buf);
    if let Some(notice) = state.notice.as_deref()
        && !context.is_empty()
    {
        Clear.render(context, buf);
        buf.set_style(context, styles.notice);
        Line::styled(format::truncate_end(notice, usize::from(context.width)), styles.notice).render(context, buf);
    }
}

pub(crate) fn action_at(state: &AppState, area: Rect, point: Position) -> Option<Pointer> {
    let (primary, context) = rows(area);
    if context.contains(point) {
        return if state.notice.is_some() {
            Some(Pointer::DismissNotice)
        } else {
            crate::shortcuts::action_at(state, context, point).map(Pointer::Action)
        };
    }
    let actions = actions_area(primary);
    let bindings: Vec<_> = KEYMAP.iter().filter(|binding| binding.label.is_some()).collect();
    let slot = actions.width / bindings.len() as u16;
    if !primary.contains(point) || !actions.contains(point) || slot == 0 {
        return None;
    }
    bindings.get(usize::from((point.x - actions.x) / slot)).map(|binding| Pointer::Action(binding.action))
}

/// Draws the function keys. After an Esc with nothing to close, a digit acts as the
/// function key, so the row shows the digits to press instead.
fn draw_functions(area: Rect, styles: &Styles<Style>, (copy, remove): (&str, &str), digits: bool, buf: &mut Buffer) {
    let items: Vec<(String, &str)> = KEYMAP
        .iter()
        .filter_map(|binding| {
            Some((
                match binding.code {
                    KeyCode::F(n @ 1..=10) if digits && binding.modifiers.is_empty() => (n % 10).to_string(),
                    code => key_name(code, binding.modifiers),
                },
                match binding.action {
                    borg_core::Action::Copy => copy,
                    borg_core::Action::Delete => remove,
                    _ => binding.label?,
                },
            ))
        })
        .collect();
    let actions = actions_area(area);
    let count = u16::try_from(items.len().max(1)).unwrap_or(u16::MAX);
    let slot = usize::from(actions.width / count);

    let spans: Vec<Span<'_>> = items
        .iter()
        .flat_map(|(key, label)| {
            let room = slot.saturating_sub(key.width());
            let key_style = if digits && key.len() == 1 { styles.cursor } else { styles.key };
            [
                Span::styled(format::truncate_end(key, slot), key_style),
                Span::styled(format!("{:<room$}", format::truncate_end(label, room)), styles.key_label),
            ]
        })
        .collect();
    Line::from(spans).render(actions, buf);
    if actions.width < area.width {
        Line::styled(ALIAS_HINT, styles.key_label).right_aligned().render(area, buf);
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
