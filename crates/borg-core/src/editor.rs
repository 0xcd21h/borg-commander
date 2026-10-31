//! The built-in editor: a full-screen text editor over `borg_edit::Buffer`. Files are read and
//! written by the app; the editor keeps the stamp the file had when read, so a save notices a
//! file that changed on disk in the meantime.

use borg_i18n::tr;
use std::{path::PathBuf, time::SystemTime};

use borg_edit::{Buffer, Motion};
use borg_view::{Search, Source};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    AppState, Effect, LineInput, Pointer,
    find_bar::{BarKey, Direction, FindBar, FindNote},
};

/// Lines above the screen highlighted with it, so a comment or string opened there colours
/// what shows.
const PAINT_CONTEXT: usize = 300;
/// Lines longer than this, such as minified code, are left uncoloured, as VS Code leaves them.
const PAINT_LINE_LIMIT: usize = 20_000;
/// What a line left uncoloured is kept as, which no line's text can equal.
const UNPAINTED: &str = "\n";
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
    fn focus_mut(&mut self) -> &mut usize {
        match self {
            Self::Replace { focus, .. }
            | Self::Line { focus, .. }
            | Self::Unsaved { focus }
            | Self::Changed { focus } => focus,
        }
    }

    fn fields(&self) -> usize {
        match self {
            Self::Replace { .. } => 2,
            Self::Line { .. } => 1,
            Self::Unsaved { .. } | Self::Changed { .. } => 0,
        }
    }

    fn buttons(&self) -> usize {
        if matches!(self, Self::Replace { .. }) {
            4
        } else if matches!(self, Self::Line { .. }) {
            2
        } else {
            3
        }
    }

    fn field_mut(&mut self) -> Option<&mut LineInput> {
        match self {
            Self::Line { input, focus: 0 } => Some(input),
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
    pub find: FindBar,
    /// Where the cursor was when the find bar opened, which a changed query searches on from.
    origin: usize,
    replacement: String,
    /// The revision a save in flight wrote, and whether the editor closes once it lands.
    saving: Option<(u64, bool)>,
    /// Set when one of the editor's keys or clicks closed it, so it is not put back.
    closed: bool,
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
            find: FindBar::default(),
            origin: 0,
            replacement: String::new(),
            saving: None,
            closed: false,
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

    /// The matches of the find bar's query in the line `text`, by character, while the bar or
    /// the replace dialog shows.
    pub fn matches_on(&self, text: &str) -> Vec<std::ops::Range<usize>> {
        let shown = self.find.open || matches!(self.prompt, Some(EditorPrompt::Replace { .. }));
        let Some(search) = self.find.search().filter(|_| shown) else { return Vec::new() };
        let (mut byte, mut char) = (0, 0);
        let mut to_char = |at: usize| {
            char += text[byte..at].chars().count();
            byte = at;
            char
        };
        search.in_line(text.as_bytes()).into_iter().map(|range| to_char(range.start)..to_char(range.end)).collect()
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
            let long = self.buffer.line_len(line) > PAINT_LINE_LIMIT;
            let text = if long { UNPAINTED.to_owned() } else { self.buffer.line(line) };
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
                // Colouring goes on below a long line as if it were not there.
                _ if long => painted.lines.push(Vec::new()),
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

    /// Closes the editor from one of its own keys or clicks.
    fn close(&mut self, state: &mut AppState) -> Vec<Effect> {
        self.closed = true;
        close_editor(state)
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
        None if editor.find.open => {
            editor.find.paste(text);
            seek_from_origin(&mut editor);
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
        None if editor.find.open => on_bar(state, &mut editor, key),
        None => on_edit(state, &mut editor, key),
    };
    // A key that closed the editor leaves it closed; otherwise it stays, also while its file is read again.
    if !editor.closed {
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
        (EditorPrompt::Replace { .. }, _) if editor.find.switch(key) => None,
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
            if let EditorPrompt::Replace { find, .. } = &prompt {
                editor.find.set_text(find.text());
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
        (EditorPrompt::Replace { find: needle, with, focus }, button @ (0 | 2)) => {
            editor.find.set_text(needle.text());
            editor.replacement = with.text().to_owned();
            if button == 0 {
                replace_selection(editor);
            }
            step(editor, Direction::Forward);
            editor.note = note(&editor.find);
            editor.prompt = Some(EditorPrompt::Replace { find: needle, with, focus });
        }
        (EditorPrompt::Replace { find: needle, with, .. }, 1) => {
            editor.find.set_text(needle.text());
            editor.find.remember(&mut state.find_recent);
            editor.replacement = with.text().to_owned();
            let count = replace_all(editor);
            editor.note = Some(if count == 0 {
                tr!(editor_no_match, text = needle.text())
            } else {
                tr!(editor_replaced, count = count)
            });
        }
        (EditorPrompt::Line { input, focus }, 0) => match input.text().trim().parse::<usize>() {
            Ok(line) => editor.buffer.go_to_line(line),
            Err(_) => editor.prompt = Some(EditorPrompt::Line { input, focus }),
        },
        (EditorPrompt::Unsaved { .. }, 0) => return editor.save(true, false),
        (EditorPrompt::Unsaved { .. }, 1) => return editor.close(state),
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
    if !editor.closed {
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

/// The editor's text as a search reads it.
struct Text<'a>(&'a Buffer);

impl Source for Text<'_> {
    fn len(&self) -> u64 {
        self.0.len_bytes() as u64
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> std::io::Result<usize> {
        Ok(self.0.read_at(usize::try_from(offset).unwrap_or(usize::MAX), buf))
    }
}

/// A key in the find bar. Keys the bar does not take that are no text, such as Ctrl-S, F2 or
/// Page Down, work on the text as they do with the bar closed.
fn on_bar(state: &mut AppState, editor: &mut Editor, key: &KeyEvent) -> Vec<Effect> {
    match editor.find.on_key(key, &state.find_recent) {
        BarKey::Step(direction) => {
            editor.find.remember(&mut state.find_recent);
            step(editor, direction);
        }
        BarKey::Changed => seek_from_origin(editor),
        BarKey::Closed => editor.find.remember(&mut state.find_recent),
        BarKey::Kept => {}
        BarKey::Unhandled => {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            if ctrl || matches!(key.code, KeyCode::F(_) | KeyCode::PageUp | KeyCode::PageDown) {
                return on_edit(state, editor, key);
            }
        }
    }
    Vec::new()
}

/// Opens the find bar on the selection, when it is within a line, or on the query it had.
fn open_bar(editor: &mut Editor) {
    let seed = editor.buffer.selected_text().filter(|text| !text.contains('\n'));
    editor.origin = editor.buffer.selection().map_or(editor.buffer.cursor(), |range| range.start);
    editor.find.open(seed);
}

/// Selects the first match from where the find bar opened, as its query changes; with no
/// query the cursor goes back there.
fn seek_from_origin(editor: &mut Editor) {
    if editor.find.search().is_none() {
        editor.buffer.select(editor.origin..editor.origin);
        return;
    }
    seek(editor, Direction::Forward, editor.origin);
}

/// Selects the next match after the cursor, or the previous one before the selection.
fn step(editor: &mut Editor, direction: Direction) {
    let from = match direction {
        Direction::Forward => editor.buffer.cursor(),
        Direction::Backward => editor.buffer.selection().map_or(editor.buffer.cursor(), |range| range.start),
    };
    seek(editor, direction, from);
}

/// Selects the first match from character `from` on, or the last before it, going on from the
/// other end of the text when there is none that way, and counts where it stands.
fn seek(editor: &mut Editor, direction: Direction, from: usize) {
    let Some(search) = editor.find.search().cloned() else { return };
    let text = Text(&editor.buffer);
    let at = editor.buffer.char_to_byte(from) as u64;
    let go_on = |_| true;
    let look = |at: u64| match direction {
        Direction::Forward => search.next(&text, at, &go_on),
        Direction::Backward => search.previous(&text, at, &go_on),
    };
    let wrap_from = if direction == Direction::Forward { 0 } else { text.len() };
    let (found, wrapped) = match look(at).ok().flatten() {
        Some(found) => (Some(found), false),
        None => (look(wrap_from).ok().flatten(), true),
    };
    let Some(found) = found else {
        editor.find.note = Some(FindNote::NoMatch);
        editor.find.count = None;
        return;
    };
    editor.find.count = search.count(&text, found.start, &go_on).ok();
    editor.find.note = wrapped.then_some(FindNote::Wrapped);
    let start = editor.buffer.byte_to_char(found.start as usize);
    let end = editor.buffer.byte_to_char(found.end as usize);
    editor.buffer.select(start..end);
}

/// What `with` becomes for the match at bytes `found`: with a regular expression's groups,
/// such as `$1`, filled in.
fn replacement(editor: &Editor, search: &Search, found: std::ops::Range<usize>) -> String {
    if !editor.find.regex {
        return editor.replacement.clone();
    }
    let buffer = &editor.buffer;
    let line = buffer.line_of(buffer.byte_to_char(found.start));
    let start = buffer.char_to_byte(buffer.line_start(line));
    search.replacement(buffer.line(line).as_bytes(), found.start - start..found.end - start, &editor.replacement)
}

/// Replaces the selection when it is a match.
fn replace_selection(editor: &mut Editor) {
    let (Some(search), Some(range)) = (editor.find.search().cloned(), editor.buffer.selection()) else { return };
    let bytes = editor.buffer.char_to_byte(range.start)..editor.buffer.char_to_byte(range.end);
    let found = search.next(&Text(&editor.buffer), bytes.start as u64, &|_| true).ok().flatten();
    if found == Some(bytes.start as u64..bytes.end as u64) {
        let with = replacement(editor, &search, bytes);
        editor.buffer.insert(&with);
    }
}

/// Replaces every match in one step, returning how many there were.
fn replace_all(editor: &mut Editor) -> usize {
    let Some(search) = editor.find.search().cloned() else { return 0 };
    let mut found = Vec::new();
    let _ = search.each_match(&Text(&editor.buffer), 0, &|_| true, &mut |range| {
        found.push(range.start as usize..range.end as usize);
        true
    });
    let edits: Vec<_> = found
        .into_iter()
        .map(|bytes| {
            let with = replacement(editor, &search, bytes.clone());
            (editor.buffer.byte_to_char(bytes.start)..editor.buffer.byte_to_char(bytes.end), with)
        })
        .collect();
    editor.buffer.replace_ranges(&edits)
}

/// The status line's note after a step with the find bar closed.
fn note(find: &FindBar) -> Option<String> {
    match &find.note {
        Some(FindNote::NoMatch) => Some(tr!(editor_no_match, text = find.input.text())),
        _ => crate::find_bar::summary(find),
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
                return editor.close(state);
            }
        }
        KeyCode::F(2) => return editor.save(false, false),
        KeyCode::Char('s') if ctrl => return editor.save(false, false),
        KeyCode::Char('f') if ctrl => open_bar(editor),
        KeyCode::F(3 | 7) if editor.find.input.text().is_empty() => open_bar(editor),
        KeyCode::F(3) => {
            step(editor, if shift { Direction::Backward } else { Direction::Forward });
            editor.note = note(&editor.find);
        }
        KeyCode::F(7) if shift || alt => {
            step(editor, if alt { Direction::Backward } else { Direction::Forward });
            editor.note = note(&editor.find);
        }
        KeyCode::F(7) => open_bar(editor),
        KeyCode::Char('r') if ctrl => editor.prompt = Some(replace(editor)),
        KeyCode::F(4) => editor.prompt = Some(replace(editor)),
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
        KeyCode::Char('f' | 'F') if alt && shift => format(editor),
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

/// Lays out JSON one value to a line with the file's indentation, in one step that undoes at
/// once, as VS Code's Shift-Alt-F does. The cursor stays at the character it was at.
fn format(editor: &mut Editor) {
    if editor.language.as_deref().is_some_and(|language| !language.starts_with("JSON")) {
        editor.note = Some(tr!(editor_format_json_only).into());
        return;
    }
    let buffer = &mut editor.buffer;
    let text = buffer.slice(0..buffer.len_chars());
    let laid_out = match borg_view::pretty::json(&text, &buffer.indent.unit(), &|| true) {
        Some(Ok(laid_out)) => laid_out,
        Some(Err(reason)) => {
            editor.note = Some(tr!(editor_not_json, reason = reason));
            return;
        }
        None => return,
    };
    // Laying out changes only the spaces between values, so the cursor is found again by the
    // characters other than spaces before it.
    let before = text.chars().take(buffer.cursor()).filter(|c| !c.is_whitespace()).count();
    let mut shown = laid_out.chars().enumerate().filter(|(_, c)| !c.is_whitespace());
    let cursor = shown.nth(before).map_or_else(|| laid_out.chars().count(), |(at, _)| at);
    let all = 0..buffer.len_chars();
    buffer.replace_ranges(&[(all, laid_out)]);
    buffer.select(cursor..cursor);
    editor.note = Some(tr!(editor_formatted).into());
}

/// What to look for: a selection within one line, or else what was looked for last.
fn needle(editor: &Editor) -> String {
    editor.buffer.selected_text().filter(|text| !text.contains('\n')).unwrap_or_else(|| editor.find.input.text().into())
}

fn replace(editor: &mut Editor) -> EditorPrompt {
    let needle = needle(editor);
    editor.find.set_text(&needle);
    EditorPrompt::Replace { find: LineInput::new(needle), with: LineInput::new(editor.replacement.as_str()), focus: 0 }
}

/// The dialog open over the editor, for the renderer.
pub fn dialog(editor: &Editor) -> Option<EditorDialog<'_>> {
    let name = editor.name();
    let keys = tr!(editor_dialog_keys);
    let (title, message, fields, buttons, keys) = match editor.prompt.as_ref()? {
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
        EditorPrompt::Replace { focus, .. }
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
    fn the_editor_closes_over_search_results_and_collections() {
        let (mut state, now) = state();
        let side = state.active;
        state.panel_mut(side).open_results(crate::ResultSource::Collection("notes".into()), Vec::new(), 1);
        opened(&mut state, now, "a");
        let effects = press(&mut state, now, KeyCode::Esc.into());
        assert!(state.editor.is_none(), "Esc closes an unchanged file: {effects:?}");
        assert!(matches!(&effects[..], [Effect::RefreshCollection { .. }]), "{effects:?}");

        opened(&mut state, now, "a");
        press(&mut state, now, KeyCode::Char('b').into());
        press(&mut state, now, KeyCode::Esc.into());
        press(&mut state, now, KeyCode::Right.into());
        press(&mut state, now, KeyCode::Enter.into());
        assert!(state.editor.is_none(), "Discard closes the editor");

        opened(&mut state, now, "a");
        press(&mut state, now, KeyCode::Char('b').into());
        press(&mut state, now, KeyCode::Esc.into());
        update(&mut state, Msg::Pointer(Pointer::EditorControl(1)), now);
        assert!(state.editor.is_none(), "a click on Discard closes it too");
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
    fn a_line_as_long_as_minified_code_is_left_uncoloured_and_the_rest_colours_as_without_it() {
        let (mut other, _) = state();
        let (mut state, now) = state();
        opened(&mut other, now, "/* open\n\nclosed */ let value = 1;\n");
        let without = other.editor.as_ref().unwrap().runs(2).map(<[_]>::to_vec);
        let long = format!("let data = \"{}\";", "x".repeat(PAINT_LINE_LIMIT));
        opened(&mut state, now, &format!("/* open\n{long}\nclosed */ let value = 1;\n"));
        let editor = state.editor.as_ref().unwrap();
        assert_eq!(editor.runs(1), Some(&[][..]));
        assert_eq!(editor.runs(2).map(<[_]>::to_vec), without, "the comment opened above it still closes below");
    }

    #[test]
    fn alt_shift_f_lays_out_json_in_one_undo_with_the_cursor_on_its_character() {
        let (mut state, now) = state();
        let format = KeyEvent::new(KeyCode::Char('F'), KeyModifiers::ALT | KeyModifiers::SHIFT);
        let open = |state: &mut AppState, path: &str, text: &str| {
            let file = EditorFile { path: path.into(), text: text.into(), stamp: stamp(text.len() as u64) };
            update(state, Msg::EditorOpened(Ok(file)), now);
        };
        let text = r#"{"name":"Ada","tags":["x","y"]}"#;
        open(&mut state, "/work/data.json", text);
        state.editor.as_mut().unwrap().buffer.place(0, 23, false);
        press(&mut state, now, format);
        let editor = state.editor.as_ref().unwrap();
        assert_eq!(
            editor.buffer.text(),
            "{\n    \"name\": \"Ada\",\n    \"tags\": [\n        \"x\",\n        \"y\"\n    ]\n}"
        );
        assert_eq!(editor.buffer.position(), (3, 9), "still before the x");
        assert_eq!(editor.note.as_deref(), Some("Formatted as JSON"));
        press(&mut state, now, ctrl('z'));
        assert_eq!(state.editor.as_ref().unwrap().buffer.text(), text, "one undo puts it back");

        open(&mut state, "/work/broken.json", r#"{"a":"#);
        press(&mut state, now, format);
        let note = state.editor.as_ref().unwrap().note.clone();
        assert_eq!(note.as_deref(), Some("Not JSON: EOF while parsing a value at line 1 column 5"));
        opened(&mut state, now, "fn main() {}\n");
        press(&mut state, now, format);
        assert_eq!(state.editor.as_ref().unwrap().note.as_deref(), Some("Only JSON is formatted"));
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
    fn go_to_line_and_the_find_bar_search_on_from_the_cursor() {
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
        let editor = state.editor.as_ref().unwrap();
        assert_eq!(editor.buffer.selection(), Some(303..309), "typing finds line 40, the first after line 30");
        assert_eq!(crate::find_bar::summary(&editor.find).as_deref(), Some("2 of 2"));
        assert_eq!(editor.matches_on("line 4 and line 40"), [0..6, 11..17], "the bar marks every match");
        press(&mut state, now, KeyCode::Enter.into());
        let editor = state.editor.as_ref().unwrap();
        assert_eq!(editor.buffer.position(), (3, 6), "Enter goes on past the end to line 4");
        assert_eq!(crate::find_bar::summary(&editor.find).as_deref(), Some("1 of 2 · from the other end"));
        press(&mut state, now, KeyCode::Up.into());
        assert_eq!(state.editor.as_ref().unwrap().buffer.position(), (39, 6), "Up goes back");
        for _ in 0.."line 4".len() {
            press(&mut state, now, KeyCode::Backspace.into());
        }
        let editor = state.editor.as_ref().unwrap();
        assert_eq!((editor.buffer.position(), editor.buffer.selection()), ((29, 0), None), "no query, no move");
        update(&mut state, Msg::Paste("line 9".into()), now);
        press(&mut state, now, KeyCode::Esc.into());
        let editor = state.editor.as_ref().unwrap();
        assert!(!editor.find.open && editor.matches_on("line 9").is_empty(), "Esc takes the marks away");
        assert_eq!(editor.buffer.selected_text().as_deref(), Some("line 9"), "and keeps the match selected");
        assert_eq!(state.find_recent, ["line 9", "line 4"], "queries stepped through or closed on are kept");
        press(&mut state, now, KeyEvent::new(KeyCode::F(3), KeyModifiers::SHIFT));
        let editor = state.editor.as_ref().unwrap();
        assert_eq!(editor.buffer.position(), (8, 6), "Shift-F3 steps back with the bar closed");
        assert_eq!(editor.note.as_deref(), Some("1 of 1 · from the other end"));
        press(&mut state, now, ctrl('f'));
        press(&mut state, now, KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        let editor = state.editor.as_ref().unwrap();
        assert_eq!(editor.note, None);
        assert_eq!(crate::find_bar::summary(&editor.find).as_deref(), Some("No matches"));
    }

    #[test]
    fn whole_words_and_regular_expressions_find_and_replace_with_groups() {
        let (mut state, now) = state();
        opened(&mut state, now, "cat concat cat\nlet cat = 1;\n");
        press(&mut state, now, ctrl('f'));
        update(&mut state, Msg::Paste("cat".into()), now);
        assert_eq!(state.editor.as_ref().unwrap().find.count.map(|count| count.total), Some(4));
        press(&mut state, now, KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT));
        assert_eq!(state.editor.as_ref().unwrap().find.count.map(|count| count.total), Some(3), "concat is no word");
        press(&mut state, now, KeyCode::Esc.into());
        press(&mut state, now, ctrl('r'));
        for _ in 0..3 {
            press(&mut state, now, KeyCode::Backspace.into());
        }
        update(&mut state, Msg::Paste(r"(\w+) = (\d)".into()), now);
        press(&mut state, now, KeyEvent::new(KeyCode::Char('r'), KeyModifiers::ALT));
        assert!(state.editor.as_ref().unwrap().find.regex && state.editor.as_ref().unwrap().find.words);
        press(&mut state, now, KeyCode::Tab.into());
        update(&mut state, Msg::Paste("$2 = $1".into()), now);
        press(&mut state, now, KeyEvent::new(KeyCode::Char('a'), KeyModifiers::ALT));
        let editor = state.editor.as_ref().unwrap();
        assert_eq!(editor.buffer.text(), "cat concat cat\nlet 1 = cat;\n");
        assert_eq!(editor.note.as_deref(), Some("Replaced 1"));
        press(&mut state, now, ctrl('z'));
        assert_eq!(state.editor.as_ref().unwrap().buffer.text(), "cat concat cat\nlet cat = 1;\n");
    }
}
