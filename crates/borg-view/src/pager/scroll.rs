//! Scrolling the pager: by line, page and column, to either end, and to a match it shows.

use super::{HORIZONTAL_STEP, Pager, Position};
use crate::Search;
use std::{io, ops::Range};

impl Pager {
    /// Marks the matches of `search` on the page, or none; the match stepped to stays marked.
    pub fn show_matches(&mut self, search: Option<Search>) -> io::Result<()> {
        self.matches = search;
        self.refresh()
    }

    /// Whether the page marks matches.
    pub fn shows_matches(&self) -> bool {
        self.matches.is_some()
    }

    /// The match stepped to last.
    pub fn current_match(&self) -> Option<Range<u64>> {
        self.current.clone()
    }

    /// Stops marking the match stepped to, as a new query has other matches.
    pub fn forget_match(&mut self) -> io::Result<()> {
        self.current = None;
        self.refresh()
    }

    /// Marks `found` as the match stepped to and scrolls to it, when it is off the page, with a
    /// third of the page above it.
    pub fn reveal(&mut self, found: Range<u64>) -> io::Result<()> {
        self.current = Some(found.clone());
        let start = self.previous_line_start(found.start + 1)?;
        let line = self.line_at(start)?;
        let cells = self.cells(line)?;
        let first = cells.iter().position(|cell| cell.offset >= found.start).unwrap_or(cells.len());
        let row = if self.wrap {
            let mut seen = 0;
            self.rich_layout(line)?
                .iter()
                .position(|row| {
                    seen += row.iter().map(|run| run.text.chars().count()).sum::<usize>();
                    seen > first
                })
                .unwrap_or(0)
        } else {
            let column: usize = cells[..first].iter().map(|cell| cell.width).sum();
            let width: usize =
                cells[first..].iter().take_while(|cell| cell.offset < found.end).map(|cell| cell.width).sum();
            if column < self.column || column + width > self.column + self.width {
                self.column = column.saturating_sub(self.width / 4);
            }
            0
        };
        let target = Position { line: start, row };
        if target < self.top || self.after_page.is_some_and(|after| target >= after) {
            self.top = target;
            for _ in 0..self.height / 3 {
                match self.previous_position(self.top)? {
                    Some(position) => self.top = position,
                    None => break,
                }
            }
            self.top = self.top.min(self.last_top()?);
        }
        self.refresh()
    }

    pub fn scroll_down(&mut self, rows: usize) -> io::Result<()> {
        let mut after = self.after_page;
        for _ in 0..rows {
            let Some(bottom) = after else { break };
            self.top = self.next_position(self.top)?.unwrap_or(self.top);
            after = self.next_position(bottom)?;
        }
        self.refresh()
    }

    pub fn scroll_up(&mut self, rows: usize) -> io::Result<()> {
        for _ in 0..rows {
            match self.previous_position(self.top)? {
                Some(position) => self.top = position,
                None => break,
            }
        }
        self.refresh()
    }

    pub fn page_down(&mut self) -> io::Result<()> {
        self.scroll_down(self.page())
    }

    pub fn page_up(&mut self) -> io::Result<()> {
        self.scroll_up(self.page())
    }

    pub fn home(&mut self) -> io::Result<()> {
        self.top = Position { line: 0, row: 0 };
        self.refresh()
    }

    pub fn end(&mut self) -> io::Result<()> {
        self.top = self.last_top()?;
        self.refresh()
    }

    pub fn scroll_left(&mut self) -> io::Result<()> {
        self.column = self.column.saturating_sub(HORIZONTAL_STEP);
        self.refresh()
    }

    pub fn scroll_right(&mut self) -> io::Result<()> {
        if !self.wrap {
            self.column += HORIZONTAL_STEP;
        }
        self.refresh()
    }

    pub(super) fn page(&self) -> usize {
        self.height.saturating_sub(1).max(1)
    }

    pub(super) fn at_end(&self) -> bool {
        self.after_page.is_none()
    }

    pub(super) fn last_top(&self) -> io::Result<Position> {
        let len = self.len();
        if len == 0 {
            return Ok(Position { line: 0, row: 0 });
        }
        let last_line = self.previous_line_start(len)?;
        let rows = self.row_count(self.line_at(last_line)?)?;
        let mut top = Position { line: last_line, row: rows.saturating_sub(1) };
        for _ in 1..self.height.max(1) {
            match self.previous_position(top)? {
                Some(position) => top = position,
                None => break,
            }
        }
        Ok(top)
    }
}
