use borg_term::{TermMode, encode_paste};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{AppState, Effect, Overlay, View, command::SUBMIT, input::Edit, update::prepare_shell};

const NEWLINE: char = '\n';
const CLEAR_LINE: &[u8] = b"\x05\x15";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Composer {
    text: String,
    cursor: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Up,
    Down,
}

impl Composer {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.len();
        Self { text, cursor }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.text.split(NEWLINE)
    }

    pub fn line_count(&self) -> usize {
        self.lines().count()
    }

    pub fn caret(&self) -> (usize, usize) {
        let before = &self.text[..self.cursor];
        let row = before.matches(NEWLINE).count();
        (row, before[self.line_start()..].chars().count())
    }

    pub fn apply(&mut self, key: &KeyEvent) -> Edit {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('u') if ctrl => self.remove(self.line_start(), self.cursor),
            KeyCode::Char('k') if ctrl => self.remove(self.cursor, self.line_end()),
            KeyCode::Char('w') if ctrl => self.remove(self.word_start(), self.cursor),
            KeyCode::Char('a') if ctrl => self.cursor = self.line_start(),
            KeyCode::Char('e') if ctrl => self.cursor = self.line_end(),
            KeyCode::Char(_) if ctrl => return Edit::Unhandled,
            KeyCode::Char(c) => self.insert(c.encode_utf8(&mut [0; 4])),
            KeyCode::Enter => self.insert(NEWLINE.encode_utf8(&mut [0; 4])),
            KeyCode::Backspace => self.remove(self.prev_boundary(), self.cursor),
            KeyCode::Delete => self.remove(self.cursor, self.next_boundary()),
            KeyCode::Left => self.cursor = self.prev_boundary(),
            KeyCode::Right => self.cursor = self.next_boundary(),
            KeyCode::Up => self.step(Step::Up),
            KeyCode::Down => self.step(Step::Down),
            KeyCode::Home => self.cursor = self.line_start(),
            KeyCode::End => self.cursor = self.line_end(),
            _ => return Edit::Unhandled,
        }
        Edit::Changed
    }

    pub fn insert(&mut self, text: &str) {
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    fn remove(&mut self, start: usize, end: usize) {
        self.text.replace_range(start..end, "");
        self.cursor = start;
    }

    fn line_start(&self) -> usize {
        self.text[..self.cursor].rfind(NEWLINE).map_or(0, |at| at + NEWLINE.len_utf8())
    }

    fn line_end(&self) -> usize {
        self.text[self.cursor..].find(NEWLINE).map_or(self.text.len(), |at| self.cursor + at)
    }

    fn prev_boundary(&self) -> usize {
        self.text[..self.cursor].char_indices().next_back().map_or(0, |(at, _)| at)
    }

    fn next_boundary(&self) -> usize {
        self.text[self.cursor..].chars().next().map_or(self.cursor, |c| self.cursor + c.len_utf8())
    }

    fn word_start(&self) -> usize {
        let start = self.line_start();
        let before = self.text[start..self.cursor].trim_end();
        before.rfind(char::is_whitespace).map_or(start, |at| start + at + 1)
    }

    fn step(&mut self, step: Step) {
        let (_, column) = self.caret();
        let target = match step {
            Step::Up => self.line_start().checked_sub(NEWLINE.len_utf8()).map(|end| {
                let start = self.text[..end].rfind(NEWLINE).map_or(0, |at| at + NEWLINE.len_utf8());
                (start, end)
            }),
            Step::Down => {
                let end = self.line_end();
                (end < self.text.len()).then(|| {
                    let start = end + NEWLINE.len_utf8();
                    (start, self.text[start..].find(NEWLINE).map_or(self.text.len(), |at| start + at))
                })
            }
        };
        if let Some((start, end)) = target {
            let line = &self.text[start..end];
            self.cursor = start + line.char_indices().nth(column).map_or(line.len(), |(at, _)| at);
        }
    }
}

pub(crate) fn open(text: String, state: &mut AppState) {
    state.overlay = Some(Overlay::Composer(Composer::new(text)));
}

fn runs(key: &KeyEvent) -> bool {
    key.code == KeyCode::Enter && key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL)
}

fn fits_on_prompt(text: &str, mode: TermMode) -> bool {
    mode.contains(TermMode::BRACKETED_PASTE) || !text.contains(NEWLINE)
}

fn replace_line(state: &mut AppState, text: &str) -> Vec<Effect> {
    state.shell.clear_line();
    let mut effects = vec![Effect::Write(CLEAR_LINE.to_vec())];
    effects.extend(prepare_shell(state));
    if !text.is_empty() {
        effects.push(Effect::Write(encode_paste(text, state.shell.observed.mode)));
        state.shell.mark_typed();
    }
    effects
}

fn run(state: &mut AppState, text: &str) -> Vec<Effect> {
    let mut effects = replace_line(state, text);
    effects.push(Effect::Write(SUBMIT.to_vec()));
    state.shell.submit();
    state.view = View::Shell;
    effects
}

pub(crate) fn on_key(state: &mut AppState, mut composer: Composer, key: &KeyEvent) -> Vec<Effect> {
    let mode = state.shell.observed.mode;
    match key.code {
        KeyCode::Esc => {
            state.overlay = None;
            if fits_on_prompt(composer.text(), mode) { replace_line(state, composer.text()) } else { Vec::new() }
        }
        KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
            state.overlay = None;
            Vec::new()
        }
        _ if runs(key) => {
            state.overlay = None;
            let text = composer.text().trim_end().to_owned();
            if text.is_empty() { replace_line(state, &text) } else { run(state, &text) }
        }
        _ => {
            if composer.apply(key) == Edit::Changed {
                state.overlay = Some(Overlay::Composer(composer));
            }
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(composer: &mut Composer, code: KeyCode) {
        composer.apply(&KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn typed(text: &str) -> Composer {
        let mut composer = Composer::default();
        text.chars().for_each(|c| {
            let code = if c == NEWLINE { KeyCode::Enter } else { KeyCode::Char(c) };
            press(&mut composer, code);
        });
        composer
    }

    #[test]
    fn enter_splits_lines_and_backspace_joins_them() {
        let mut composer = typed("for f in *\ndo echo $f\ndone");
        assert_eq!(composer.line_count(), 3);
        assert_eq!(composer.caret(), (2, 4));
        press(&mut composer, KeyCode::Home);
        press(&mut composer, KeyCode::Backspace);
        assert_eq!(composer.text(), "for f in *\ndo echo $fdone");
        assert_eq!(composer.caret(), (1, 10));
    }

    #[test]
    fn up_and_down_keep_the_column_when_the_line_is_long_enough() {
        let mut composer = typed("echo one\nls\necho three");
        press(&mut composer, KeyCode::Up);
        assert_eq!(composer.caret(), (1, 2));
        press(&mut composer, KeyCode::Up);
        assert_eq!(composer.caret(), (0, 2));
        press(&mut composer, KeyCode::End);
        press(&mut composer, KeyCode::Down);
        press(&mut composer, KeyCode::Down);
        assert_eq!(composer.caret(), (2, 2));
        press(&mut composer, KeyCode::End);
        press(&mut composer, KeyCode::Down);
        assert_eq!(composer.caret(), (2, 10));
    }

    #[test]
    fn line_shortcuts_stay_on_the_current_line() {
        let mut composer = typed("cd src\ncargo build --release");
        composer.apply(&KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(composer.text(), "cd src\ncargo build ");
        composer.apply(&KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(composer.text(), "cd src\n");
        press(&mut composer, KeyCode::Up);
        composer.apply(&KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert_eq!(composer.text(), "\n");
    }

    #[test]
    fn unicode_moves_by_characters() {
        let mut composer = typed("héllo\nwörld");
        press(&mut composer, KeyCode::Up);
        press(&mut composer, KeyCode::Left);
        press(&mut composer, KeyCode::Backspace);
        assert_eq!(composer.text(), "hélo\nwörld");
    }
}
