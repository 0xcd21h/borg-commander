//! Keys and clicks in the editor and its dialogs, and the edits they make.

use super::find::{note, on_bar, open_bar, replace, replace_all, replace_selection, step};
use super::{Editor, EditorPrompt, EditorSlot, holder, open, settle, text_size};
use crate::{AppState, Effect, LineInput, Pointer, Side, find_bar::Direction};
use borg_edit::Motion;
use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;

/// A key for the editor a panel holds. The panels' keys that edit nothing work here too: F6 goes
/// to the other panel, as it moves between parts in VS Code, Ctrl-O shows the console and F9
/// the menus.
pub(crate) fn on_panel_key(state: &mut AppState, side: Side, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    let editing = state.docked[side.index()].as_ref().is_some_and(|editor| editor.prompt.is_none());
    let action = match (key.code, key.modifiers) {
        (KeyCode::F(6), KeyModifiers::NONE) => Some(crate::Action::SwitchPanel),
        (KeyCode::Char('o'), KeyModifiers::CONTROL) => Some(crate::Action::ToggleShell),
        (KeyCode::F(9), KeyModifiers::NONE) => Some(crate::Action::OpenMenuBar),
        _ => None,
    };
    match action.filter(|_| editing) {
        Some(action) => crate::actions::perform(state, action, now),
        None => on_key(state, EditorSlot::Panel(side), key),
    }
}

pub(crate) fn on_key(state: &mut AppState, slot: EditorSlot, key: &KeyEvent) -> Vec<Effect> {
    let Some(mut editor) = holder(state, slot).take() else { return Vec::new() };
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

pub(super) fn on_prompt(
    state: &mut AppState,
    editor: &mut Editor,
    mut prompt: EditorPrompt,
    key: &KeyEvent,
) -> Vec<Effect> {
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
pub(super) fn press(state: &mut AppState, editor: &mut Editor, prompt: EditorPrompt, button: usize) -> Vec<Effect> {
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
        (EditorPrompt::Changed { .. }, 1) => return open(editor.path.clone(), editor.slot),
        _ => {}
    }
    Vec::new()
}

/// A click, drag or wheel turn in the editor, or a click on its dialog's field or button.
pub(crate) fn on_pointer(state: &mut AppState, slot: EditorSlot, pointer: Pointer) -> Vec<Effect> {
    let Some(mut editor) = holder(state, slot).take() else { return Vec::new() };
    let mut follow = true;
    let effects = match (pointer, editor.prompt.take()) {
        (Pointer::EditorControl(_, control), Some(mut prompt)) => {
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
        (Pointer::EditorClick { line, column, select, word, .. }, None) => {
            editor.note = None;
            if word {
                editor.buffer.select_word(line, column);
            } else {
                editor.buffer.place(line, column, select);
            }
            Vec::new()
        }
        (Pointer::EditorScroll(_, delta), prompt) => {
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
            *holder(state, slot) = Some(editor);
        }
    }
    effects
}

pub(super) fn on_edit(state: &mut AppState, editor: &mut Editor, key: &KeyEvent) -> Vec<Effect> {
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
pub(super) fn format(editor: &mut Editor) {
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
