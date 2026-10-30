//! The built-in editor: a full-screen text editor over `borg_edit::Buffer`. Files are read and
//! written by the app; the editor keeps the stamp the file had when read, so a save notices a
//! file that changed on disk in the meantime.

use borg_i18n::tr;
use std::{path::PathBuf, time::SystemTime};

use borg_edit::{Buffer, Motion};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{AppState, Effect, LineInput};

/// Lines above the screen highlighted with it, so a comment or string opened there colours
/// what shows.
const PAINT_CONTEXT: usize = 300;
/// Rows the editor's title and status lines take from the screen.
pub const EDITOR_CHROME: u16 = 2;

/// What identifies a file's contents on disk without reading them again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    pub len: u64,
    pub modified: Option<SystemTime>,
    pub inode: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorFile {
    pub path: PathBuf,
    pub text: String,
    pub stamp: FileStamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// Not UTF-8 text, or binary: the viewer shows it instead.
    NotText(PathBuf),
    TooLarge(PathBuf),
    Failed(PathBuf, String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    /// The file on disk is no longer the one the editor read.
    Changed,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorPrompt {
    Find(LineInput),
    Line(LineInput),
    /// Closing with changes not saved.
    Unsaved,
    /// Saving over a file changed on disk.
    Changed,
}

#[derive(Debug, Clone)]
pub struct Editor {
    pub path: PathBuf,
    pub buffer: Buffer,
    pub stamp: FileStamp,
    /// The first line and screen column shown.
    pub top: usize,
    pub left: usize,
    pub prompt: Option<EditorPrompt>,
    /// A message in the status line, until the next key.
    pub note: Option<String>,
    pub language: Option<String>,
    /// Syntax colours: the revision and first line they were made for, and each line's runs.
    pub painted: Option<(u64, usize, Vec<Vec<borg_view::Run>>)>,
    needle: String,
    /// The revision a save in flight wrote, and whether the editor closes once it lands.
    saving: Option<(u64, bool)>,
}

impl Editor {
    fn new(file: EditorFile) -> Self {
        let language = borg_view::language_of(&file.path, file.text.lines().next().unwrap_or_default());
        Self {
            buffer: Buffer::new(&file.text),
            path: file.path,
            stamp: file.stamp,
            top: 0,
            left: 0,
            prompt: None,
            note: None,
            language,
            painted: None,
            needle: String::new(),
            saving: None,
        }
    }

    /// The width of the line numbers and the space after them.
    pub fn gutter(&self) -> u16 {
        self.buffer.lines().to_string().len() as u16 + 2
    }

    /// Scrolls so the cursor shows in a text area of `rows` × `columns`.
    fn follow(&mut self, rows: usize, columns: usize) {
        let (line, _) = self.buffer.position();
        if line < self.top {
            self.top = line;
        } else if line >= self.top + rows.max(1) {
            self.top = line + 1 - rows.max(1);
        }
        let column = self.buffer.screen_column();
        let columns = columns.max(1);
        if column < self.left {
            self.left = column;
        } else if column >= self.left + columns {
            self.left = column + 1 - columns;
        }
    }

    /// Highlights the lines from `top` down, with context above, when the text or the view
    /// moved past what is coloured.
    fn paint(&mut self, rows: usize) {
        let Some(language) = &self.language else { return };
        let revision = self.buffer.revision();
        let fresh = self.painted.as_ref().is_some_and(|(painted, first, lines)| {
            *painted == revision && self.top >= *first && self.top + rows <= first + lines.len()
        });
        if fresh {
            return;
        }
        let first = self.top.saturating_sub(PAINT_CONTEXT);
        let last = (self.top + rows + PAINT_CONTEXT / 3).min(self.buffer.lines());
        let text: String = (first..last).map(|line| self.buffer.line(line) + "\n").collect();
        self.painted = Some((revision, first, borg_view::highlight_lines(&text, language)));
    }

    fn save(&mut self, close: bool, force: bool) -> Vec<Effect> {
        self.saving = Some((self.buffer.revision(), close));
        self.note = None;
        vec![Effect::SaveFile {
            path: self.path.clone(),
            bytes: self.buffer.text().into_bytes(),
            expected: self.stamp,
            force,
        }]
    }

    fn name(&self) -> String {
        self.path.file_name().unwrap_or(self.path.as_os_str()).to_string_lossy().into_owned()
    }
}

/// The text area's size on a screen of `cols` × `rows`, beside the gutter.
fn text_size(state: &AppState, editor: &Editor) -> (usize, usize) {
    let rows = usize::from(state.rows.saturating_sub(EDITOR_CHROME));
    let columns = usize::from(state.cols.saturating_sub(editor.gutter()));
    (rows, columns)
}

pub(crate) fn open(path: PathBuf) -> Vec<Effect> {
    vec![Effect::OpenEditor(path)]
}

pub(crate) fn opened(state: &mut AppState, result: Result<EditorFile, OpenError>) -> Vec<Effect> {
    match result {
        Ok(file) => {
            let mut editor = Editor::new(file);
            let (rows, _) = text_size(state, &editor);
            editor.paint(rows);
            state.editor = Some(editor);
            Vec::new()
        }
        Err(OpenError::NotText(path)) => {
            state.notice = Some(tr!(editor_not_text, name = display_name(&path)));
            vec![Effect::View(path)]
        }
        Err(OpenError::TooLarge(path)) => {
            state.notice = Some(tr!(editor_too_large, name = display_name(&path)));
            vec![Effect::View(path)]
        }
        Err(OpenError::Failed(path, error)) => {
            state.notice = Some(tr!(editor_open_failed, name = display_name(&path), error = error));
            Vec::new()
        }
    }
}

fn display_name(path: &std::path::Path) -> String {
    path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned()
}

pub(crate) fn saved(state: &mut AppState, result: Result<FileStamp, SaveError>) -> Vec<Effect> {
    let Some(editor) = state.editor.as_mut() else { return Vec::new() };
    let Some((revision, close)) = editor.saving.take() else { return Vec::new() };
    match result {
        Ok(stamp) => {
            editor.stamp = stamp;
            if editor.buffer.revision() == revision {
                editor.buffer.mark_saved();
            }
            editor.note = Some(tr!(editor_saved).into());
            if close {
                return close_editor(state);
            }
        }
        Err(SaveError::Changed) => {
            editor.saving = Some((revision, close));
            editor.prompt = Some(EditorPrompt::Changed);
        }
        Err(SaveError::Failed(error)) => editor.note = Some(tr!(editor_save_failed, error = error)),
    }
    Vec::new()
}

fn close_editor(state: &mut AppState) -> Vec<Effect> {
    state.editor = None;
    vec![state.reload(state.active)]
}

pub(crate) fn paste(state: &mut AppState, text: &str) -> Vec<Effect> {
    let Some(mut editor) = state.editor.take() else { return Vec::new() };
    match editor.prompt.as_mut() {
        Some(EditorPrompt::Find(input) | EditorPrompt::Line(input)) => {
            for c in text.chars().filter(|c| !c.is_control()).take(512) {
                input.apply(&KeyCode::Char(c).into());
            }
        }
        Some(_) => {}
        None => editor.buffer.insert(text),
    }
    settle(state, editor);
    Vec::new()
}

fn settle(state: &mut AppState, mut editor: Editor) {
    let (rows, columns) = text_size(state, &editor);
    editor.follow(rows, columns);
    editor.paint(rows);
    state.editor = Some(editor);
}

pub(crate) fn resize(state: &mut AppState) {
    if let Some(editor) = state.editor.take() {
        settle(state, editor);
    }
}

pub(crate) fn on_key(state: &mut AppState, key: &KeyEvent) -> Vec<Effect> {
    let Some(mut editor) = state.editor.take() else { return Vec::new() };
    editor.note = None;
    let effects = match editor.prompt.take() {
        Some(prompt) => on_prompt(state, &mut editor, prompt, key),
        None => on_edit(state, &mut editor, key),
    };
    // Closing reloads the panel; otherwise the editor stays, also while a reload is read.
    if !effects.iter().any(|effect| matches!(effect, Effect::List { .. })) {
        settle(state, editor);
    }
    effects
}

fn on_prompt(state: &mut AppState, editor: &mut Editor, prompt: EditorPrompt, key: &KeyEvent) -> Vec<Effect> {
    match (prompt, key.code) {
        (_, KeyCode::Esc) => {}
        (EditorPrompt::Find(input), KeyCode::Enter) => {
            editor.needle = input.text().to_owned();
            find(editor, true);
        }
        (EditorPrompt::Line(input), KeyCode::Enter) => match input.text().trim().parse::<usize>() {
            Ok(line) => editor.buffer.go_to_line(line),
            Err(_) => editor.prompt = Some(EditorPrompt::Line(input)),
        },
        (EditorPrompt::Find(mut input), _) => {
            input.apply(key);
            editor.prompt = Some(EditorPrompt::Find(input));
        }
        (EditorPrompt::Line(mut input), _) => {
            if !matches!(key.code, KeyCode::Char(c) if !c.is_ascii_digit()) {
                input.apply(key);
            }
            editor.prompt = Some(EditorPrompt::Line(input));
        }
        (EditorPrompt::Unsaved, KeyCode::Enter | KeyCode::Char('s' | 'S')) => return editor.save(true, false),
        (EditorPrompt::Unsaved, KeyCode::Char('n' | 'N' | 'd' | 'D')) => return close_editor(state),
        (EditorPrompt::Changed, KeyCode::Char('o' | 'O')) => {
            let close = editor.saving.is_some_and(|(_, close)| close);
            return editor.save(close, true);
        }
        (EditorPrompt::Changed, KeyCode::Char('r' | 'R')) => return vec![Effect::OpenEditor(editor.path.clone())],
        (prompt, _) => editor.prompt = Some(prompt),
    }
    Vec::new()
}

fn find(editor: &mut Editor, forward: bool) {
    if !editor.buffer.find(&editor.needle, forward) && !editor.needle.is_empty() {
        editor.note = Some(tr!(editor_no_match, text = editor.needle));
    }
}

fn on_edit(state: &mut AppState, editor: &mut Editor, key: &KeyEvent) -> Vec<Effect> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let (rows, _) = text_size(state, editor);
    let buffer = &mut editor.buffer;
    let motion = match key.code {
        KeyCode::Left if ctrl || alt => Some(Motion::WordLeft),
        KeyCode::Right if ctrl || alt => Some(Motion::WordRight),
        KeyCode::Home if ctrl => Some(Motion::Start),
        KeyCode::End if ctrl => Some(Motion::Finish),
        KeyCode::Left => Some(Motion::Left),
        KeyCode::Right => Some(Motion::Right),
        KeyCode::Up => Some(Motion::Up),
        KeyCode::Down => Some(Motion::Down),
        KeyCode::Home => Some(Motion::Home),
        KeyCode::End => Some(Motion::End),
        KeyCode::PageUp => Some(Motion::PageUp(rows)),
        KeyCode::PageDown => Some(Motion::PageDown(rows)),
        _ => None,
    };
    if let Some(motion) = motion {
        buffer.move_to(motion, shift);
        return Vec::new();
    }
    match key.code {
        KeyCode::Esc | KeyCode::F(10) => {
            if buffer.modified() {
                editor.prompt = Some(EditorPrompt::Unsaved);
            } else {
                return close_editor(state);
            }
        }
        KeyCode::F(2) => return editor.save(false, false),
        KeyCode::Char('s') if ctrl => return editor.save(false, false),
        KeyCode::Char('f') if ctrl => editor.prompt = Some(EditorPrompt::Find(LineInput::new(editor.needle.clone()))),
        KeyCode::F(7) => editor.prompt = Some(EditorPrompt::Find(LineInput::new(editor.needle.clone()))),
        KeyCode::F(3) => find(editor, !shift),
        KeyCode::Char('g') if ctrl => editor.prompt = Some(EditorPrompt::Line(LineInput::default())),
        KeyCode::Char('z') if ctrl && shift => {
            buffer.redo();
        }
        KeyCode::Char('Z') if ctrl => {
            buffer.redo();
        }
        KeyCode::Char('z') if ctrl => {
            buffer.undo();
        }
        KeyCode::Char('y') if ctrl => {
            buffer.redo();
        }
        KeyCode::Char('a') if ctrl => buffer.select_all(),
        KeyCode::Char('c') if ctrl => {
            if let Some(text) = buffer.selected_text() {
                return crate::clipboard::copy(state, text);
            }
        }
        KeyCode::Char('x') if ctrl => {
            if let Some(text) = buffer.cut() {
                return crate::clipboard::copy(state, text);
            }
        }
        KeyCode::Char('v') if ctrl => {
            if let Some(text) = state.clipboard.latest() {
                buffer.insert(text);
            }
        }
        KeyCode::Insert if shift => {
            if let Some(text) = state.clipboard.latest() {
                buffer.insert(text);
            }
        }
        KeyCode::Backspace if ctrl || alt => buffer.delete_word_back(),
        KeyCode::Char('w') if ctrl => buffer.delete_word_back(),
        KeyCode::Backspace => buffer.backspace(),
        KeyCode::Delete => buffer.delete(),
        KeyCode::Enter => buffer.newline(),
        KeyCode::Tab => buffer.indent(false),
        KeyCode::BackTab => buffer.indent(true),
        KeyCode::Char(c) if !ctrl && !alt => buffer.type_char(c),
        _ => {}
    }
    Vec::new()
}

/// The status line's prompt or note, for the renderer.
pub fn status(editor: &Editor) -> Option<(String, Option<&LineInput>)> {
    let name = editor.name();
    match &editor.prompt {
        Some(EditorPrompt::Find(input)) => Some((tr!(editor_find).into(), Some(input))),
        Some(EditorPrompt::Line(input)) => Some((tr!(editor_line).into(), Some(input))),
        Some(EditorPrompt::Unsaved) => Some((tr!(editor_unsaved, name = name), None)),
        Some(EditorPrompt::Changed) => Some((tr!(editor_changed, name = name), None)),
        None => editor.note.clone().map(|note| (note, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Appearance, Msg, Settings, Startup, ThemeChoice, update};
    use std::time::Instant;

    fn state() -> (AppState, Instant) {
        let now = Instant::now();
        let themes = vec![ThemeChoice { id: "midnight".into(), name: "Midnight".into() }];
        let (state, _) =
            AppState::new(Startup::at("/work".into()), 80, 12, Settings::default(), Appearance::new(themes, 0), now);
        (state, now)
    }

    fn stamp(len: u64) -> FileStamp {
        FileStamp { len, modified: None, inode: 7 }
    }

    fn opened(state: &mut AppState, now: Instant, text: &str) {
        let file = EditorFile { path: "/work/notes.rs".into(), text: text.into(), stamp: stamp(text.len() as u64) };
        update(state, Msg::EditorOpened(Ok(file)), now);
    }

    fn press(state: &mut AppState, now: Instant, key: KeyEvent) -> Vec<Effect> {
        update(state, Msg::Key(key), now)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn typing_saves_with_the_stamp_read_and_lands_clean() {
        let (mut state, now) = state();
        opened(&mut state, now, "fn main() {}\n");
        for c in "// hi\n".chars() {
            let key = if c == '\n' { KeyCode::Enter.into() } else { KeyCode::Char(c).into() };
            press(&mut state, now, key);
        }
        let effects = press(&mut state, now, ctrl('s'));
        let [Effect::SaveFile { bytes, expected, force: false, .. }] = &effects[..] else { panic!("{effects:?}") };
        assert_eq!(String::from_utf8_lossy(bytes), "// hi\nfn main() {}\n");
        assert_eq!(*expected, stamp(13));
        update(&mut state, Msg::EditorSaved(Ok(stamp(19))), now);
        let editor = state.editor.as_ref().unwrap();
        assert!(!editor.buffer.modified() && editor.stamp == stamp(19));
        assert!(editor.painted.is_some(), "Rust is highlighted");
    }

    #[test]
    fn closing_with_changes_asks_and_a_changed_file_can_be_overwritten() {
        let (mut state, now) = state();
        opened(&mut state, now, "a");
        press(&mut state, now, KeyCode::Char('b').into());
        press(&mut state, now, KeyCode::Esc.into());
        assert_eq!(state.editor.as_ref().unwrap().prompt, Some(EditorPrompt::Unsaved));
        let effects = press(&mut state, now, KeyCode::Enter.into());
        assert!(matches!(&effects[..], [Effect::SaveFile { force: false, .. }]));
        update(&mut state, Msg::EditorSaved(Err(SaveError::Changed)), now);
        assert_eq!(state.editor.as_ref().unwrap().prompt, Some(EditorPrompt::Changed));
        let effects = press(&mut state, now, KeyCode::Char('o').into());
        assert!(matches!(&effects[..], [Effect::SaveFile { force: true, .. }]));
        let effects = update(&mut state, Msg::EditorSaved(Ok(stamp(2))), now);
        assert!(state.editor.is_none(), "the save that closing asked for closes the editor");
        assert!(
            effects.iter().any(|effect| matches!(effect, Effect::List { .. })),
            "the panel shows the new size: {effects:?}"
        );
    }

    #[test]
    fn copy_cut_and_paste_go_through_borgs_clipboard() {
        let (mut state, now) = state();
        opened(&mut state, now, "one two");
        press(&mut state, now, KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL | KeyModifiers::SHIFT));
        assert_eq!(press(&mut state, now, ctrl('x')), vec![Effect::Copy("one".into())]);
        press(&mut state, now, KeyCode::End.into());
        press(&mut state, now, ctrl('v'));
        update(&mut state, Msg::Paste(" three".into()), now);
        assert_eq!(state.editor.as_ref().unwrap().buffer.text(), " twoone three");
    }

    #[test]
    fn find_and_go_to_line_use_the_status_line() {
        let (mut state, now) = state();
        opened(&mut state, now, &(1..=40).map(|n| format!("line {n}\n")).collect::<String>());
        press(&mut state, now, ctrl('g'));
        for c in "30".chars() {
            press(&mut state, now, KeyCode::Char(c).into());
        }
        press(&mut state, now, KeyCode::Enter.into());
        let editor = state.editor.as_ref().unwrap();
        assert_eq!(editor.buffer.position(), (29, 0));
        assert!(editor.top <= 29 && 29 < editor.top + 10, "the cursor stays on screen");
        press(&mut state, now, ctrl('f'));
        update(&mut state, Msg::Paste("line 4".into()), now);
        press(&mut state, now, KeyCode::Enter.into());
        assert_eq!(state.editor.as_ref().unwrap().buffer.selection(), Some(303..309), "line 40 matches after line 30");
        press(&mut state, now, KeyCode::F(3).into());
        assert_eq!(state.editor.as_ref().unwrap().buffer.position(), (3, 6));
    }
}
