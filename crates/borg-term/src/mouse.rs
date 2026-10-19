use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::TermMode;

const MOTION: u8 = 32;
const WHEEL: u8 = 64;
const LEGACY_OFFSET: u16 = 33;

pub fn encode_mouse(event: MouseEvent, mode: TermMode) -> Option<Vec<u8>> {
    if event.modifiers.contains(KeyModifiers::SHIFT) {
        return None;
    }
    if !mode.intersects(TermMode::MOUSE_MODE) {
        if !mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
            return None;
        }
        let key = match event.kind {
            MouseEventKind::ScrollUp => crossterm::event::KeyCode::Up,
            MouseEventKind::ScrollDown => crossterm::event::KeyCode::Down,
            _ => return None,
        };
        let key = crossterm::event::KeyEvent::new(key, event.modifiers);
        return crate::encode_key(&key, mode).map(|bytes| bytes.repeat(3));
    }
    let button = |button| match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    let (mut code, release) = match event.kind {
        MouseEventKind::Down(b) => (button(b), false),
        MouseEventKind::Up(b) => (button(b), true),
        MouseEventKind::Drag(b) if mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION) => {
            (button(b) + MOTION, false)
        }
        MouseEventKind::Moved if mode.contains(TermMode::MOUSE_MOTION) => (3 + MOTION, false),
        MouseEventKind::ScrollUp => (WHEEL, false),
        MouseEventKind::ScrollDown => (WHEEL + 1, false),
        MouseEventKind::ScrollLeft => (WHEEL + 2, false),
        MouseEventKind::ScrollRight => (WHEEL + 3, false),
        _ => return None,
    };
    if event.modifiers.contains(KeyModifiers::ALT) {
        code += 8;
    }
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        code += 16;
    }
    if mode.contains(TermMode::SGR_MOUSE) {
        let end = if release { 'm' } else { 'M' };
        return Some(
            format!("\x1b[<{code};{};{}{end}", u32::from(event.column) + 1, u32::from(event.row) + 1).into_bytes(),
        );
    }
    let code = if release { 3 } else { code };
    let coords = [event.column.checked_add(LEGACY_OFFSET)?, event.row.checked_add(LEGACY_OFFSET)?];
    let mut bytes = vec![0x1b, b'[', b'M', code + 32];
    for coord in coords {
        if mode.contains(TermMode::UTF8_MOUSE) {
            let mut encoded = [0; 4];
            bytes.extend_from_slice(char::from_u32(u32::from(coord))?.encode_utf8(&mut encoded).as_bytes());
        } else {
            bytes.push(u8::try_from(coord).ok()?);
        }
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mouse(kind: MouseEventKind) -> MouseEvent {
        MouseEvent { kind, column: 9, row: 4, modifiers: KeyModifiers::NONE }
    }

    #[test]
    fn mouse_reports_follow_the_programs_protocol() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert_eq!(encode_mouse(mouse(MouseEventKind::Down(MouseButton::Left)), mode), Some(b"\x1b[<0;10;5M".to_vec()));
        assert_eq!(encode_mouse(mouse(MouseEventKind::Up(MouseButton::Left)), mode), Some(b"\x1b[<0;10;5m".to_vec()));
        assert_eq!(encode_mouse(mouse(MouseEventKind::ScrollUp), mode), Some(b"\x1b[<64;10;5M".to_vec()));
        assert_eq!(encode_mouse(mouse(MouseEventKind::Moved), mode), None);
        assert_eq!(encode_mouse(mouse(MouseEventKind::Drag(MouseButton::Left)), mode), None);
        assert_eq!(
            encode_mouse(mouse(MouseEventKind::Drag(MouseButton::Left)), mode | TermMode::MOUSE_DRAG),
            Some(b"\x1b[<32;10;5M".to_vec())
        );
    }

    #[test]
    fn normal_shell_wheel_never_becomes_command_history_keys() {
        assert_eq!(encode_mouse(mouse(MouseEventKind::ScrollUp), TermMode::empty()), None);
        let mode = TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL;
        assert_eq!(encode_mouse(mouse(MouseEventKind::ScrollUp), mode), Some(b"\x1b[A\x1b[A\x1b[A".to_vec()));
        let shifted = MouseEvent { modifiers: KeyModifiers::SHIFT, ..mouse(MouseEventKind::Down(MouseButton::Left)) };
        assert_eq!(encode_mouse(shifted, TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE), None);
    }

    #[test]
    fn legacy_reports_do_not_wrap_large_coordinates() {
        let click = mouse(MouseEventKind::Down(MouseButton::Right));
        assert_eq!(encode_mouse(click, TermMode::MOUSE_REPORT_CLICK), Some(vec![0x1b, b'[', b'M', 34, 42, 37]));
        let far = MouseEvent { column: 300, ..click };
        assert_eq!(encode_mouse(far, TermMode::MOUSE_REPORT_CLICK), None);
        assert_eq!(
            encode_mouse(far, TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE),
            Some(b"\x1b[<2;301;5M".to_vec())
        );
    }
}
