use std::{fmt, io, sync::Arc};

use crate::{Source, text};

pub const MAX_LINE: u64 = 64 * 1024;
const BACK_SCAN_LIMIT: u64 = 1024 * 1024;
const BACK_CHUNK: u64 = 64 * 1024;
const FIRST_READ: u64 = 4 * 1024;
const NEWLINE: u8 = b'\n';
const HORIZONTAL_STEP: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Line {
    start: u64,
    end: u64,
    next: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Position {
    line: u64,
    row: usize,
}

#[derive(Clone)]
pub struct Pager {
    source: Arc<dyn Source>,
    line_limit: u64,
    scan_limit: u64,
    top: Position,
    wrap: bool,
    column: usize,
    width: usize,
    height: usize,
    rows: Vec<String>,
}

impl fmt::Debug for Pager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pager")
            .field("len", &self.source.len())
            .field("top", &self.top)
            .field("wrap", &self.wrap)
            .field("column", &self.column)
            .finish_non_exhaustive()
    }
}

impl Pager {
    pub fn new(source: Arc<dyn Source>, width: usize, height: usize) -> io::Result<Self> {
        Self::with_limits(source, width, height, MAX_LINE, BACK_SCAN_LIMIT)
    }

    pub fn with_limits(
        source: Arc<dyn Source>,
        width: usize,
        height: usize,
        line_limit: u64,
        scan_limit: u64,
    ) -> io::Result<Self> {
        let mut pager = Self {
            source,
            line_limit: line_limit.max(1),
            scan_limit: scan_limit.max(line_limit),
            top: Position { line: 0, row: 0 },
            wrap: true,
            column: 0,
            width,
            height,
            rows: Vec::new(),
        };
        pager.refresh()?;
        Ok(pager)
    }

    pub fn rows(&self) -> &[String] {
        &self.rows
    }

    pub fn len(&self) -> u64 {
        self.source.len()
    }

    pub fn is_empty(&self) -> bool {
        self.source.is_empty()
    }

    pub fn offset(&self) -> u64 {
        self.top.line
    }

    pub fn wrapped(&self) -> bool {
        self.wrap
    }

    pub fn column(&self) -> usize {
        self.column
    }

    pub fn percent(&self) -> u64 {
        const COMPLETE: u64 = 100;
        match self.len() {
            0 => COMPLETE,
            _ if self.at_end() => COMPLETE,
            len => self.top.line.saturating_mul(COMPLETE) / len,
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) -> io::Result<()> {
        self.width = width;
        self.height = height;
        self.top.row = 0;
        self.refresh()
    }

    pub fn toggle_wrap(&mut self) -> io::Result<()> {
        self.wrap = !self.wrap;
        self.column = 0;
        self.top.row = 0;
        self.refresh()
    }

    pub fn scroll_down(&mut self, rows: usize) -> io::Result<()> {
        let limit = self.last_top()?;
        for _ in 0..rows {
            if self.top == limit {
                break;
            }
            self.top = self.next_position(self.top)?.unwrap_or(self.top);
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

    fn page(&self) -> usize {
        self.height.saturating_sub(1).max(1)
    }

    fn at_end(&self) -> bool {
        self.last_top().is_ok_and(|last| last == self.top)
    }

    fn refresh(&mut self) -> io::Result<()> {
        let mut rows = Vec::with_capacity(self.height);
        let mut position = (!self.is_empty()).then_some(self.top);
        while rows.len() < self.height {
            let Some(current) = position else { break };
            let line = self.line_at(current.line)?;
            let layout = self.layout(line)?;
            rows.extend(layout.into_iter().skip(current.row).take(self.height - rows.len()));
            position = (line.next < self.len()).then_some(Position { line: line.next, row: 0 });
        }
        self.rows = rows;
        Ok(())
    }

    fn layout(&self, line: Line) -> io::Result<Vec<String>> {
        let cells = text::display(&self.read(line.start, line.end)?);
        Ok(if self.wrap { text::wrap(&cells, self.width) } else { vec![text::slice(&cells, self.column, self.width)] })
    }

    fn row_count(&self, line: Line) -> io::Result<usize> {
        if self.wrap { self.layout(line).map(|rows| rows.len()) } else { Ok(1) }
    }

    fn next_position(&self, position: Position) -> io::Result<Option<Position>> {
        let line = self.line_at(position.line)?;
        if position.row + 1 < self.row_count(line)? {
            return Ok(Some(Position { row: position.row + 1, ..position }));
        }
        Ok((line.next < self.len()).then_some(Position { line: line.next, row: 0 }))
    }

    fn previous_position(&self, position: Position) -> io::Result<Option<Position>> {
        if position.row > 0 {
            return Ok(Some(Position { row: position.row - 1, ..position }));
        }
        if position.line == 0 {
            return Ok(None);
        }
        let start = self.previous_line_start(position.line)?;
        let rows = self.row_count(self.line_at(start)?)?;
        Ok(Some(Position { line: start, row: rows.saturating_sub(1) }))
    }

    fn last_top(&self) -> io::Result<Position> {
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

    fn line_at(&self, start: u64) -> io::Result<Line> {
        let window = self.line_limit + 1;
        let mut bytes = self.read(start, start + FIRST_READ.min(window))?;
        if !bytes.contains(&NEWLINE) && bytes.len() as u64 == FIRST_READ.min(window) {
            bytes = self.read(start, start + window)?;
        }
        let read = bytes.len() as u64;
        Ok(match bytes.iter().position(|&b| b == NEWLINE) {
            Some(i) => Line { start, end: start + i as u64, next: start + i as u64 + 1 },
            None if read <= self.line_limit => Line { start, end: start + read, next: start + read },
            None => Line { start, end: start + self.line_limit, next: start + self.line_limit },
        })
    }

    fn previous_line_start(&self, offset: u64) -> io::Result<u64> {
        let ends_with_newline = offset > 0 && self.byte_at(offset - 1)? == Some(NEWLINE);
        let content_end = if ends_with_newline { offset - 1 } else { offset };
        if content_end == 0 {
            return Ok(0);
        }
        let real_start = self.scan_back_for_newline(content_end)?;
        if content_end == real_start {
            return Ok(real_start);
        }
        Ok(real_start + (content_end - 1 - real_start) / self.line_limit * self.line_limit)
    }

    fn scan_back_for_newline(&self, before: u64) -> io::Result<u64> {
        let floor = before.saturating_sub(self.scan_limit);
        let mut end = before;
        while end > floor {
            let start = end.saturating_sub(BACK_CHUNK).max(floor);
            let chunk = self.read(start, end)?;
            if let Some(i) = chunk.iter().rposition(|&b| b == NEWLINE) {
                return Ok(start + i as u64 + 1);
            }
            end = start;
        }
        Ok(floor)
    }

    fn byte_at(&self, offset: u64) -> io::Result<Option<u8>> {
        let mut byte = [0];
        Ok((self.source.read_at(offset, &mut byte)? == 1).then_some(byte[0]))
    }

    fn read(&self, start: u64, end: u64) -> io::Result<Vec<u8>> {
        let mut buf = vec![0; usize::try_from(end.saturating_sub(start)).unwrap_or(usize::MAX)];
        let read = self.source.read_at(start, &mut buf)?;
        buf.truncate(read);
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pager(content: &str, width: usize, height: usize) -> Pager {
        Pager::new(Arc::new(content.as_bytes().to_vec()), width, height).unwrap_or_else(|e| panic!("{e}"))
    }

    fn numbered(count: usize) -> String {
        (1..=count).map(|i| format!("line {i}\n")).collect()
    }

    fn ok(result: io::Result<()>) {
        result.unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn shows_first_page_and_scrolls() {
        let mut view = pager(&numbered(10), 20, 3);
        assert_eq!(view.rows(), ["line 1", "line 2", "line 3"]);
        ok(view.scroll_down(2));
        assert_eq!(view.rows(), ["line 3", "line 4", "line 5"]);
        ok(view.scroll_up(1));
        assert_eq!(view.rows()[0], "line 2");
    }

    #[test]
    fn end_shows_last_page_and_stops_there() {
        let mut view = pager(&numbered(10), 20, 3);
        ok(view.end());
        assert_eq!(view.rows(), ["line 8", "line 9", "line 10"]);
        assert_eq!(view.percent(), 100);
        ok(view.scroll_down(5));
        assert_eq!(view.rows(), ["line 8", "line 9", "line 10"]);
        ok(view.page_up());
        assert_eq!(view.rows(), ["line 6", "line 7", "line 8"]);
        ok(view.home());
        assert_eq!(view.rows()[0], "line 1");
    }

    #[test]
    fn wraps_long_lines_and_scrolls_by_visual_row() {
        let mut view = pager("abcdefghij\nxy\n", 4, 2);
        assert_eq!(view.rows(), ["abcd", "efgh"]);
        ok(view.scroll_down(1));
        assert_eq!(view.rows(), ["efgh", "ij"]);
        ok(view.scroll_down(1));
        assert_eq!(view.rows(), ["ij", "xy"]);
        ok(view.scroll_up(2));
        assert_eq!(view.rows(), ["abcd", "efgh"]);
    }

    #[test]
    fn unwrapped_view_scrolls_horizontally() {
        let mut view = pager("0123456789abcdef\n", 6, 1);
        ok(view.toggle_wrap());
        assert_eq!(view.rows(), ["012345"]);
        ok(view.scroll_right());
        assert_eq!(view.rows(), ["89abcd"]);
        ok(view.scroll_left());
        assert_eq!(view.rows(), ["012345"]);
    }

    #[test]
    fn handles_files_without_trailing_newline_and_empty_files() {
        let mut view = pager("a\nb", 5, 5);
        assert_eq!(view.rows(), ["a", "b"]);
        ok(view.end());
        assert_eq!(view.rows(), ["a", "b"]);

        let empty = pager("", 5, 5);
        assert!(empty.rows().is_empty());
        assert_eq!(empty.percent(), 100);
    }

    #[test]
    fn splits_giant_lines_into_bounded_chunks() {
        let content = format!("{}\nend\n", "x".repeat(10));
        let source: Arc<dyn Source> = Arc::new(content.into_bytes());
        let mut view = Pager::with_limits(source, 3, 10, 4, 64).unwrap_or_else(|e| panic!("{e}"));
        ok(view.toggle_wrap());
        assert_eq!(view.rows(), ["xxx", "xxx", "xx", "end"]);
        ok(view.end());
        ok(view.scroll_up(1));
        assert_eq!(view.rows()[0], "xxx");
    }
}
