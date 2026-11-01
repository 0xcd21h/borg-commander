//! The find bar the editor and the viewer share: a query typed in their status line and looked
//! for as it changes, with switches for case, whole words and regular expressions.

use borg_i18n::tr;
use borg_view::{Count, Query, Search};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{LineInput, input::Edit};

/// Queries kept for Ctrl-P and Ctrl-N, the latest first.
const RECENT_LIMIT: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Backward,
}

/// What the bar says about the search beside the query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindNote {
    NoMatch,
    /// The search went on from the other end of the text.
    Wrapped,
    /// How far a search through a large file got, in percent.
    Searching(u8),
    /// Why the query is not a regular expression.
    Invalid(String),
}

/// What a key did in the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BarKey {
    Step(Direction),
    /// The query or a switch changed.
    Changed,
    Closed,
    Kept,
    /// Not the bar's key, for the editor or viewer to use.
    Unhandled,
}

#[derive(Debug, Clone, Default)]
pub struct FindBar {
    pub input: LineInput,
    /// Match case exactly; otherwise a query without capitals ignores case.
    pub case: bool,
    pub words: bool,
    pub regex: bool,
    /// The bar shows and takes keys.
    pub open: bool,
    /// Where the match stepped to stands among all of them.
    pub count: Option<Count>,
    pub note: Option<FindNote>,
    search: Option<Search>,
    /// Which recent query Ctrl-P and Ctrl-N show.
    recalled: Option<usize>,
}

/// Bars are alike when their query, switches and state are; the compiled search follows from those.
impl PartialEq for FindBar {
    fn eq(&self, other: &Self) -> bool {
        (self.input == other.input && self.case == other.case && self.words == other.words)
            && (self.regex == other.regex && self.open == other.open && self.count == other.count)
            && (self.note == other.note && self.recalled == other.recalled)
    }
}

impl Eq for FindBar {}

impl FindBar {
    pub fn query(&self) -> Query {
        Query { text: self.input.text().to_owned(), case: self.case, words: self.words, regex: self.regex }
    }

    /// The search the query asks for, unless it is empty or not a regular expression.
    pub fn search(&self) -> Option<&Search> {
        self.search.as_ref()
    }

    /// Opens the bar on `seed`, or else on the query it had.
    pub(crate) fn open(&mut self, seed: Option<String>) {
        if let Some(seed) = seed {
            self.input = LineInput::new(seed);
        }
        self.open = true;
        self.recalled = None;
        self.compile();
    }

    /// Takes `query`'s text and switches.
    pub(crate) fn load(&mut self, query: &Query) {
        self.input = LineInput::new(query.text.as_str());
        (self.case, self.words, self.regex) = (query.case, query.words, query.regex);
        self.recalled = None;
        self.compile();
    }

    /// Looks for `text` from now on, as the replace dialog's field asks.
    pub(crate) fn set_text(&mut self, text: &str) {
        if text != self.input.text() {
            self.input = LineInput::new(text);
            self.compile();
        }
    }

    /// Types `text` into the field, as a paste does, leaving out control characters.
    pub(crate) fn paste(&mut self, text: &str) {
        for c in text.chars().filter(|c| !c.is_control()).take(512) {
            self.input.apply(&KeyCode::Char(c).into());
        }
        self.recalled = None;
        self.compile();
    }

    fn compile(&mut self) {
        self.count = None;
        (self.search, self.note) = match self.query().compile() {
            Ok(search) => (search, None),
            Err(reason) => (None, Some(FindNote::Invalid(reason))),
        };
    }

    /// The switches' keys, Alt-C, Alt-W and Alt-R as in VS Code, which the replace dialog takes too.
    pub(crate) fn switch(&mut self, key: &KeyEvent) -> bool {
        if !key.modifiers.contains(KeyModifiers::ALT) {
            return false;
        }
        let switch = match key.code {
            KeyCode::Char('c' | 'C') => &mut self.case,
            KeyCode::Char('w' | 'W') => &mut self.words,
            KeyCode::Char('r' | 'R') => &mut self.regex,
            _ => return false,
        };
        *switch = !*switch;
        self.compile();
        true
    }

    pub(crate) fn on_key(&mut self, key: &KeyEvent, recent: &[String]) -> BarKey {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Esc => {
                self.open = false;
                BarKey::Closed
            }
            KeyCode::Enter | KeyCode::F(3) if shift => BarKey::Step(Direction::Backward),
            KeyCode::F(7) if alt => BarKey::Step(Direction::Backward),
            KeyCode::Up => BarKey::Step(Direction::Backward),
            KeyCode::Enter | KeyCode::Down | KeyCode::F(3 | 7) => BarKey::Step(Direction::Forward),
            _ => self.edit(key, recent),
        }
    }

    /// A key that edits the query: typing, a switch, or Ctrl-P and Ctrl-N for a recent query.
    /// The search dialog's contents field takes these too.
    pub(crate) fn edit(&mut self, key: &KeyEvent, recent: &[String]) -> BarKey {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.switch(key) {
            return BarKey::Changed;
        }
        match key.code {
            KeyCode::Char('p') if ctrl => self.recall(recent, true),
            KeyCode::Char('n') if ctrl => self.recall(recent, false),
            _ => {
                let before = self.input.text().to_owned();
                match self.input.apply(key) {
                    Edit::Unhandled => BarKey::Unhandled,
                    Edit::Changed if self.input.text() == before => BarKey::Kept,
                    Edit::Changed => {
                        self.recalled = None;
                        self.compile();
                        BarKey::Changed
                    }
                }
            }
        }
    }

    /// Shows an older recent query, or a newer one and then an empty field.
    fn recall(&mut self, recent: &[String], older: bool) -> BarKey {
        let index = match (self.recalled, older) {
            (None, true) => Some(0),
            (Some(index), true) => Some((index + 1).min(recent.len().saturating_sub(1))),
            (Some(index), false) => index.checked_sub(1),
            (None, false) => return BarKey::Kept,
        };
        let Some(text) = index.map_or(Some(""), |index| recent.get(index).map(String::as_str)) else {
            return BarKey::Kept;
        };
        self.recalled = index;
        self.input = LineInput::new(text);
        self.compile();
        BarKey::Changed
    }

    /// Keeps the query among the recent ones, unless it is empty.
    pub(crate) fn remember(&self, recent: &mut Vec<String>) {
        let text = self.input.text();
        if text.is_empty() {
            return;
        }
        recent.retain(|kept| kept != text);
        recent.insert(0, text.to_owned());
        recent.truncate(RECENT_LIMIT);
    }
}

/// What the bar says beside the query: where the match stands among all, or what went wrong.
pub fn summary(find: &FindBar) -> Option<String> {
    let wrapped = find.note == Some(FindNote::Wrapped);
    match (&find.note, find.count) {
        (Some(FindNote::Invalid(reason)), _) => Some(tr!(find_invalid, reason = reason)),
        (Some(FindNote::NoMatch), _) => Some(tr!(find_no_match).into()),
        (Some(FindNote::Searching(percent)), _) => Some(tr!(find_searching, percent = percent)),
        (_, Some(count)) => {
            let index = count.index.map_or_else(|| "?".to_owned(), |index| index.to_string());
            let total = if count.more { format!("{}+", count.total) } else { count.total.to_string() };
            let place = tr!(find_count, index = index, total = total);
            Some(if wrapped { format!("{place} · {}", tr!(find_wrapped)) } else { place })
        }
        (Some(FindNote::Wrapped), None) => Some(tr!(find_wrapped).into()),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn the_bar_types_steps_switches_and_recalls() {
        let mut bar = FindBar::default();
        bar.open(None);
        let recent = vec!["latest".to_owned(), "older".to_owned()];
        assert_eq!(bar.on_key(&KeyCode::Char('a').into(), &recent), BarKey::Changed);
        assert!(bar.search().is_some());
        assert_eq!(bar.on_key(&KeyCode::Enter.into(), &recent), BarKey::Step(Direction::Forward));
        assert_eq!(bar.on_key(&key(KeyCode::Enter, KeyModifiers::SHIFT), &recent), BarKey::Step(Direction::Backward));
        assert_eq!(bar.on_key(&KeyCode::Up.into(), &recent), BarKey::Step(Direction::Backward));
        assert_eq!(bar.on_key(&KeyCode::Home.into(), &recent), BarKey::Kept);
        assert_eq!(bar.on_key(&KeyCode::PageDown.into(), &recent), BarKey::Unhandled);
        assert_eq!(bar.on_key(&key(KeyCode::Char('r'), KeyModifiers::ALT), &recent), BarKey::Changed);
        assert!(bar.regex);
        bar.set_text("(");
        assert_eq!(bar.note, Some(FindNote::Invalid("unclosed group".into())));
        assert!(bar.search().is_none());
        assert_eq!(bar.on_key(&key(KeyCode::Char('p'), KeyModifiers::CONTROL), &recent), BarKey::Changed);
        assert_eq!(bar.input.text(), "latest");
        bar.on_key(&key(KeyCode::Char('p'), KeyModifiers::CONTROL), &recent);
        bar.on_key(&key(KeyCode::Char('p'), KeyModifiers::CONTROL), &recent);
        assert_eq!(bar.input.text(), "older", "the oldest query stays");
        bar.on_key(&key(KeyCode::Char('n'), KeyModifiers::CONTROL), &recent);
        bar.on_key(&key(KeyCode::Char('n'), KeyModifiers::CONTROL), &recent);
        assert_eq!(bar.input.text(), "", "past the latest the field empties");
        assert_eq!(bar.on_key(&KeyCode::Esc.into(), &recent), BarKey::Closed);
        assert!(!bar.open);

        let mut kept = recent.clone();
        bar.set_text("older");
        bar.remember(&mut kept);
        assert_eq!(kept, ["older", "latest"]);
    }
}
