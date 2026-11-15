use borg_i18n::tr;
use std::ops::Range;

use borg_vfs::Entry;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{AppState, Effect, LineInput, MatchRank, Needle, Overlay, Pattern, Seek, Side, input::Edit};

const GLOB_CHARS: [char; 3] = ['*', '?', '['];
const NEXT_KEY: char = 's';
const PREVIOUS_KEY: char = 'r';
const ALL_KEY: char = 'a';

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Find {
    pub side: Side,
    pub input: LineInput,
    /// Nothing in the panel matches what was typed, so names that match it closely count.
    pub close: bool,
}

impl Find {
    pub fn query(&self) -> Query {
        let query = Query::parse(self.input.text());
        if self.close { query.closely() } else { query }
    }
}

#[derive(Debug, Clone)]
pub enum Query {
    Empty,
    /// Text found in names as typed, or with `near`, closely when not as typed.
    Text {
        needle: Vec<char>,
        fold: bool,
        near: Option<Box<Needle>>,
    },
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
        Self::Text { needle: text.chars().map(|c| if fold { fold_char(c) } else { c }).collect(), fold, near: None }
    }

    /// The query that also finds names close to its text, when the text can match closely.
    pub fn closely(self) -> Self {
        match self {
            Self::Text { needle, fold, .. } => {
                let near = Some(Needle::new(&needle.iter().collect::<String>(), true)).filter(Needle::can_be_close);
                Self::Text { needle, fold, near: near.map(Box::new) }
            }
            other => other,
        }
    }

    pub fn hits(&self, entry: &Entry) -> Option<Vec<Range<usize>>> {
        match self {
            Self::Empty => None,
            Self::Glob(pattern) => pattern.matches(entry).then(Vec::new),
            Self::Text { needle, fold, near } => {
                let found = occurrences(&entry.name, needle, *fold);
                if !found.is_empty() {
                    return Some(found);
                }
                near.as_ref().filter(|near| near.close_to(&entry.name))?.locate(&entry.name, true).map(|(_, s)| s)
            }
        }
    }

    pub fn accepts(&self, entry: &Entry) -> bool {
        self.hits(entry).is_some()
    }

    /// How well `entry`'s name matches the text closely.
    fn rank(&self, entry: &Entry) -> Option<MatchRank> {
        match self {
            Self::Text { near: Some(near), .. } => near.rank(&entry.name, true),
            _ => None,
        }
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
    state.overlay = Some(Overlay::Find(Find { side: state.active, input: LineInput::default(), close: false }));
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
                find.close = only_close(state, &find);
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
            let query = find.query();
            let count = state.panel_mut(find.side).mark_where(|entry| query.accepts(entry), true);
            state.notice = Some(tr!(marked_count, count = count));
        }
        _ => {
            if find.input.apply(key) == Edit::Changed {
                find.close = only_close(state, &find);
                jump(state, &find, Seek::Here);
            }
        }
    }
    state.overlay = Some(Overlay::Find(find));
    Vec::new()
}

/// Whether names count when they match `find` only closely: close matches are allowed and
/// nothing in the panel matches what was typed.
fn only_close(state: &AppState, find: &Find) -> bool {
    let query = Query::parse(find.input.text());
    state.settings.search.typos
        && matches!(query, Query::Text { .. })
        && !state.panel(find.side).entries().iter().any(|entry| query.accepts(entry))
}

/// Moves to the next or previous match; on typing, to the nearest one or, among close matches,
/// the best.
fn jump(state: &mut AppState, find: &Find, step: Seek) {
    let query = find.query();
    let panel = state.panel_mut(find.side);
    if find.close && step == Seek::Here {
        panel.seek_best(|entry| query.rank(entry));
    } else {
        panel.seek(step, |entry| query.accepts(entry));
    }
}

pub fn summary(state: &AppState, find: &Find) -> (usize, usize) {
    let query = find.query();
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
        Entry {
            source: None,
            missing: false,
            usage: None,
            name: name.into(),
            kind: EntryKind::File,
            size: 0,
            modified: None,
            executable: false,
            permissions: 0,
        }
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
    fn close_matches_count_only_once_asked_for() {
        assert_eq!(spans("reciept", "receipt.pdf"), None);
        let close = |query: &str, name: &str| {
            Query::parse(query)
                .closely()
                .hits(&entry(name))
                .map(|hits| hits.into_iter().map(|h| (h.start, h.end)).collect())
        };
        assert_eq!(close("reciept", "my receipt.pdf"), Some(vec![(3, 10)]));
        assert_eq!(close("rec", "my receipt.pdf"), Some(vec![(3, 6)]), "what matches as typed still does");
        assert_eq!(close("cat", "chat.log"), None);
        assert!(matches!(Query::parse("*.rs").closely(), Query::Glob(_)));
    }

    #[test]
    fn globs_match_whole_names() {
        assert!(Query::parse("*.rs").accepts(&entry("main.rs")));
        assert!(!Query::parse("*.rs").accepts(&entry("main.rsx")));
    }
}
