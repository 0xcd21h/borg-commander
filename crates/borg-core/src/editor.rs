//! The built-in editor: a full-screen text editor over `borg_edit::Buffer`. Files are read and
//! written by the app; the editor keeps the stamp the file had when read, so a save notices a
//! file that changed on disk in the meantime.

use borg_i18n::tr;
use std::{path::PathBuf, time::SystemTime};

use borg_edit::{Buffer, Motion};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{AppState, Effect, LineInput, Pointer};

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

/// A dialog over the editor. Its fields and then its buttons share one focus, counted from 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorPrompt {
    Find {
        input: LineInput,
        focus: usize,
    },
    Replace {
        find: LineInput,
        with: LineInput,
        focus: usize,
    },
    Line {
        input: LineInput,
        focus: usize,
    },
    /// Closing with changes not saved.
    Unsaved {
        focus: usize,
    },
    /// Saving over a file changed on disk.
    Changed {
        focus: usize,
    },
}

/// What an editor dialog shows, for the renderer and for clicks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorDialog<'a> {
    pub title: &'static str,
    pub message: Option<String>,
    pub fields: Vec<(&'static str, &'a LineInput)>,
    pub buttons: Vec<&'static str>,
    pub focus: usize,
    pub keys: &'static str,
}

impl EditorPrompt {
    fn find(needle: &str) -> Self {
        Self::Find { input: LineInput::new(needle), focus: 0 }
    }

    fn focus_mut(&mut self) -> &mut usize {
        match self {
            Self::Find { focus, .. }
            | Self::Replace { focus, .. }
            | Self::Line { focus, .. }
            | Self::Unsaved { focus }
            | Self::Changed { focus } => focus,
        }
    }

    fn fields(&self) -> usize {
        match self {
            Self::Replace { .. } => 2,
            Self::Find { .. } | Self::Line { .. } => 1,
            Self::Unsaved { .. } | Self::Changed { .. } => 0,
        }
    }

    fn buttons(&self) -> usize {
        if matches!(self, Self::Replace { .. }) {
            4
        } else if matches!(self, Self::Find { .. } | Self::Line { .. }) {
            2
        } else {
            3
        }
    }

    fn field_mut(&mut self) -> Option<&mut LineInput> {
        match self {
            Self::Find { input, focus: 0 } | Self::Line { input, focus: 0 } => Some(input),
            Self::Replace { find, focus: 0, .. } => Some(find),
            Self::Replace { with, focus: 1, .. } => Some(with),
            _ => None,
        }
    }
}

/// Syntax colours for the lines from `first`, with the text and state each was coloured from,
/// so that after an edit only the lines it changed are coloured again.
#[derive(Debug, Clone)]
struct Painted {
    revision: u64,
    first: usize,
    /// The text's line count when coloured, to find the lines below an edit again.
    total: usize,
    texts: Vec<String>,
    lines: Vec<Vec<borg_view::Run>>,
    /// Where colouring stood at the start of each line, and after the last.
    states: Vec<borg_view::LineState>,
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
    painted: Option<Painted>,
    needle: String,
    replacement: String,
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
            replacement: String::new(),
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

    /// The syntax colours of line `line`, when it is coloured.
    pub fn runs(&self, line: usize) -> Option<&[borg_view::Run]> {
        let painted = self.painted.as_ref()?;
        painted.lines.get(line.checked_sub(painted.first)?).map(Vec::as_slice)
    }

    /// Highlights the lines from `top` down, with context above, when the text or the view
    /// moved past what is coloured. A line keeps its colours while its text and the state
    /// colouring reaches it in stay the same, so an edit colours little more than its own lines.
    fn paint(&mut self, rows: usize) {
        let Some(language) = &self.language else { return };
        let changed = self.buffer.take_changed_line();
        let revision = self.buffer.revision();
        let fresh = self.painted.as_ref().is_some_and(|painted| {
            painted.revision == revision
                && self.top >= painted.first
                && self.top + rows <= painted.first + painted.lines.len()
        });
        if fresh {
            return;
        }
        let wanted = self.top.saturating_sub(PAINT_CONTEXT);
        let last = (self.top + rows + PAINT_CONTEXT / 3).min(self.buffer.lines());
        // Colouring carries on from what is coloured above the view, unless a change above that
        // may have altered the state it starts in.
        let old = self.painted.take().filter(|old| {
            let first = wanted.max(old.first);
            old.first <= self.top && first <= old.first + old.lines.len() && changed.is_none_or(|line| line >= first)
        });
        let first = old.as_ref().map_or(wanted, |old| wanted.max(old.first));
        let mut state = old
            .as_ref()
            .map_or_else(|| borg_view::LineState::new(language), |old| old.states[first - old.first].clone());
        let total = self.buffer.lines();
        let shift = old.as_ref().map_or(0, |old| total as isize - old.total as isize);
        let mut painted = Painted { revision, first, total, texts: Vec::new(), lines: Vec::new(), states: Vec::new() };
        for line in first..last {
            let text = self.buffer.line(line);
            // Below the change, the lines that were there before have moved by the lines it added.
            let before = old.as_ref().and_then(|old| {
                let at = if changed.is_none_or(|changed| line < changed) {
                    Some(line)
                } else {
                    line.checked_add_signed(-shift)
                };
                let index = at?.checked_sub(old.first)?;
                (old.texts.get(index) == Some(&text) && old.states[index] == state).then_some(index)
            });
            painted.states.push(state.clone());
            match (before, &old) {
                (Some(index), Some(old)) => {
                    painted.lines.push(old.lines[index].clone());
                    state = old.states[index + 1].clone();
                }
                _ => painted.lines.push(borg_view::highlight_line(&text, &mut state)),
            }
            painted.texts.push(text);
        }
        painted.states.push(state);
        self.painted = Some(painted);
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
            vec![crate::viewer::request(state, path)]
        }
        Err(OpenError::TooLarge(path)) => {
            state.notice = Some(tr!(editor_too_large, name = display_name(&path)));
            vec![crate::viewer::request(state, path)]
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
            editor.prompt = Some(EditorPrompt::Changed { focus: 0 });
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
        Some(prompt) => {
            let line = matches!(prompt, EditorPrompt::Line { .. });
            if let Some(input) = prompt.field_mut() {
                for c in text.chars().filter(|c| !c.is_control() && (!line || c.is_ascii_digit())).take(512) {
                    input.apply(&KeyCode::Char(c).into());
                }
            }
        }
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

fn on_prompt(state: &mut AppState, editor: &mut Editor, mut prompt: EditorPrompt, key: &KeyEvent) -> Vec<Effect> {
    let (fields, count) = (prompt.fields(), prompt.fields() + prompt.buttons());
    let focus = *prompt.focus_mut();
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let button = match (&prompt, key.code) {
        (_, KeyCode::Esc) => return Vec::new(),
        (_, KeyCode::Enter) => Some(focus.saturating_sub(fields)),
        (_, KeyCode::Tab | KeyCode::Down) => {
            *prompt.focus_mut() = (focus + 1) % count;
            None
        }
        (_, KeyCode::BackTab | KeyCode::Up) => {
            *prompt.focus_mut() = (focus + count - 1) % count;
            None
        }
        (_, KeyCode::Left | KeyCode::Right) if focus >= fields => {
            let buttons = count - fields;
            let step = if key.code == KeyCode::Left { buttons - 1 } else { 1 };
            *prompt.focus_mut() = fields + (focus - fields + step) % buttons;
            None
        }
        (EditorPrompt::Replace { .. }, KeyCode::Char('a' | 'A')) if alt => Some(1),
        (EditorPrompt::Unsaved { .. }, KeyCode::Char('s' | 'S')) => Some(0),
        (EditorPrompt::Unsaved { .. }, KeyCode::Char('n' | 'N' | 'd' | 'D')) => Some(1),
        (EditorPrompt::Changed { .. }, KeyCode::Char('o' | 'O')) => Some(0),
        (EditorPrompt::Changed { .. }, KeyCode::Char('r' | 'R')) => Some(1),
        (_, code) => {
            let digits = matches!(prompt, EditorPrompt::Line { .. });
            if let Some(input) =
                prompt.field_mut().filter(|_| !digits || !matches!(code, KeyCode::Char(c) if !c.is_ascii_digit()))
            {
                input.apply(key);
            }
            None
        }
    };
    match button {
        Some(button) => press(state, editor, prompt, button),
        None => {
            editor.prompt = Some(prompt);
            Vec::new()
        }
    }
}

/// Presses the dialog's button `button`; the dialog closes unless it stays to go on.
fn press(state: &mut AppState, editor: &mut Editor, prompt: EditorPrompt, button: usize) -> Vec<Effect> {
    match (prompt, button) {
        (EditorPrompt::Find { input, .. }, 0) => {
            editor.needle = input.text().to_owned();
            find(editor, true);
        }
        (EditorPrompt::Replace { find: needle, with, focus }, button @ (0 | 2)) => {
            editor.needle = needle.text().to_owned();
            editor.replacement = with.text().to_owned();
            if button == 0 && editor.buffer.selection_matches(&editor.needle) {
                editor.buffer.insert(&editor.replacement);
            }
            find(editor, true);
            editor.prompt = Some(EditorPrompt::Replace { find: needle, with, focus });
        }
        (EditorPrompt::Replace { find: needle, with, .. }, 1) => {
            editor.needle = needle.text().to_owned();
            editor.replacement = with.text().to_owned();
            let count = editor.buffer.replace_all(&editor.needle, &editor.replacement);
            editor.note = Some(if count == 0 {
                tr!(editor_no_match, text = editor.needle)
            } else {
                tr!(editor_replaced, count = count)
            });
        }
        (EditorPrompt::Line { input, focus }, 0) => match input.text().trim().parse::<usize>() {
            Ok(line) => editor.buffer.go_to_line(line),
            Err(_) => editor.prompt = Some(EditorPrompt::Line { input, focus }),
        },
        (EditorPrompt::Unsaved { .. }, 0) => return editor.save(true, false),
        (EditorPrompt::Unsaved { .. }, 1) => return close_editor(state),
        (EditorPrompt::Changed { .. }, 0) => {
            let close = editor.saving.is_some_and(|(_, close)| close);
            return editor.save(close, true);
        }
        (EditorPrompt::Changed { .. }, 1) => return vec![Effect::OpenEditor(editor.path.clone())],
        _ => {}
    }
    Vec::new()
}

/// A click, drag or wheel turn in the editor, or a click on its dialog's field or button.
pub(crate) fn on_pointer(state: &mut AppState, pointer: Pointer) -> Vec<Effect> {
    let Some(mut editor) = state.editor.take() else { return Vec::new() };
    let mut follow = true;
    let effects = match (pointer, editor.prompt.take()) {
        (Pointer::EditorControl(control), Some(mut prompt)) => {
            let fields = prompt.fields();
            if control < fields {
                *prompt.focus_mut() = control;
                editor.prompt = Some(prompt);
                Vec::new()
            } else {
                *prompt.focus_mut() = control;
                press(state, &mut editor, prompt, control - fields)
            }
        }
        (Pointer::EditorClick { line, column, select, word }, None) => {
            editor.note = None;
            if word {
                editor.buffer.select_word(line, column);
            } else {
                editor.buffer.place(line, column, select);
            }
            Vec::new()
        }
        (Pointer::EditorScroll(delta), prompt) => {
            // The view moves and the cursor stays where it is, until a key brings it back.
            editor.top = editor.top.saturating_add_signed(delta).min(editor.buffer.lines().saturating_sub(1));
            editor.prompt = prompt;
            follow = false;
            Vec::new()
        }
        (_, prompt) => {
            editor.prompt = prompt;
            Vec::new()
        }
    };
    if !effects.iter().any(|effect| matches!(effect, Effect::List { .. })) {
        if follow {
            settle(state, editor);
        } else {
            let (rows, _) = text_size(state, &editor);
            editor.paint(rows);
            state.editor = Some(editor);
        }
    }
    effects
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
                editor.prompt = Some(EditorPrompt::Unsaved { focus: 0 });
            } else {
                return close_editor(state);
            }
        }
        KeyCode::F(2) => return editor.save(false, false),
        KeyCode::Char('s') if ctrl => return editor.save(false, false),
        KeyCode::Char('f') if ctrl => editor.prompt = Some(EditorPrompt::find(&needle(editor))),
        KeyCode::F(7) => editor.prompt = Some(EditorPrompt::find(&needle(editor))),
        KeyCode::Char('r') if ctrl => editor.prompt = Some(replace(editor)),
        KeyCode::F(4) => editor.prompt = Some(replace(editor)),
        KeyCode::F(3) => find(editor, !shift),
        KeyCode::Char('g') if ctrl => {
            editor.prompt = Some(EditorPrompt::Line { input: LineInput::default(), focus: 0 })
        }
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

/// What to look for: a selection within one line, or else what was looked for last.
fn needle(editor: &Editor) -> String {
    editor.buffer.selected_text().filter(|text| !text.contains('\n')).unwrap_or_else(|| editor.needle.clone())
}

fn replace(editor: &Editor) -> EditorPrompt {
    EditorPrompt::Replace {
        find: LineInput::new(needle(editor)),
        with: LineInput::new(editor.replacement.as_str()),
        focus: 0,
    }
}

/// The dialog open over the editor, for the renderer.
pub fn dialog(editor: &Editor) -> Option<EditorDialog<'_>> {
    let name = editor.name();
    let keys = tr!(editor_dialog_keys);
    let (title, message, fields, buttons, keys) = match editor.prompt.as_ref()? {
        EditorPrompt::Find { input, .. } => (
            tr!(editor_find_title),
            None,
            vec![(tr!(editor_find), input)],
            vec![tr!(editor_find_action), tr!(cancel)],
            keys,
        ),
        EditorPrompt::Replace { find, with, .. } => (
            tr!(editor_replace_title),
            None,
            vec![(tr!(editor_find), find), (tr!(editor_replace_with), with)],
            vec![tr!(editor_replace_action), tr!(editor_replace_all), tr!(editor_skip), tr!(editor_close_action)],
            tr!(editor_replace_keys),
        ),
        EditorPrompt::Line { input, .. } => {
            (tr!(editor_line_title), None, vec![(tr!(editor_line), input)], vec![tr!(editor_go), tr!(cancel)], keys)
        }
        EditorPrompt::Unsaved { .. } => (
            tr!(editor_unsaved_title),
            Some(tr!(editor_unsaved, name = name)),
            Vec::new(),
            vec![tr!(editor_save_action), tr!(editor_discard), tr!(cancel)],
            keys,
        ),
        EditorPrompt::Changed { .. } => (
            tr!(editor_changed_title),
            Some(tr!(editor_changed, name = name)),
            Vec::new(),
            vec![tr!(editor_overwrite), tr!(editor_reload), tr!(cancel)],
            keys,
        ),
    };
    let focus = match editor.prompt.as_ref()? {
        EditorPrompt::Find { focus, .. }
        | EditorPrompt::Replace { focus, .. }
        | EditorPrompt::Line { focus, .. }
        | EditorPrompt::Unsaved { focus }
        | EditorPrompt::Changed { focus } => *focus,
    };
    Some(EditorDialog { title, message, fields, buttons, focus, keys })
}

/// The status line's note, for the renderer.
pub fn status(editor: &Editor) -> Option<&str> {
    editor.note.as_deref()
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
        assert_eq!(state.editor.as_ref().unwrap().prompt, Some(EditorPrompt::Unsaved { focus: 0 }));
        let effects = press(&mut state, now, KeyCode::Enter.into());
        assert!(matches!(&effects[..], [Effect::SaveFile { force: false, .. }]));
        update(&mut state, Msg::EditorSaved(Err(SaveError::Changed)), now);
        assert_eq!(state.editor.as_ref().unwrap().prompt, Some(EditorPrompt::Changed { focus: 0 }));
        press(&mut state, now, KeyCode::Right.into());
        press(&mut state, now, KeyCode::Right.into());
        assert!(press(&mut state, now, KeyCode::Enter.into()).is_empty(), "Cancel keeps editing");
        assert!(state.editor.as_ref().unwrap().prompt.is_none());
        update(&mut state, Msg::EditorSaved(Err(SaveError::Changed)), now);
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
    fn an_edit_colours_the_text_as_colouring_it_afresh_would() {
        let (mut state, now) = state();
        let text: String = (0..60).map(|n| format!("let value_{n} = \"{n}\"; // line {n}\n")).collect();
        opened(&mut state, now, &text);
        let rows = usize::from(state.rows - EDITOR_CHROME);
        let colours = |editor: &Editor| {
            (0..editor.buffer.lines()).map(|line| editor.runs(line).map(<[_]>::to_vec)).collect::<Vec<_>>()
        };
        let afresh = |editor: &Editor| {
            let mut fresh =
                Editor::new(EditorFile { path: editor.path.clone(), text: editor.buffer.text(), stamp: stamp(0) });
            fresh.top = editor.top;
            fresh.paint(rows);
            colours(&fresh)
        };
        for _ in 0..3 {
            press(&mut state, now, KeyCode::Down.into());
        }
        // A comment opened above the rest changes the colour of every line below it.
        for key in ["/", "*", " "].map(|c| KeyCode::Char(c.chars().next().unwrap())).into_iter().chain([KeyCode::Enter])
        {
            press(&mut state, now, key.into());
            let editor = state.editor.as_ref().unwrap();
            assert_eq!(colours(editor), afresh(editor), "after {key:?}");
        }
        for key in [KeyCode::Backspace, KeyCode::Backspace, KeyCode::Backspace, KeyCode::Backspace, KeyCode::PageDown] {
            press(&mut state, now, key.into());
            let editor = state.editor.as_ref().unwrap();
            assert_eq!(colours(editor), afresh(editor), "after {key:?}");
        }
        press(&mut state, now, ctrl('z'));
        let editor = state.editor.as_ref().unwrap();
        assert_eq!(colours(editor), afresh(editor), "after an undo");
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
    fn closing_asks_in_a_dialog_whose_buttons_take_clicks() {
        let (mut state, now) = state();
        opened(&mut state, now, "a");
        press(&mut state, now, KeyCode::Char('b').into());
        press(&mut state, now, KeyCode::Esc.into());
        let dialog = dialog(state.editor.as_ref().unwrap()).unwrap();
        assert_eq!(dialog.buttons, ["Save", "Discard", "Cancel"]);
        assert_eq!(dialog.message.as_deref(), Some("Save the changes to notes.rs?"));
        update(&mut state, Msg::Pointer(Pointer::EditorControl(2)), now);
        assert!(state.editor.as_ref().is_some_and(|editor| editor.prompt.is_none()), "Cancel keeps editing");
        press(&mut state, now, KeyCode::Esc.into());
        let effects = update(&mut state, Msg::Pointer(Pointer::EditorControl(1)), now);
        assert!(state.editor.is_none() && effects.iter().any(|effect| matches!(effect, Effect::List { .. })));
    }

    #[test]
    fn replace_steps_through_matches_and_all_replaces_the_rest_in_one_undo() {
        let (mut state, now) = state();
        opened(&mut state, now, "cat Cat\ncat dog\n");
        press(&mut state, now, ctrl('r'));
        update(&mut state, Msg::Paste("cat".into()), now);
        press(&mut state, now, KeyCode::Tab.into());
        update(&mut state, Msg::Paste("cow".into()), now);
        press(&mut state, now, KeyCode::Enter.into());
        assert_eq!(state.editor.as_ref().unwrap().buffer.selection(), Some(0..3), "the first Enter finds a match");
        press(&mut state, now, KeyCode::Enter.into());
        let editor = state.editor.as_ref().unwrap();
        assert_eq!(editor.buffer.text(), "cow Cat\ncat dog\n");
        assert_eq!(editor.buffer.selection(), Some(4..7), "replacing moves on to the next match, in any case");
        assert!(matches!(editor.prompt, Some(EditorPrompt::Replace { .. })), "the dialog stays open");
        press(&mut state, now, KeyEvent::new(KeyCode::Char('a'), KeyModifiers::ALT));
        let editor = state.editor.as_ref().unwrap();
        assert_eq!(editor.buffer.text(), "cow cow\ncow dog\n");
        assert_eq!(editor.note.as_deref(), Some("Replaced 2"));
        assert!(editor.prompt.is_none());
        press(&mut state, now, ctrl('z'));
        assert_eq!(state.editor.as_ref().unwrap().buffer.text(), "cow Cat\ncat dog\n");
        press(&mut state, now, KeyCode::Right.into());
        press(&mut state, now, KeyCode::F(4).into());
        let Some(EditorPrompt::Replace { find, with, .. }) = &state.editor.as_ref().unwrap().prompt else { panic!() };
        assert_eq!((find.text(), with.text()), ("cat", "cow"), "the dialog remembers what it replaced");
    }

    #[test]
    fn clicks_place_the_cursor_drags_select_and_the_wheel_moves_the_view_only() {
        let (mut state, now) = state();
        opened(&mut state, now, &(1..=40).map(|n| format!("line {n}\n")).collect::<String>());
        let click = |line, column, select, word| Msg::Pointer(Pointer::EditorClick { line, column, select, word });
        update(&mut state, click(2, 3, false, false), now);
        assert_eq!(state.editor.as_ref().unwrap().buffer.position(), (2, 3));
        update(&mut state, click(3, 6, true, false), now);
        assert_eq!(state.editor.as_ref().unwrap().buffer.selected_text().as_deref(), Some("e 3\nline 4"));
        update(&mut state, click(5, 1, false, true), now);
        assert_eq!(state.editor.as_ref().unwrap().buffer.selected_text().as_deref(), Some("line"));
        update(&mut state, Msg::Pointer(Pointer::EditorScroll(20)), now);
        let editor = state.editor.as_ref().unwrap();
        assert_eq!((editor.top, editor.buffer.position()), (20, (5, 4)), "the cursor stays behind");
        update(
            &mut state,
            Msg::Pointer(Pointer::Panel { side: crate::Side::Left, index: 2, open: true, mark: false }),
            now,
        );
        assert!(state.editor.is_some(), "clicks never reach the panels behind the editor");
        press(&mut state, now, KeyCode::Right.into());
        assert!(state.editor.as_ref().unwrap().top <= 5, "a key brings the cursor back into view");
    }

    #[test]
    fn find_and_go_to_line_open_dialogs() {
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
