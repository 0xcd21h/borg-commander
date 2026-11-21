//! Showing the source as hex rows, read a page at a time, so that a file of any size scrolls at
//! once.

use super::{MAX_LINE, Pager, Position};
use crate::{HexLayout, Mark, Run};
use std::{io, ops::Range};

impl Pager {
    /// Whether the page shows the source as hex rows.
    pub fn hex(&self) -> bool {
        self.hex
    }

    /// How the page lays the source out as hex rows, when it shows them: as many bytes a row as
    /// fit its width.
    pub(super) fn hex_layout(&self) -> Option<HexLayout> {
        self.hex.then(|| HexLayout::fitting(self.width, self.len()))
    }

    /// Switches between the text and the hex rows of the source, keeping the same part of it on
    /// the page. A formatted document shows its source in hex, the bytes of the file.
    pub fn toggle_hex(&mut self) -> io::Result<()> {
        if self.hex {
            self.hex = false;
            self.top = Position { line: self.previous_line_start(self.top.line + 1)?, row: 0 };
        } else {
            if self.formatted {
                self.toggle_source()?;
            }
            self.hex = true;
            let layout = HexLayout::fitting(self.width, self.len());
            self.top = Position { line: layout.row_start(self.top.line), row: 0 };
        }
        self.top = self.top.min(self.last_top()?);
        self.refresh()
    }

    /// Lays the hex rows out again for a new size, keeping the row the page starts on, or the
    /// end of the source on the last row.
    pub(super) fn resize_hex(&mut self, width: usize, height: usize) -> io::Result<()> {
        let bottom = self.at_end() && self.top.line > 0;
        self.width = width;
        self.height = height;
        let layout = HexLayout::fitting(width, self.len());
        self.top = if bottom { self.last_top()? } else { Position { line: layout.row_start(self.top.line), row: 0 } };
        self.refresh()
    }

    /// Reads the page's bytes alone and lays them out as hex rows, with the matches on them.
    pub(super) fn refresh_hex(&mut self, layout: HexLayout) -> io::Result<()> {
        let start = self.top.line;
        let end = start.saturating_add(layout.step().saturating_mul(self.height as u64)).min(self.len());
        let bytes = self.read(start, end)?;
        let found = match &self.matches {
            Some(search) => search.around(&*self.source, start..end, MAX_LINE)?,
            None => Vec::new(),
        };
        let current = self.current.clone();
        let mark = |at: u64| mark_at(&found, current.as_ref(), at);
        let rich_rows: Vec<Vec<Run>> = bytes
            .chunks(layout.columns)
            .zip((start..).step_by(layout.columns))
            .map(|(row, at)| layout.runs(at, row, &mark))
            .collect();
        self.rows = rich_rows.iter().map(|row| row.iter().map(|run| run.text.as_str()).collect()).collect();
        self.rich_rows = rich_rows;
        self.after_page = (end < self.len()).then_some(Position { line: end, row: 0 });
        Ok(())
    }

    /// Marks `found` as the match stepped to and scrolls to its row, when it is off the page,
    /// with a third of the page above it.
    pub(super) fn reveal_hex(&mut self, layout: HexLayout, found: Range<u64>) -> io::Result<()> {
        let target = Position { line: layout.row_start(found.start), row: 0 };
        self.current = Some(found);
        if target < self.top || self.after_page.is_some_and(|after| target >= after) {
            let above = layout.step().saturating_mul((self.height / 3) as u64);
            self.top = Position { line: target.line.saturating_sub(above), row: 0 }.min(self.last_top()?);
        }
        self.refresh()
    }

    /// Puts the row of the byte at `offset` at the top of the hex rows, or as near to it as the
    /// end of the source lets the page go.
    pub fn show_byte(&mut self, offset: u64) -> io::Result<()> {
        let Some(layout) = self.hex_layout() else { return Ok(()) };
        let row = layout.row_start(offset.min(self.len().saturating_sub(1)));
        self.top = Position { line: row, row: 0 }.min(self.last_hex_top(layout));
        self.refresh()
    }

    /// The row the page starts on when its last row is the source's last.
    pub(super) fn last_hex_top(&self, layout: HexLayout) -> Position {
        let rows = layout.rows(self.len()).saturating_sub(self.height.max(1) as u64);
        Position { line: rows.saturating_mul(layout.step()), row: 0 }
    }

    pub(super) fn next_hex_row(&self, layout: HexLayout, position: Position) -> Option<Position> {
        let next = position.line.saturating_add(layout.step());
        (next < self.len()).then_some(Position { line: next, row: 0 })
    }
}

fn mark_at(found: &[Range<u64>], current: Option<&Range<u64>>, at: u64) -> Mark {
    if current.is_some_and(|current| current.contains(&at)) {
        return Mark::Current;
    }
    let next = found.partition_point(|range| range.end <= at);
    if found.get(next).is_some_and(|range| range.contains(&at)) { Mark::Match } else { Mark::None }
}
