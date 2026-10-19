use borg_term::{TermMode, encode_key};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn key(code: KeyCode) -> Vec<u8> {
    with(code, KeyModifiers::NONE)
}

pub fn ctrl(c: char) -> Vec<u8> {
    with(KeyCode::Char(c), KeyModifiers::CONTROL)
}

pub fn alt(code: KeyCode) -> Vec<u8> {
    with(code, KeyModifiers::ALT)
}

pub fn with(code: KeyCode, modifiers: KeyModifiers) -> Vec<u8> {
    encode_key(&KeyEvent::new(code, modifiers), TermMode::empty()).unwrap_or_default()
}

pub fn up() -> Vec<u8> {
    key(KeyCode::Up)
}

pub fn down() -> Vec<u8> {
    key(KeyCode::Down)
}

pub fn left() -> Vec<u8> {
    key(KeyCode::Left)
}

pub fn right() -> Vec<u8> {
    key(KeyCode::Right)
}

pub fn enter() -> Vec<u8> {
    key(KeyCode::Enter)
}

pub fn tab() -> Vec<u8> {
    key(KeyCode::Tab)
}

pub fn esc() -> Vec<u8> {
    key(KeyCode::Esc)
}

pub fn f(n: u8) -> Vec<u8> {
    key(KeyCode::F(n))
}
