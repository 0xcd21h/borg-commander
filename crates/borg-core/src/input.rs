use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineInput {
    text: String,
    cursor: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    Changed,
    Unhandled,
}

impl LineInput {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.len();
        Self { text, cursor }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor_chars(&self) -> usize {
        self.text[..self.cursor].chars().count()
    }

    pub fn apply(&mut self, key: &KeyEvent) -> Edit {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('u') if ctrl => self.replace_range(0, self.cursor),
            KeyCode::Char('w') if ctrl => self.replace_range(self.word_start(), self.cursor),
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = self.text.len(),
            KeyCode::Char(_) if ctrl => return Edit::Unhandled,
            KeyCode::Char(c) => {
                self.text.insert(self.cursor, c);
                self.cursor += c.len_utf8();
            }
            KeyCode::Backspace => self.replace_range(self.prev_boundary(), self.cursor),
            KeyCode::Delete => self.replace_range(self.cursor, self.next_boundary()),
            KeyCode::Left => self.cursor = self.prev_boundary(),
            KeyCode::Right => self.cursor = self.next_boundary(),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.text.len(),
            _ => return Edit::Unhandled,
        }
        Edit::Changed
    }

    fn replace_range(&mut self, start: usize, end: usize) {
        self.text.replace_range(start..end, "");
        self.cursor = start;
    }

    fn prev_boundary(&self) -> usize {
        self.text[..self.cursor].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    fn next_boundary(&self) -> usize {
        self.text[self.cursor..].chars().next().map_or(self.cursor, |c| self.cursor + c.len_utf8())
    }

    fn word_start(&self) -> usize {
        let before = self.text[..self.cursor].trim_end();
        before.rfind(char::is_whitespace).map_or(0, |i| i + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(input: &mut LineInput, code: KeyCode) {
        input.apply(&KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn ctrl(input: &mut LineInput, c: char) {
        input.apply(&KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
    }

    #[test]
    fn edits_at_cursor_with_unicode() {
        let mut input = LineInput::new("héllo");
        press(&mut input, KeyCode::Left);
        press(&mut input, KeyCode::Left);
        press(&mut input, KeyCode::Char('X'));
        assert_eq!(input.text(), "hélXlo");
        press(&mut input, KeyCode::Home);
        press(&mut input, KeyCode::Delete);
        assert_eq!(input.text(), "élXlo");
        press(&mut input, KeyCode::Right);
        press(&mut input, KeyCode::Backspace);
        assert_eq!(input.text(), "lXlo");
    }

    #[test]
    fn readline_shortcuts() {
        let mut input = LineInput::new("tar czf archive");
        ctrl(&mut input, 'w');
        assert_eq!(input.text(), "tar czf ");
        ctrl(&mut input, 'u');
        assert_eq!(input.text(), "");
    }
}
