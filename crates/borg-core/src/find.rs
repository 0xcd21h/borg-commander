use std::ops::Range;

use borg_vfs::Entry;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{AppState, Effect, LineInput, Overlay, Pattern, Seek, Side, input::Edit};

const GLOB_CHARS: [char; 3] = ['*', '?', '['];
const NEXT_KEY: char = 's';
const PREVIOUS_KEY: char = 'r';
const ALL_KEY: char = 'a';

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Find {
    pub side: Side,
    pub input: LineInput,
}

#[derive(Debug, Clone)]
pub enum Query {
    Empty,
    Text { needle: Vec<char>, fold: bool },
    Glob(Pattern),
}

fn fold_char(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

impl Query {
    pub fn parse(text: &str) -> Self {
        if text.is_empty() {
            return Self::Empty;
        }
        if text.contains(GLOB_CHARS)
            && let Ok(pattern) = Pattern::parse(text)
        {
            return Self::Glob(pattern);
        }
        let fold = !text.chars().any(char::is_uppercase);
        Self::Text { needle: text.chars().map(|c| if fold { fold_char(c) } else { c }).collect(), fold }
    }

    pub fn hits(&self, entry: &Entry) -> Option<Vec<Range<usize>>> {
        match self {
            Self::Empty => None,
            Self::Glob(pattern) => pattern.matches(entry).then(Vec::new),
            Self::Text { needle, fold } => {
                let found = occurrences(&entry.name, needle, *fold);
                (!found.is_empty()).then_some(found)
            }
        }
    }

    pub fn accepts(&self, entry: &Entry) -> bool {
        self.hits(entry).is_some()
    }
}

fn occurrences(name: &str, needle: &[char], fold: bool) -> Vec<Range<usize>> {
    let chars: Vec<(usize, char)> = name.char_indices().collect();
    let normal = |c: char| if fold { fold_char(c) } else { c };
    let mut found = Vec::new();
    let mut at = 0;
    while !needle.is_empty() && at + needle.len() <= chars.len() {
        if needle.iter().zip(&chars[at..]).all(|(want, (_, have))| normal(*have) == *want) {
            let end = chars.get(at + needle.len()).map_or(name.len(), |(byte, _)| *byte);
            found.push(chars[at].0..end);
            at += needle.len();
        } else {
            at += 1;
        }
    }
    found
}

pub(crate) fn open(state: &mut AppState) -> Vec<Effect> {
    state.overlay = Some(Overlay::Find(Find { side: state.active, input: LineInput::default() }));
    Vec::new()
}

pub(crate) fn on_key(state: &mut AppState, mut find: Find, key: &KeyEvent) -> Vec<Effect> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            if !find.input.text().is_empty() {
                state.last_find = Some(find.input.text().to_owned());
            }
            state.overlay = None;
            return Vec::new();
        }
        KeyCode::Char(NEXT_KEY) if ctrl && find.input.text().is_empty() => {
            if let Some(last) = state.last_find.clone() {
                find.input = LineInput::new(last);
                jump(state, &find, Seek::Here);
            }
        }
        KeyCode::Down | KeyCode::Tab => jump(state, &find, Seek::Next),
        KeyCode::Char(NEXT_KEY) if ctrl => jump(state, &find, Seek::Next),
        KeyCode::Up | KeyCode::BackTab => jump(state, &find, Seek::Previous),
        KeyCode::Char(PREVIOUS_KEY) if ctrl => jump(state, &find, Seek::Previous),
        KeyCode::Insert => {
            state.panel_mut(find.side).toggle_mark();
            jump(state, &find, Seek::Next);
        }
        KeyCode::Char(ALL_KEY) if ctrl => {
            let query = Query::parse(find.input.text());
            let count = state.panel_mut(find.side).mark_where(|entry| query.accepts(entry), true);
            state.notice = Some(format!("{count} marked"));
        }
        _ => {
            if find.input.apply(key) == Edit::Changed {
                jump(state, &find, Seek::Here);
            }
        }
    }
    state.overlay = Some(Overlay::Find(find));
    Vec::new()
}

fn jump(state: &mut AppState, find: &Find, step: Seek) {
    let query = Query::parse(find.input.text());
    state.panel_mut(find.side).seek(step, |entry| query.accepts(entry));
}

pub fn summary(state: &AppState, find: &Find) -> (usize, usize) {
    let query = Query::parse(find.input.text());
    let panel = state.panel(find.side);
    let matches: Vec<usize> = (0..panel.len())
        .filter(|index| matches!(panel.row(*index), Some(crate::Row::Entry(entry)) if query.accepts(entry)))
        .collect();
    let current = matches.iter().position(|index| *index == panel.cursor()).map_or(0, |position| position + 1);
    (current, matches.len())
}

#[cfg(test)]
mod tests {
    use borg_vfs::EntryKind;

    use super::*;

    fn entry(name: &str) -> Entry {
        Entry { name: name.into(), kind: EntryKind::File, size: 0, modified: None, executable: false, permissions: 0 }
    }

    fn spans(query: &str, name: &str) -> Option<Vec<(usize, usize)>> {
        Query::parse(query).hits(&entry(name)).map(|hits| hits.into_iter().map(|hit| (hit.start, hit.end)).collect())
    }

    #[test]
    fn smart_case_substring_with_highlight_ranges() {
        assert_eq!(spans("js", "next.config.JS"), Some(vec![(12, 14)]));
        assert_eq!(spans("JS", "next.config.js"), None);
        assert_eq!(spans("o", "foo"), Some(vec![(1, 2), (2, 3)]));
        assert_eq!(spans("é", "cafÉ.txt"), Some(vec![(3, 5)]));
        assert_eq!(spans("", "x"), None);
    }

    #[test]
    fn globs_match_whole_names() {
        assert!(Query::parse("*.rs").accepts(&entry("main.rs")));
        assert!(!Query::parse("*.rs").accepts(&entry("main.rsx")));
    }
}
