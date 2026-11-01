use crate::{AppState, Effect, FindBar, FindNote, LineInput, Overlay, PastSearch, ResultSource};
use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

/// The search dialog's fields, in the order Tab takes them.
pub const PATTERN_FIELD: usize = 0;
pub const CONTENTS_FIELD: usize = 1;
pub const ROOT_FIELD: usize = 2;
pub const KIND_FIELD: usize = 3;
pub const MINIMUM_FIELD: usize = 4;
pub const FILES_FIELD: usize = 5;
pub const HIDDEN_FIELD: usize = 6;
pub const SKIP_FIELD: usize = 7;
pub const SEARCH_FIELDS: usize = 8;
/// Recent searches the search dialog lists.
pub const RECENT_SHOWN: usize = 5;
const UNIT: f64 = 1024.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchKind {
    #[default]
    All,
    Files,
    Folders,
    FolderTotals,
    FolderTree,
}

impl SearchKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => tr!(files_and_folders),
            Self::Files => tr!(files_only),
            Self::Folders => tr!(folders_only),
            Self::FolderTotals => tr!(folder_totals),
            Self::FolderTree => tr!(folder_size_tree),
        }
    }
    fn step(self, reverse: bool) -> Self {
        let kinds = [Self::All, Self::Files, Self::Folders, Self::FolderTotals, Self::FolderTree];
        let index = kinds.iter().position(|kind| *kind == self).unwrap_or(0);
        kinds[(index + if reverse { kinds.len() - 1 } else { 1 }) % kinds.len()]
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub refresh: bool,
    pub kind: SearchKind,
    pub minimum: u64,
    pub minimum_files: u64,
    /// Text the files must contain. A search with it lists files only.
    pub contents: Option<Box<borg_view::Query>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchForm {
    pub pattern: LineInput,
    /// Text the files must contain, with the find bar's switches for case, words and regular
    /// expressions.
    pub contents: FindBar,
    pub root: LineInput,
    pub original_root: PathBuf,
    pub kind: SearchKind,
    pub minimum: LineInput,
    pub files: LineInput,
    pub hidden: bool,
    /// Leave out generated folders, ignored files and the configured globs.
    pub skip: bool,
    pub selected: usize,
    pub error: Option<String>,
    /// The recalled query's place in the history, and what was typed before recalling.
    pub recalled: Option<(usize, String)>,
}

impl SearchForm {
    pub(crate) fn new(state: &AppState, space: bool) -> Self {
        let request = match state.active_panel().results.as_ref().map(|r| &r.source) {
            Some(ResultSource::Search { request, .. }) if !space => Some(request),
            _ => None,
        };
        let root = request.map_or_else(|| state.active_panel().dir().to_path_buf(), |r| r.root.clone());
        let mut contents = FindBar::default();
        if let Some(query) = request.and_then(|r| r.options.contents.as_ref()) {
            contents.load(query);
        }
        Self {
            pattern: LineInput::new(request.map_or_else(|| "*".into(), |r| r.query.clone())),
            contents,
            root: LineInput::new(root.display().to_string()),
            original_root: root,
            kind: if space { SearchKind::FolderTotals } else { request.map_or(SearchKind::All, |r| r.options.kind) },
            minimum: LineInput::new(
                request.filter(|r| r.options.minimum > 0).map_or_else(String::new, |r| r.options.minimum.to_string()),
            ),
            files: LineInput::new(
                request
                    .filter(|r| r.options.minimum_files > 0)
                    .map_or_else(String::new, |r| r.options.minimum_files.to_string()),
            ),
            hidden: space || request.map_or(state.settings.preferences.show_hidden, |r| r.hidden),
            skip: request.is_none_or(|r| r.skip.is_some()),
            selected: 0,
            error: None,
            recalled: None,
        }
    }

    /// Whether the dialog shows `field`. Size scans measure folders and read everything, so
    /// they look inside no file and have no switch for leaving things out.
    pub fn shows(&self, field: usize) -> bool {
        let sizes = matches!(self.kind, SearchKind::FolderTotals | SearchKind::FolderTree);
        field < SEARCH_FIELDS && !(sizes && matches!(field, CONTENTS_FIELD | SKIP_FIELD))
    }

    /// The field Tab goes to, or Shift-Tab when `back`, passing over those not shown.
    fn step(&self, back: bool) -> usize {
        let mut field = self.selected;
        loop {
            field = (field + if back { SEARCH_FIELDS - 1 } else { 1 }) % SEARCH_FIELDS;
            if self.shows(field) {
                return field;
            }
        }
    }

    /// The folder the search runs in.
    pub fn root(&self, state: &AppState) -> PathBuf {
        if self.root.text() == self.original_root.display().to_string() {
            self.original_root.clone()
        } else {
            state.active_panel().dir().join(PathBuf::from(self.root.text()))
        }
    }

    /// The recent searches the dialog lists, with their places in the history: the latest, or
    /// those up to the one recalled.
    pub fn recent<'a>(&self, state: &'a AppState) -> Vec<(usize, &'a PastSearch)> {
        let first = self.recalled.as_ref().map_or(0, |(at, _)| (at + 1).saturating_sub(RECENT_SHOWN));
        state.search_history.recent(&self.root(state)).into_iter().enumerate().skip(first).take(RECENT_SHOWN).collect()
    }

    /// Ctrl-P recalls an older search, Ctrl-N a newer one and finally what was typed.
    fn recall(&mut self, state: &AppState, older: bool) {
        let count = state.search_history.recent(&self.root(state)).len();
        let current = self.recalled.as_ref().map(|(at, _)| *at);
        let next = match (current, older) {
            (None, true) => Some(0),
            (Some(at), true) => Some((at + 1).min(count.saturating_sub(1))),
            (Some(at), false) => at.checked_sub(1),
            (None, false) => return,
        };
        match next.filter(|at| *at < count) {
            Some(at) => self.take_recent(state, at),
            None => {
                if let Some((_, draft)) = self.recalled.take() {
                    self.pattern = LineInput::new(draft);
                }
            }
        }
    }

    /// Puts the search at `at` in the history in the query field, keeping what was typed for
    /// Ctrl-N to bring back.
    pub(crate) fn take_recent(&mut self, state: &AppState, at: usize) {
        let past = state.search_history.recent(&self.root(state));
        let Some(search) = past.get(at) else { return };
        let draft = self.recalled.take().map_or_else(|| self.pattern.text().to_owned(), |(_, draft)| draft);
        self.pattern = LineInput::new(search.query.as_str());
        self.recalled = Some((at, draft));
        self.selected = 0;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionPicker {
    pub selected: usize,
    pub creating: Option<LineInput>,
}

pub(crate) fn collections_key(state: &mut AppState, mut picker: CollectionPicker, key: &KeyEvent) -> Vec<Effect> {
    if let Some(input) = picker.creating.as_mut() {
        match key.code {
            KeyCode::Esc => picker.creating = None,
            KeyCode::Enter => {
                let name = input.text().trim().to_owned();
                if !name.is_empty() {
                    state.overlay = None;
                    return crate::results::collection(state, &name, state.active);
                }
            }
            _ => {
                input.apply(key);
            }
        }
    } else {
        let last = state.collections.len().saturating_sub(1);
        let page = state.rows.saturating_sub(12).clamp(1, 10) as usize;
        match key.code {
            KeyCode::Esc => {
                state.overlay = None;
                return Vec::new();
            }
            KeyCode::Up => picker.selected = picker.selected.saturating_sub(1),
            KeyCode::Down => picker.selected = (picker.selected + 1).min(last),
            KeyCode::PageUp => picker.selected = picker.selected.saturating_sub(page),
            KeyCode::PageDown => picker.selected = (picker.selected + page).min(last),
            KeyCode::Home => picker.selected = 0,
            KeyCode::End => picker.selected = last,
            KeyCode::F(7) | KeyCode::Char('n') => picker.creating = Some(LineInput::new(String::new())),
            KeyCode::Enter => {
                if let Some(collection) = state.collections.get(picker.selected) {
                    let name = collection.name.clone();
                    state.overlay = None;
                    return crate::results::collection(state, &name, state.active);
                }
                picker.creating = Some(LineInput::new(String::new()));
            }
            _ => {}
        }
    }
    state.overlay = Some(Overlay::Collections(picker));
    Vec::new()
}

pub(crate) fn search_key(state: &mut AppState, mut form: SearchForm, key: &KeyEvent) -> Vec<Effect> {
    if form.shows(CONTENTS_FIELD) && form.contents.switch(key) {
        state.overlay = Some(Overlay::Search(form));
        return Vec::new();
    }
    match key.code {
        KeyCode::Esc => {
            state.overlay = None;
            return Vec::new();
        }
        KeyCode::Tab | KeyCode::Down => form.selected = form.step(false),
        KeyCode::BackTab | KeyCode::Up => form.selected = form.step(true),
        KeyCode::Char(c @ ('p' | 'n')) if key.modifiers == KeyModifiers::CONTROL && form.selected == PATTERN_FIELD => {
            form.recall(state, c == 'p');
        }
        KeyCode::F(4) => return crate::exclusions::open(state, form),
        KeyCode::Enter => match options(&form) {
            Ok(options) => {
                let root = form.root(state);
                let query = form.pattern.text().trim();
                let query = if query.is_empty() { "*" } else { query };
                match crate::results::name_search(state, root, query, form.hidden, form.skip, options) {
                    Ok(request) => {
                        state.overlay = None;
                        let mut effects = Vec::new();
                        if state.settings.search.history && state.search_history.record(&request.query, &request.root) {
                            effects.push(Effect::SaveSearchHistory);
                        }
                        form.contents.remember(&mut state.find_recent);
                        effects.extend(crate::results::start_search(state, request));
                        return effects;
                    }
                    Err(error) => form.error = Some(error.message()),
                }
            }
            Err(error) => form.error = Some(error),
        },
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.selected == KIND_FIELD => {
            form.kind = form.kind.step(key.code == KeyCode::Left);
        }
        KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if form.selected == HIDDEN_FIELD => {
            form.hidden = !form.hidden;
        }
        KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if form.selected == SKIP_FIELD => form.skip = !form.skip,
        _ if form.selected == CONTENTS_FIELD => {
            form.contents.edit(key, &state.find_recent);
        }
        _ => {
            if form.selected == PATTERN_FIELD {
                form.recalled = None;
            }
            let input = match form.selected {
                PATTERN_FIELD => Some(&mut form.pattern),
                ROOT_FIELD => Some(&mut form.root),
                MINIMUM_FIELD => Some(&mut form.minimum),
                FILES_FIELD => Some(&mut form.files),
                _ => None,
            };
            if let Some(input) = input {
                input.apply(key);
            }
        }
    }
    if key.modifiers == KeyModifiers::ALT && key.code == KeyCode::Char('d') {
        form.kind = SearchKind::FolderTotals;
        form.hidden = true;
        if !form.shows(form.selected) {
            form.selected = KIND_FIELD;
        }
    }
    state.overlay = Some(Overlay::Search(form));
    Vec::new()
}

/// What the form asks the search for, or why it cannot search. Folder sizes and file counts
/// turn a name search into folder totals; text to look for makes it a search of files, and
/// size scans leave that text aside.
fn options(form: &SearchForm) -> Result<SearchOptions, String> {
    let files = form.files.text().trim();
    let files = if files.is_empty() { Ok(0) } else { files.parse::<u64>() };
    let (Some(minimum), Ok(minimum_files)) = (parse_size(form.minimum.text()), files) else {
        return Err(tr!(use_a_size_such_as_500m_or).into());
    };
    if let Some(FindNote::Invalid(reason)) = form.contents.note.as_ref().filter(|_| form.shows(CONTENTS_FIELD)) {
        return Err(tr!(find_invalid, reason = reason));
    }
    let contents = Some(form.contents.query()).filter(|query| form.shows(CONTENTS_FIELD) && !query.text.is_empty());
    if contents.is_some() && minimum_files > 0 {
        return Err(tr!(contents_lists_files).into());
    }
    let totals =
        form.kind != SearchKind::FolderTree && (minimum_files > 0 || (form.kind == SearchKind::Folders && minimum > 0));
    let kind = if contents.is_some() {
        SearchKind::Files
    } else if totals {
        SearchKind::FolderTotals
    } else {
        form.kind
    };
    Ok(SearchOptions { refresh: false, kind, minimum, minimum_files, contents: contents.map(Box::new) })
}

pub(crate) fn parse_size(text: &str) -> Option<u64> {
    let text = text.trim().to_ascii_lowercase();
    if text.is_empty() {
        return Some(0);
    }
    let split = text.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(text.len());
    let number: f64 = text[..split].parse().ok()?;
    let power = match text[split..].trim() {
        "" | "b" => 0,
        "k" | "kb" | "kib" => 1,
        "m" | "mb" | "mib" => 2,
        "g" | "gb" | "gib" => 3,
        "t" | "tb" | "tib" => 4,
        _ => return None,
    };
    let bytes = number * UNIT.powi(power);
    (bytes.is_finite() && bytes >= 0.0 && bytes < u64::MAX as f64).then_some(bytes as u64)
}

#[cfg(test)]
mod tests;
