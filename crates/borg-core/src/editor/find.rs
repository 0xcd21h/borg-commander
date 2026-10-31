//! Finding and replacing in the editor: the find bar, stepping from match to match, and replacing one or all.

use super::{Editor, EditorPrompt, on_edit};
use crate::{
    AppState, Effect, LineInput,
    find_bar::{BarKey, Direction, FindBar, FindNote},
};
use borg_edit::Buffer;
use borg_i18n::tr;
use borg_view::{Search, Source};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The editor's text as a search reads it.
pub(super) struct Text<'a>(&'a Buffer);

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
pub(super) fn on_bar(state: &mut AppState, editor: &mut Editor, key: &KeyEvent) -> Vec<Effect> {
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
pub(super) fn open_bar(editor: &mut Editor) {
    let seed = editor.buffer.selected_text().filter(|text| !text.contains('\n'));
    editor.origin = editor.buffer.selection().map_or(editor.buffer.cursor(), |range| range.start);
    editor.find.open(seed);
}

/// Selects the first match from where the find bar opened, as its query changes; with no
/// query the cursor goes back there.
pub(super) fn seek_from_origin(editor: &mut Editor) {
    if editor.find.search().is_none() {
        editor.buffer.select(editor.origin..editor.origin);
        return;
    }
    seek(editor, Direction::Forward, editor.origin);
}

/// Selects the next match after the cursor, or the previous one before the selection.
pub(super) fn step(editor: &mut Editor, direction: Direction) {
    let from = match direction {
        Direction::Forward => editor.buffer.cursor(),
        Direction::Backward => editor.buffer.selection().map_or(editor.buffer.cursor(), |range| range.start),
    };
    seek(editor, direction, from);
}

/// Selects the first match from character `from` on, or the last before it, going on from the
/// other end of the text when there is none that way, and counts where it stands.
pub(super) fn seek(editor: &mut Editor, direction: Direction, from: usize) {
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
pub(super) fn replacement(editor: &Editor, search: &Search, found: std::ops::Range<usize>) -> String {
    if !editor.find.regex {
        return editor.replacement.clone();
    }
    let buffer = &editor.buffer;
    let line = buffer.line_of(buffer.byte_to_char(found.start));
    let start = buffer.char_to_byte(buffer.line_start(line));
    search.replacement(buffer.line(line).as_bytes(), found.start - start..found.end - start, &editor.replacement)
}

/// Replaces the selection when it is a match.
pub(super) fn replace_selection(editor: &mut Editor) {
    let (Some(search), Some(range)) = (editor.find.search().cloned(), editor.buffer.selection()) else { return };
    let bytes = editor.buffer.char_to_byte(range.start)..editor.buffer.char_to_byte(range.end);
    let found = search.next(&Text(&editor.buffer), bytes.start as u64, &|_| true).ok().flatten();
    if found == Some(bytes.start as u64..bytes.end as u64) {
        let with = replacement(editor, &search, bytes);
        editor.buffer.insert(&with);
    }
}

/// Replaces every match in one step, returning how many there were.
pub(super) fn replace_all(editor: &mut Editor) -> usize {
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
pub(super) fn note(find: &FindBar) -> Option<String> {
    match &find.note {
        Some(FindNote::NoMatch) => Some(tr!(editor_no_match, text = find.input.text())),
        _ => crate::find_bar::summary(find),
    }
}

/// What to look for: a selection within one line, or else what was looked for last.
pub(super) fn needle(editor: &Editor) -> String {
    editor.buffer.selected_text().filter(|text| !text.contains('\n')).unwrap_or_else(|| editor.find.input.text().into())
}

pub(super) fn replace(editor: &mut Editor) -> EditorPrompt {
    let needle = needle(editor);
    editor.find.set_text(&needle);
    EditorPrompt::Replace { find: LineInput::new(needle), with: LineInput::new(editor.replacement.as_str()), focus: 0 }
}
