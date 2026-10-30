//! What searches leave out, edited from the search dialog and saved in the configuration.

use crate::{AppState, Effect, LineInput, Overlay, SearchForm};
use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent};

/// The rows above the patterns: generated folders and ignore files.
pub const EXCLUSION_SWITCHES: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExclusionEditor {
    /// The search dialog to return to.
    pub search: Box<SearchForm>,
    /// The row chosen: the switches, each pattern, then a row for adding one.
    pub selected: usize,
    /// The pattern being typed, and the one it replaces; `None` adds it.
    pub editing: Option<(Option<usize>, LineInput)>,
    pub error: Option<String>,
}

impl ExclusionEditor {
    pub(crate) fn new(search: SearchForm) -> Self {
        Self { search: Box::new(search), selected: 0, editing: None, error: None }
    }

    /// The rows: the switches, the patterns and the row for adding one.
    pub fn rows(state: &AppState) -> usize {
        EXCLUSION_SWITCHES + state.settings.search.exclude.len() + 1
    }

    /// Acts on `row` as Enter would: flips a switch, edits a pattern or starts a new one.
    pub(crate) fn activate(&mut self, state: &mut AppState, row: usize) -> Vec<Effect> {
        self.selected = row.min(Self::rows(state) - 1);
        self.error = None;
        let search = &mut state.settings.search;
        match self.selected {
            0 => search.artifacts = !search.artifacts,
            1 => search.ignore_files = !search.ignore_files,
            row => {
                let at = row - EXCLUSION_SWITCHES;
                let text = search.exclude.get(at).cloned().unwrap_or_default();
                self.editing = Some((search.exclude.get(at).map(|_| at), LineInput::new(text)));
                return Vec::new();
            }
        }
        vec![Effect::SaveConfig]
    }

    /// Keeps the pattern typed, or takes it out when it was emptied.
    fn commit(&mut self, state: &mut AppState) -> Vec<Effect> {
        let Some((at, input)) = self.editing.take() else { return Vec::new() };
        let glob = input.text().trim().to_owned();
        let exclude = &mut state.settings.search.exclude;
        if glob.is_empty() {
            let Some(at) = at else { return Vec::new() };
            exclude.remove(at);
            self.selected = self.selected.min(Self::rows(state) - 1);
            return vec![Effect::SaveConfig];
        }
        if let Err(error) = check(&glob) {
            self.error = Some(error);
            self.editing = Some((at, input));
            return Vec::new();
        }
        if let Some(existing) = exclude.iter().position(|past| *past == glob) {
            self.selected = EXCLUSION_SWITCHES + existing;
            return Vec::new();
        }
        let at = match at {
            Some(at) => {
                exclude[at] = glob;
                at
            }
            None => {
                exclude.push(glob);
                exclude.len() - 1
            }
        };
        self.selected = EXCLUSION_SWITCHES + at;
        vec![Effect::SaveConfig]
    }
}

/// Whether `glob` reads as a pattern: a name, a folder with a trailing `/`, or a path from
/// the search folder with a leading one.
fn check(glob: &str) -> Result<(), String> {
    globset::Glob::new(glob.trim_start_matches('/').trim_end_matches('/'))
        .map(drop)
        .map_err(|error| tr!(exclusion_not_valid, error.kind()))
}

pub(crate) fn open(state: &mut AppState, form: SearchForm) -> Vec<Effect> {
    state.overlay = Some(Overlay::Exclusions(ExclusionEditor::new(form)));
    Vec::new()
}

pub(crate) fn on_key(state: &mut AppState, mut editor: ExclusionEditor, key: &KeyEvent) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some((_, input)) = editor.editing.as_mut() {
        match key.code {
            KeyCode::Esc => {
                editor.editing = None;
                editor.error = None;
            }
            KeyCode::Enter => effects = editor.commit(state),
            _ => {
                input.apply(key);
                editor.error = None;
            }
        }
        state.overlay = Some(Overlay::Exclusions(editor));
        return effects;
    }
    let rows = ExclusionEditor::rows(state);
    let pattern = editor.selected.checked_sub(EXCLUSION_SWITCHES).filter(|at| *at < rows - 1 - EXCLUSION_SWITCHES);
    match key.code {
        KeyCode::Esc | KeyCode::F(4) | KeyCode::F(10) => {
            state.overlay = Some(Overlay::Search(*editor.search));
            return Vec::new();
        }
        KeyCode::Up | KeyCode::BackTab => editor.selected = (editor.selected + rows - 1) % rows,
        KeyCode::Down | KeyCode::Tab => editor.selected = (editor.selected + 1) % rows,
        KeyCode::Home => editor.selected = 0,
        KeyCode::End => editor.selected = rows - 1,
        KeyCode::Enter | KeyCode::Char(' ') => effects = editor.activate(state, editor.selected),
        KeyCode::Insert | KeyCode::F(7) | KeyCode::Char('+') => effects = editor.activate(state, rows - 1),
        KeyCode::Delete | KeyCode::F(8) | KeyCode::Char('-') => {
            if let Some(at) = pattern {
                state.settings.search.exclude.remove(at);
                editor.selected = editor.selected.min(rows - 2);
                effects.push(Effect::SaveConfig);
            }
        }
        _ => {}
    }
    state.overlay = Some(Overlay::Exclusions(editor));
    effects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn press(state: &mut AppState, code: KeyCode) -> Vec<Effect> {
        let Some(Overlay::Exclusions(editor)) = state.overlay.clone() else { panic!("the list is open") };
        on_key(state, editor, &KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn typed(state: &mut AppState, text: &str) {
        for c in text.chars() {
            press(state, KeyCode::Char(c));
        }
    }

    #[test]
    fn patterns_are_added_edited_checked_and_removed_and_each_change_is_saved() {
        let now = std::time::Instant::now();
        let (mut state, _) = AppState::new(
            crate::Startup::at("/work".into()),
            100,
            30,
            crate::Settings::default(),
            crate::Appearance::new(vec![crate::ThemeChoice { id: "midnight".into(), name: "Midnight".into() }], 0),
            now,
        );
        state.settings.search.exclude = vec!["*.min.js".into()];
        let form = SearchForm::new(&state, false);
        open(&mut state, form);
        assert_eq!(press(&mut state, KeyCode::Enter), [Effect::SaveConfig]);
        assert!(!state.settings.search.artifacts, "Enter on the first row flips generated folders");

        press(&mut state, KeyCode::Insert);
        typed(&mut state, "[a-");
        assert!(press(&mut state, KeyCode::Enter).is_empty());
        let Some(Overlay::Exclusions(editor)) = &state.overlay else { panic!("the list is open") };
        assert!(editor.error.is_some() && editor.editing.is_some(), "a broken pattern is kept for fixing");
        press(&mut state, KeyCode::Esc);
        press(&mut state, KeyCode::Insert);
        typed(&mut state, " Library/Caches/ ");
        assert_eq!(press(&mut state, KeyCode::Enter), [Effect::SaveConfig]);
        assert_eq!(state.settings.search.exclude, ["*.min.js", "Library/Caches/"]);

        press(&mut state, KeyCode::Up);
        press(&mut state, KeyCode::Enter);
        for _ in 0.."*.min.js".len() {
            press(&mut state, KeyCode::Backspace);
        }
        typed(&mut state, "*.map");
        press(&mut state, KeyCode::Enter);
        assert_eq!(state.settings.search.exclude, ["*.map", "Library/Caches/"]);

        assert_eq!(press(&mut state, KeyCode::Delete), [Effect::SaveConfig]);
        assert_eq!(state.settings.search.exclude, ["Library/Caches/"]);
        press(&mut state, KeyCode::Esc);
        assert!(matches!(state.overlay, Some(Overlay::Search(_))), "Esc goes back to the search dialog");
    }
}
