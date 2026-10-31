//! Key hints that take clicks: a click on `Esc close` in a dialog's border presses Esc.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Position, Rect};
use unicode_width::UnicodeWidthChar;

/// The key a hint's first word names: `Enter`, `Esc`, `F2`, `Ctrl-F`, `^O`, `Alt-A`, `S-F2`,
/// `Alt-Shift-F` or a letter.
pub(crate) fn key(word: &str) -> Option<KeyEvent> {
    const HELD: [(&str, KeyModifiers); 5] = [
        ("Ctrl-", KeyModifiers::CONTROL),
        ("^", KeyModifiers::CONTROL),
        ("Alt-", KeyModifiers::ALT),
        ("Shift-", KeyModifiers::SHIFT),
        ("S-", KeyModifiers::SHIFT),
    ];
    let mut name = word.trim_end_matches(':');
    let mut modifiers = KeyModifiers::NONE;
    while let Some((rest, held)) = HELD
        .iter()
        .find_map(|(prefix, held)| name.strip_prefix(prefix).filter(|rest| !rest.is_empty()).map(|rest| (rest, held)))
    {
        modifiers |= *held;
        name = rest;
    }
    let code = match name {
        "Enter" => KeyCode::Enter,
        "Esc" => KeyCode::Esc,
        "Space" => KeyCode::Char(' '),
        "Tab" => KeyCode::Tab,
        "Del" | "Supr" => KeyCode::Delete,
        "Ins" => KeyCode::Insert,
        name if name.len() > 1 && name.starts_with('F') => KeyCode::F(name[1..].parse().ok()?),
        name if name.len() == 1 && name.bytes().all(|b| b.is_ascii_alphanumeric()) => {
            KeyCode::Char(name.to_ascii_lowercase().chars().next()?)
        }
        _ => return None,
    };
    Some(KeyEvent::new(code, modifiers))
}

/// The key the part of `hint` under column `x` names, for a hint drawn from column `start`.
/// Hints list keys in parts split by `·` or by two spaces, each part starting with its key.
pub(crate) fn key_at(hint: &str, start: u16, x: u16) -> Option<KeyEvent> {
    let mut column = start;
    let mut part = String::new();
    let mut began = start;
    let chars: Vec<char> = hint.chars().collect();
    for (at, c) in chars.iter().enumerate() {
        let split = *c == '·' || (*c == ' ' && chars.get(at + 1) == Some(&' '));
        if split {
            if (began..column).contains(&x) {
                return key(part.split_whitespace().next()?);
            }
            part.clear();
            began = column + 1;
        } else {
            part.push(*c);
        }
        column += c.width().unwrap_or(0) as u16;
    }
    (began..column).contains(&x).then(|| key(part.split_whitespace().next()?)).flatten()
}

/// The key a click at `point` presses on the hint in the bottom border of `rect`, centred or
/// starting after the corner.
pub(crate) fn bottom(rect: Rect, hint: &str, centered: bool, point: Position) -> Option<KeyEvent> {
    use unicode_width::UnicodeWidthStr;
    if rect.height == 0 || point.y != rect.bottom() - 1 || !rect.contains(point) {
        return None;
    }
    let room = rect.width.saturating_sub(2);
    let width = (hint.width() as u16).min(room);
    let start = rect.x + 1 + if centered { (room - width) / 2 } else { 0 };
    key_at(hint, start, point.x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_name_the_keys_their_parts_start_with() {
        let hint = " Enter keep · Esc cancel ";
        assert_eq!(key_at(hint, 10, 12).map(|key| key.code), Some(KeyCode::Enter));
        assert_eq!(key_at(hint, 10, 25).map(|key| key.code), Some(KeyCode::Esc));
        assert_eq!(key_at(hint, 10, 22), None, "the separator presses nothing");
        let remote = " Enter files  M mirror  S-F2 session  Esc close ";
        let at = |text: &str| remote.find(text).unwrap() as u16;
        assert_eq!(key_at(remote, 0, at("mirror")), Some(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE)));
        assert_eq!(key_at(remote, 0, at("session")), Some(KeyEvent::new(KeyCode::F(2), KeyModifiers::SHIFT)));
        let editor = "F2 Save · Ctrl-F Find · Alt-A all · ↑↓ choose";
        let at = |text: &str| editor[..editor.find(text).unwrap()].chars().count() as u16;
        assert_eq!(key_at(editor, 0, at("Find")), Some(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)));
        assert_eq!(key_at(editor, 0, at("all")), Some(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::ALT)));
        assert_eq!(key_at(editor, 0, editor.chars().count() as u16 - 2), None, "arrows press nothing");
    }
}
