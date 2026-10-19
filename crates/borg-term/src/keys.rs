use alacritty_terminal::term::TermMode;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

const ESC: u8 = 0x1b;
const DEL: u8 = 0x7f;
const BACKSPACE_CTRL: u8 = 0x08;
const CONTROL_MASK: u8 = 0x1f;
const CSI: &[u8] = b"\x1b[";
const SS3: &[u8] = b"\x1bO";
const BACK_TAB: &[u8] = b"\x1b[Z";
const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";
const QUOTED_NEWLINE: &[u8] = b"\x16\n";

enum Sequence {
    Cursor(u8),
    Tilde(u8),
    Function(u8),
}

pub fn encode_key(key: &KeyEvent, mode: TermMode) -> Option<Vec<u8>> {
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    let plain = match key.code {
        KeyCode::Char(c) if ctrl => vec![control_byte(c)?],
        KeyCode::Char(c) => c.to_string().into_bytes(),
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => BACK_TAB.to_vec(),
        KeyCode::Backspace if ctrl => vec![BACKSPACE_CTRL],
        KeyCode::Backspace => vec![DEL],
        KeyCode::Esc => vec![ESC],
        code => return sequence_for(code).map(|seq| render(seq, key.modifiers, mode)),
    };

    Some(if alt { [&[ESC][..], &plain].concat() } else { plain })
}

pub fn encode_paste(text: &str, mode: TermMode) -> Vec<u8> {
    if mode.contains(TermMode::BRACKETED_PASTE) {
        [PASTE_START, text.as_bytes(), PASTE_END].concat()
    } else {
        text.as_bytes().to_vec()
    }
}

pub fn encode_newline(mode: TermMode) -> Vec<u8> {
    if mode.contains(TermMode::BRACKETED_PASTE) { encode_paste("\n", mode) } else { QUOTED_NEWLINE.to_vec() }
}

fn control_byte(c: char) -> Option<u8> {
    match c {
        ' ' | '@' => Some(0),
        '?' => Some(DEL),
        c if c.is_ascii_alphabetic() || ('['..='_').contains(&c) => {
            u8::try_from(c.to_ascii_uppercase()).ok().map(|b| b & CONTROL_MASK)
        }
        _ => None,
    }
}

fn sequence_for(code: KeyCode) -> Option<Sequence> {
    Some(match code {
        KeyCode::Up => Sequence::Cursor(b'A'),
        KeyCode::Down => Sequence::Cursor(b'B'),
        KeyCode::Right => Sequence::Cursor(b'C'),
        KeyCode::Left => Sequence::Cursor(b'D'),
        KeyCode::Home => Sequence::Cursor(b'H'),
        KeyCode::End => Sequence::Cursor(b'F'),
        KeyCode::Insert => Sequence::Tilde(2),
        KeyCode::Delete => Sequence::Tilde(3),
        KeyCode::PageUp => Sequence::Tilde(5),
        KeyCode::PageDown => Sequence::Tilde(6),
        KeyCode::F(n @ 1..=4) => Sequence::Function(b'P' + n - 1),
        KeyCode::F(n) => Sequence::Tilde(function_tilde(n)?),
        _ => return None,
    })
}

fn function_tilde(n: u8) -> Option<u8> {
    Some(match n {
        5 => 15,
        6 => 17,
        7 => 18,
        8 => 19,
        9 => 20,
        10 => 21,
        11 => 23,
        12 => 24,
        _ => return None,
    })
}

fn modifier_param(modifiers: KeyModifiers) -> Option<u8> {
    let bits = [(KeyModifiers::SHIFT, 1), (KeyModifiers::ALT, 2), (KeyModifiers::CONTROL, 4)]
        .into_iter()
        .filter(|(flag, _)| modifiers.contains(*flag))
        .map(|(_, bit)| bit)
        .sum::<u8>();
    (bits > 0).then_some(bits + 1)
}

fn render(sequence: Sequence, modifiers: KeyModifiers, mode: TermMode) -> Vec<u8> {
    let param = modifier_param(modifiers);
    match (sequence, param) {
        (Sequence::Cursor(final_byte), None) => {
            let intro = if mode.contains(TermMode::APP_CURSOR) { SS3 } else { CSI };
            [intro, &[final_byte]].concat()
        }
        (Sequence::Function(final_byte), None) => [SS3, &[final_byte]].concat(),
        (Sequence::Cursor(final_byte) | Sequence::Function(final_byte), Some(m)) => {
            [CSI, format!("1;{m}").as_bytes(), &[final_byte]].concat()
        }
        (Sequence::Tilde(code), None) => [CSI, format!("{code}~").as_bytes()].concat(),
        (Sequence::Tilde(code), Some(m)) => [CSI, format!("{code};{m}~").as_bytes()].concat(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn encode(code: KeyCode, modifiers: KeyModifiers) -> Vec<u8> {
        encode_key(&key(code, modifiers), TermMode::empty()).unwrap_or_default()
    }

    #[test]
    fn encodes_text_and_control_keys() {
        assert_eq!(encode(KeyCode::Char('é'), KeyModifiers::NONE), "é".as_bytes());
        assert_eq!(encode(KeyCode::Char('c'), KeyModifiers::CONTROL), [0x03]);
        assert_eq!(encode(KeyCode::Char('['), KeyModifiers::CONTROL), [ESC]);
        assert_eq!(encode(KeyCode::Char('b'), KeyModifiers::ALT), [ESC, b'b']);
        assert_eq!(encode(KeyCode::Backspace, KeyModifiers::NONE), [DEL]);
    }

    #[test]
    fn encodes_cursor_keys_by_mode() {
        let up = key(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(encode_key(&up, TermMode::empty()).unwrap_or_default(), b"\x1b[A");
        assert_eq!(encode_key(&up, TermMode::APP_CURSOR).unwrap_or_default(), b"\x1bOA");
        assert_eq!(encode(KeyCode::Left, KeyModifiers::CONTROL), b"\x1b[1;5D");
    }

    #[test]
    fn encodes_function_and_editing_keys() {
        assert_eq!(encode(KeyCode::F(1), KeyModifiers::NONE), b"\x1bOP");
        assert_eq!(encode(KeyCode::F(5), KeyModifiers::NONE), b"\x1b[15~");
        assert_eq!(encode(KeyCode::F(12), KeyModifiers::SHIFT), b"\x1b[24;2~");
        assert_eq!(encode(KeyCode::PageDown, KeyModifiers::NONE), b"\x1b[6~");
    }

    #[test]
    fn wraps_paste_when_requested() {
        assert_eq!(encode_paste("x", TermMode::BRACKETED_PASTE), b"\x1b[200~x\x1b[201~");
        assert_eq!(encode_paste("x", TermMode::empty()), b"x");
    }
}
