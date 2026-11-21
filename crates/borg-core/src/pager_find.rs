use std::{io, ops::Range};

use borg_view::{Count, Pager, Query, Source};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    AppState, Aside, Content, Effect, Peek, Peeked,
    find_bar::{BarKey, Direction, FindBar, FindNote},
};

const FIND_KEY: char = '/';
const NEXT_MATCH_KEY: char = 'n';
const PREVIOUS_MATCH_KEY: char = 'N';
const SOURCE_KEY: char = 's';
const COUNT_LIMIT: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagerSlot {
    Viewer,
    QuickView,
}

/// What a search through a pager's source found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundInPager {
    pub range: Option<Range<u64>>,
    /// Found only after going on from the other end of the file.
    pub wrapped: bool,
    pub count: Option<Count>,
}

/// Looks for `query` in `source` from `from` on, or back from before it, going on from the
/// other end when there is no match that way, and counts where the match stands when asked.
pub fn find_in(
    source: &dyn Source,
    query: &Query,
    from: u64,
    direction: Direction,
    count: bool,
    go_on: &dyn Fn(u64) -> bool,
) -> io::Result<FoundInPager> {
    let search = query.compile().map_err(io::Error::other)?.ok_or_else(|| io::Error::other("nothing to look for"))?;
    let look = |at: u64| match direction {
        Direction::Forward => search.next(source, at, go_on),
        Direction::Backward => search.previous(source, at, go_on),
    };
    let (range, wrapped) = match look(from)? {
        Some(range) => (Some(range), false),
        None => (look(if direction == Direction::Forward { 0 } else { source.len() })?, true),
    };
    let count = match &range {
        Some(range) if count => Some(search.count(source, range.start, go_on)?),
        _ => None,
    };
    let wrapped = wrapped && range.is_some();
    Ok(FoundInPager { range, wrapped, count })
}

impl AppState {
    pub fn pager(&self, slot: PagerSlot) -> Option<&Pager> {
        match slot {
            PagerSlot::Viewer => match &self.viewer.as_ref()?.content {
                Content::Text(pager) => Some(pager),
                Content::Image(_) => None,
            },
            PagerSlot::QuickView => match self.peeked.as_ref()? {
                Peeked { aside: Aside::QuickView, peek: Peek::Text(pager), .. } => Some(pager),
                _ => None,
            },
        }
    }
}

pub(crate) struct Finding<'a> {
    pub slot: PagerSlot,
    pub next_id: &'a mut u64,
    pub recent: &'a mut Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PagerFind {
    pub bar: FindBar,
    pub(crate) searching: Option<u64>,
    origin: u64,
}

impl PagerFind {
    pub(crate) fn on(query: &Query) -> Self {
        let mut find = Self::default();
        find.bar.load(query);
        find
    }

    pub(crate) fn on_key(
        &mut self,
        pager: &mut Pager,
        key: &KeyEvent,
        finding: Finding<'_>,
    ) -> Option<io::Result<Vec<Effect>>> {
        if self.bar.open {
            return Some(self.on_bar(pager, key, finding));
        }
        let key = crate::keyboard::latin(key);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let direction = match key.code {
            KeyCode::F(7) if alt => Direction::Backward,
            KeyCode::F(7) if shift => Direction::Forward,
            KeyCode::F(7) | KeyCode::Char(FIND_KEY) => return Some(self.open_bar(pager)),
            KeyCode::Char('f') if ctrl => return Some(self.open_bar(pager)),
            KeyCode::Char(NEXT_MATCH_KEY) if !ctrl && !alt => Direction::Forward,
            KeyCode::Char(PREVIOUS_MATCH_KEY) if !ctrl && !alt => Direction::Backward,
            KeyCode::Esc if self.searching.is_some() => return Some(Ok(self.stop(finding.slot))),
            _ => return None,
        };
        if self.bar.search().is_none() {
            return Some(self.open_bar(pager));
        }
        Some(self.search(pager, direction, None, finding))
    }

    pub(crate) fn page_key(&mut self, pager: &Pager, code: KeyCode, slot: PagerSlot) -> Vec<Effect> {
        self.bar.note = None;
        self.bar.count = None;
        if switches_source(pager, code) { self.stop(slot) } else { Vec::new() }
    }

    pub(crate) fn land(
        &mut self,
        pager: &mut Pager,
        query: &Query,
        offset: u64,
        finding: Finding<'_>,
    ) -> io::Result<Vec<Effect>> {
        self.bar.load(query);
        self.bar.open(None);
        self.origin = offset;
        if pager.formatted() {
            pager.toggle_source()?;
        }
        pager.show_matches(self.bar.search().cloned())?;
        self.search(pager, Direction::Forward, Some(offset), finding)
    }

    fn open_bar(&mut self, pager: &mut Pager) -> io::Result<Vec<Effect>> {
        self.origin = pager.offset();
        self.bar.open(None);
        pager.show_matches(self.bar.search().cloned())?;
        Ok(Vec::new())
    }

    fn on_bar(&mut self, pager: &mut Pager, key: &KeyEvent, finding: Finding<'_>) -> io::Result<Vec<Effect>> {
        let action = self.bar.on_key(key, finding.recent);
        if matches!(action, BarKey::Step(_) | BarKey::Closed) {
            self.bar.remember(finding.recent);
        }
        match action {
            BarKey::Step(direction) => self.search(pager, direction, None, finding),
            BarKey::Changed => {
                pager.forget_match()?;
                pager.show_matches(self.bar.search().cloned())?;
                let origin = self.origin;
                self.search(pager, Direction::Forward, Some(origin), finding)
            }
            BarKey::Closed => {
                pager.show_matches(None)?;
                Ok(self.stop(finding.slot))
            }
            BarKey::Kept => Ok(Vec::new()),
            BarKey::Unhandled if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown | KeyCode::F(2 | 4)) => {
                let effects = if switches_source(pager, key.code) { self.stop(finding.slot) } else { Vec::new() };
                crate::viewer::scroll(pager, key.code)?;
                Ok(effects)
            }
            BarKey::Unhandled => Ok(Vec::new()),
        }
    }

    fn search(
        &mut self,
        pager: &mut Pager,
        direction: Direction,
        from: Option<u64>,
        finding: Finding<'_>,
    ) -> io::Result<Vec<Effect>> {
        *finding.next_id += 1;
        let id = *finding.next_id;
        let Some(search) = self.bar.search().cloned() else {
            return Ok(self.stop(finding.slot));
        };
        let from = from.unwrap_or_else(|| match (direction, pager.current_match()) {
            (Direction::Forward, Some(found)) => found.end,
            (Direction::Backward, Some(found)) => found.start,
            _ => pager.offset(),
        });
        if !pager.shows_matches() {
            pager.show_matches(Some(search))?;
        }
        self.searching = Some(id);
        let count = pager.len() <= COUNT_LIMIT;
        Ok(vec![Effect::FindInPager { id, slot: finding.slot, query: self.bar.query(), from, direction, count }])
    }

    pub(crate) fn stop(&mut self, slot: PagerSlot) -> Vec<Effect> {
        if self.searching.take().is_none() {
            return Vec::new();
        }
        if matches!(self.bar.note, Some(FindNote::Searching(_))) {
            self.bar.note = None;
        }
        vec![Effect::StopFindInPager(slot)]
    }

    pub(crate) fn found(&mut self, pager: &mut Pager, id: u64, result: Result<FoundInPager, String>) -> io::Result<()> {
        if self.searching != Some(id) {
            return Ok(());
        }
        self.searching = None;
        match result {
            Ok(FoundInPager { range: Some(range), wrapped, count }) => {
                self.bar.note = wrapped.then_some(FindNote::Wrapped);
                self.bar.count = count;
                pager.reveal(range)
            }
            Ok(FoundInPager { range: None, .. }) => {
                self.bar.note = Some(FindNote::NoMatch);
                self.bar.count = None;
                Ok(())
            }
            Err(error) => Err(io::Error::other(error)),
        }
    }

    pub(crate) fn progress(&mut self, id: u64, percent: u8) {
        if self.searching == Some(id) {
            self.bar.note = Some(FindNote::Searching(percent));
        }
    }
}

fn switches_source(pager: &Pager, code: KeyCode) -> bool {
    matches!(code, KeyCode::Char(SOURCE_KEY) | KeyCode::F(4)) || crate::viewer::is_hex_key(code) && pager.formatted()
}
