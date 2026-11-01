//! The built-in editor: a full-screen text editor over `borg_edit::Buffer`. Files are read and
//! written by the app; the editor keeps the stamp the file had when read, so a save notices a
//! file that changed on disk in the meantime.

use borg_i18n::tr;
use std::{path::PathBuf, time::SystemTime};

use borg_edit::Buffer;
use borg_view::Query;
use crossterm::event::KeyCode;

use crate::{AppState, Aside, Effect, LineInput, Side, View, find_bar::FindBar, viewer::Landing};

mod find;
mod keys;

use find::{land, seek_from_origin};
use keys::on_edit;
pub(crate) use keys::{on_key, on_panel_key, on_pointer};

/// Lines above the screen highlighted with it, so a comment or string opened there colours
/// what shows.
const PAINT_CONTEXT: usize = 300;
/// Lines longer than this, such as minified code, are left uncoloured, as VS Code leaves them.
const PAINT_LINE_LIMIT: usize = 20_000;
/// What a line left uncoloured is kept as, which no line's text can equal.
const UNPAINTED: &str = "\n";
/// Rows the editor's title and status lines take from the screen.
pub const EDITOR_CHROME: u16 = 2;

/// Where an editor shows: over the whole screen, or in a panel in place of its files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorSlot {
    Screen,
    Panel(Side),
}

/// What a panel editor opened beside results gives back when it closes: the focus goes to the
/// results, and the preview they showed comes back, read again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Beside {
    pub aside: Option<Aside>,
}

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
    pub slot: EditorSlot,
    pub(crate) beside: Option<Beside>,
}

impl Editor {
    fn new(file: EditorFile, slot: EditorSlot) -> Self {
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
            slot,
            beside: None,
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
            slot: self.slot,
        }]
    }

    /// Closes the editor from one of its own keys or clicks.
    fn close(&mut self, state: &mut AppState) -> Vec<Effect> {
        self.closed = true;
        close_editor(state, self.slot, self.beside)
    }

    fn name(&self) -> String {
        self.path.file_name().unwrap_or(self.path.as_os_str()).to_string_lossy().into_owned()
    }
}

/// Where the editor in `slot` holds its editor.
fn holder(state: &mut AppState, slot: EditorSlot) -> &mut Option<Editor> {
    match slot {
        EditorSlot::Screen => &mut state.editor,
        EditorSlot::Panel(side) => &mut state.docked[side.index()],
    }
}

/// The editor that takes keys: the one over the screen, or the active panel's while the panels
/// show and nothing covers them.
pub fn focused(state: &AppState) -> Option<EditorSlot> {
    if state.editor.is_some() {
        return Some(EditorSlot::Screen);
    }
    let shown = state.effective_view() == View::Panels
        && state.viewer.is_none()
        && state.activity.side.is_none()
        && !state.docker.is_open();
    (shown && state.docked[state.active.index()].is_some()).then_some(EditorSlot::Panel(state.active))
}

/// The text area's size: the screen's, or a panel's, less the title, status and gutter.
fn text_size(state: &AppState, editor: &Editor) -> (usize, usize) {
    let (cols, rows) = match editor.slot {
        EditorSlot::Screen => (state.cols, state.rows),
        EditorSlot::Panel(side) => {
            let layout = crate::Layout::new(state.cols, state.rows, state.chrome(), state.split_target());
            let area = layout.panels[side.index()];
            (area.width, area.height)
        }
    };
    (usize::from(rows.saturating_sub(EDITOR_CHROME)), usize::from(cols.saturating_sub(editor.gutter())))
}

pub(crate) fn open(state: &mut AppState, path: PathBuf, slot: EditorSlot) -> Vec<Effect> {
    state.landing = None;
    vec![Effect::OpenEditor { path, slot }]
}

/// Opens `path` with the match a contents search found at `offset` selected, the find bar
/// ready with `query` to step on through the others. A file the editor leaves to the viewer
/// opens there on the match.
pub(crate) fn open_at(state: &mut AppState, path: PathBuf, slot: EditorSlot, query: Query, offset: u64) -> Vec<Effect> {
    state.landing = Some(Landing { path: path.clone(), query, offset });
    vec![Effect::OpenEditor { path, slot }]
}

/// Readies the panel on `side` for an editor beside the results in the other panel. An editor it
/// holds with unsaved changes takes the focus and asks whether to save them, and `false` says so.
pub(crate) fn ready_beside(state: &mut AppState, side: Side) -> bool {
    if let Some(editor) = state.docked[side.index()].as_mut().filter(|editor| editor.buffer.modified()) {
        editor.prompt = Some(EditorPrompt::Unsaved { focus: 0 });
        state.active = side;
        return false;
    }
    let aside = match &state.docked[side.index()] {
        Some(editor) => editor.beside.and_then(|beside| beside.aside),
        None => state.aside,
    };
    state.beside = Some((side, Beside { aside }));
    true
}

pub(crate) fn opened(state: &mut AppState, slot: EditorSlot, result: Result<EditorFile, OpenError>) -> Vec<Effect> {
    let beside = state.beside.take_if(|(side, _)| slot == EditorSlot::Panel(*side)).map(|(_, beside)| beside);
    match result {
        Ok(file) => {
            let landing = state.landing.take_if(|landing| landing.path == file.path);
            let mut editor = Editor::new(file, slot);
            let reread = holder(state, slot).as_ref().and_then(|held| held.beside);
            editor.beside = beside.or(reread);
            let (rows, columns) = text_size(state, &editor);
            if let Some(landing) = landing {
                land(&mut editor, &landing, rows);
            }
            editor.follow(rows, columns);
            editor.paint(rows);
            if let EditorSlot::Panel(side) = slot {
                // The panel's files give way to the editor, and previews to both.
                state.active = side;
                state.aside = None;
                state.preview_focused = false;
            }
            *holder(state, slot) = Some(editor);
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
            state.landing.take_if(|landing| landing.path == path);
            state.notice = Some(tr!(editor_open_failed, name = display_name(&path), error = error));
            Vec::new()
        }
    }
}

fn display_name(path: &std::path::Path) -> String {
    path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned()
}

pub(crate) fn saved(state: &mut AppState, slot: EditorSlot, result: Result<FileStamp, SaveError>) -> Vec<Effect> {
    let Some(editor) = holder(state, slot).as_mut() else { return Vec::new() };
    let Some((revision, close)) = editor.saving.take() else { return Vec::new() };
    match result {
        Ok(stamp) => {
            editor.stamp = stamp;
            if editor.buffer.revision() == revision {
                editor.buffer.mark_saved();
            }
            editor.note = Some(tr!(editor_saved).into());
            if close {
                let beside = editor.beside;
                return close_editor(state, slot, beside);
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

fn close_editor(state: &mut AppState, slot: EditorSlot, beside: Option<Beside>) -> Vec<Effect> {
    *holder(state, slot) = None;
    let side = match slot {
        EditorSlot::Screen => state.active,
        EditorSlot::Panel(side) => side,
    };
    if let Some(Beside { aside }) = beside {
        state.active = side.other();
        state.aside = aside;
        state.peeked = None;
        state.peeking = None;
    }
    vec![state.reload(side)]
}

pub(crate) fn paste(state: &mut AppState, slot: EditorSlot, text: &str) -> Vec<Effect> {
    let Some(mut editor) = holder(state, slot).take() else { return Vec::new() };
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
    let slot = editor.slot;
    *holder(state, slot) = Some(editor);
}

/// Fits every editor to the room it has now, after the screen or the panels changed size.
pub(crate) fn resize(state: &mut AppState) {
    let slots = [EditorSlot::Screen, EditorSlot::Panel(Side::Left), EditorSlot::Panel(Side::Right)];
    for slot in slots {
        if let Some(editor) = holder(state, slot).take() {
            settle(state, editor);
        }
    }
}

/// Colours what the panel editors show after the panels changed height, leaving their view where
/// it is, as a wheel turn left it.
pub(crate) fn repaint(state: &mut AppState) {
    for side in Side::ALL {
        if let Some(mut editor) = state.docked[side.index()].take() {
            let (rows, _) = text_size(state, &editor);
            editor.paint(rows);
            state.docked[side.index()] = Some(editor);
        }
    }
}

/// The first panel editor with unsaved changes, made active and asking whether to save them.
pub(crate) fn ask_unsaved(state: &mut AppState) -> bool {
    let Some(side) = Side::ALL
        .into_iter()
        .find(|side| state.docked[side.index()].as_ref().is_some_and(|editor| editor.buffer.modified()))
    else {
        return false;
    };
    state.active = side;
    if let Some(editor) = state.docked[side.index()].as_mut() {
        editor.prompt = Some(EditorPrompt::Unsaved { focus: 0 });
    }
    true
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
mod tests;
