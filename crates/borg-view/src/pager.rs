use std::{fmt, io, path::Path, sync::Arc};

use unicode_width::UnicodeWidthChar;

use crate::{
    Paint, Run, Source,
    paint::{self, RichText},
    text,
};

const MARKDOWN_LIMIT: u64 = 2 * 1024 * 1024;
const SYNTAX_CONTEXT: u64 = 256 * 1024;
const SYNTAX_AHEAD: u64 = 4 * 1024;
/// How far apart the places colouring can carry on from are kept.
const RESUME_EVERY: u64 = 8 * 1024;
const MAX_RESUMES: usize = 4096;
/// How much coloured text is kept before colouring starts afresh near the page.
const SYNTAX_KEEP: u64 = 1024 * 1024;
const TAB_WIDTH: usize = 8;

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
struct Markdown {
    original: Arc<dyn Source>,
    rendered: Arc<RichText>,
    raw_top: Position,
    formatted_top: Position,
}

/// The language a document is coloured as, and the places colouring can carry on from, by
/// offset: every so often and at the end of the colours, so that moving down colours only the
/// new text.
#[derive(Clone)]
struct Syntax {
    language: String,
    resumes: Vec<(u64, paint::Resume)>,
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
    rich_rows: Vec<Vec<Run>>,
    syntax: Option<Box<Syntax>>,
    decoration: Option<Arc<RichText>>,
    markdown: Option<Markdown>,
    formatted: bool,
    after_page: Option<Position>,
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
        let mut pager = Self::blank(source, width, height, line_limit, scan_limit);
        pager.refresh()?;
        Ok(pager)
    }

    fn blank(source: Arc<dyn Source>, width: usize, height: usize, line_limit: u64, scan_limit: u64) -> Self {
        Self {
            source,
            line_limit: line_limit.max(1),
            scan_limit: scan_limit.max(line_limit),
            top: Position { line: 0, row: 0 },
            wrap: true,
            column: 0,
            width,
            height,
            rows: Vec::new(),
            rich_rows: Vec::new(),
            syntax: None,
            decoration: None,
            markdown: None,
            formatted: false,
            after_page: None,
        }
    }

    pub fn for_path(source: Arc<dyn Source>, path: &Path, width: usize, height: usize) -> io::Result<Self> {
        let mut pager = Self::blank(source.clone(), width, height, MAX_LINE, BACK_SCAN_LIMIT);
        let head = pager.read(0, FIRST_READ)?;
        let binary = head.contains(&0);
        pager.syntax = if binary {
            None
        } else {
            paint::syntax_name(path, String::from_utf8_lossy(&head).lines().next().unwrap_or_default())
                .map(|language| Box::new(Syntax { language, resumes: Vec::new() }))
        };
        let markdown = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdown" | "mkd"));
        if markdown && !binary && source.len() <= MARKDOWN_LIMIT {
            let bytes = pager.read(0, source.len())?;
            let rendered =
                Arc::new(crate::markdown::render(&String::from_utf8_lossy(&bytes), &|| source.is_cancelled())?);
            pager.source = Arc::new(rendered.text.as_bytes().to_vec());
            pager.markdown =
                Some(Markdown { original: source, rendered, raw_top: pager.top, formatted_top: pager.top });
            pager.formatted = true;
        }
        pager.refresh()?;
        Ok(pager)
    }

    pub fn rich_rows(&self) -> &[Vec<Run>] {
        &self.rich_rows
    }

    pub fn markdown(&self) -> bool {
        self.markdown.is_some()
    }

    pub fn formatted(&self) -> bool {
        self.formatted
    }

    pub fn language(&self) -> Option<&str> {
        self.syntax.as_ref().map(|syntax| syntax.language.as_str())
    }

    pub fn toggle_source(&mut self) -> io::Result<()> {
        let Some(markdown) = self.markdown.as_mut() else { return Ok(()) };
        if self.formatted {
            markdown.formatted_top = self.top;
            self.top = markdown.raw_top;
            self.source = markdown.original.clone();
        } else {
            markdown.raw_top = self.top;
            self.top = markdown.formatted_top;
            self.source = Arc::new(markdown.rendered.text.as_bytes().to_vec());
        }
        self.formatted = !self.formatted;
        self.decoration = None;
        if let Some(syntax) = self.syntax.as_mut() {
            syntax.resumes.clear();
        }
        self.refresh()
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
        if self.width == width && self.height == height {
            return Ok(());
        }
        let bottom = self.at_end() && self.top != (Position { line: 0, row: 0 });
        let anchor = if self.wrap && self.top.row > 0 && self.width != width {
            Some(
                self.layout(self.line_at(self.top.line)?)?
                    .iter()
                    .take(self.top.row)
                    .map(|row| row.chars().count())
                    .sum::<usize>(),
            )
        } else {
            None
        };
        self.width = width;
        self.height = height;
        if bottom {
            self.top = self.last_top()?;
        } else if let Some(mut anchor) = anchor {
            for (row, text) in self.layout(self.line_at(self.top.line)?)?.iter().enumerate() {
                self.top.row = row;
                let count = text.chars().count();
                if anchor < count {
                    break;
                }
                anchor -= count;
            }
        }
        self.refresh()
    }

    pub fn toggle_wrap(&mut self) -> io::Result<()> {
        self.wrap = !self.wrap;
        self.column = 0;
        self.top.row = 0;
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

    fn page(&self) -> usize {
        self.height.saturating_sub(1).max(1)
    }

    fn at_end(&self) -> bool {
        self.after_page.is_none()
    }

    fn refresh(&mut self) -> io::Result<()> {
        self.decorate()?;
        let mut rows = Vec::with_capacity(self.height);
        let mut rich_rows = Vec::with_capacity(self.height);
        let mut position = (!self.is_empty()).then_some(self.top);
        while rows.len() < self.height {
            let Some(current) = position else { break };
            let line = self.line_at(current.line)?;
            let layout = self.rich_layout(line)?;
            let shown = (layout.len().saturating_sub(current.row)).min(self.height - rows.len());
            let more = current.row + shown < layout.len();
            for rich in layout.into_iter().skip(current.row).take(shown) {
                rows.push(rich.iter().map(|run| run.text.as_str()).collect());
                rich_rows.push(rich);
            }
            position = if more {
                Some(Position { line: current.line, row: current.row + shown })
            } else {
                (line.next < self.len()).then_some(Position { line: line.next, row: 0 })
            };
        }
        self.rows = rows;
        self.rich_rows = rich_rows;
        self.after_page = position;
        Ok(())
    }

    fn decorate(&mut self) -> io::Result<()> {
        if self.formatted {
            self.decoration = self.markdown.as_ref().map(|markdown| markdown.rendered.clone());
            return Ok(());
        }
        let Some(language) = self.syntax.as_ref().map(|syntax| syntax.language.clone()) else { return Ok(()) };
        let mut end = self.top.line;
        let mut remaining = self.height;
        let mut row = self.top.row;
        while remaining > 0 && end < self.len() {
            let line = self.line_at(end)?;
            remaining = remaining.saturating_sub(self.row_count(line)?.saturating_sub(row));
            end = line.next;
            row = 0;
        }
        let top = self.top.line;
        if self.decoration.as_ref().is_some_and(|rich| rich.base <= top && rich.end() >= end) {
            return Ok(());
        }
        // Carry on from the end of the colours when the page starts among them; otherwise start
        // from the nearest place above the page, or with no context some way above it.
        let carry = self
            .decoration
            .as_ref()
            .filter(|rich| rich.base <= top && top <= rich.end() && rich.end() - rich.base < SYNTAX_KEEP)
            .and_then(|rich| Some((rich.end(), self.resume_at(rich.end())?.clone())));
        let above =
            || self.resumes().iter().rev().find(|(at, _)| *at <= top).filter(|(at, _)| top - at <= SYNTAX_CONTEXT);
        let carried = carry.is_some();
        let (start, mut resume, cold) = match carry.or_else(|| above().cloned()) {
            Some((at, resume)) => (at, resume, false),
            None => (top.saturating_sub(SYNTAX_CONTEXT), paint::Resume::new(&language), true),
        };
        let to = end.saturating_add(SYNTAX_AHEAD).min(self.len());
        let bytes = self.read(start, to)?;
        // A cold start begins at the next line; colouring stops at the end of a line, where it can
        // carry on from, unless that would leave part of the page out.
        let skip = if cold && start > 0 { bytes.iter().position(|b| *b == NEWLINE).map_or(0, |i| i + 1) } else { 0 };
        let keep = if to < self.len() {
            bytes.iter().rposition(|b| *b == NEWLINE).map(|i| i + 1).filter(|&i| i > skip && start + i as u64 >= end)
        } else {
            None
        };
        let bytes = &bytes[skip..keep.unwrap_or(bytes.len()).max(skip)];
        let base = start + skip as u64;
        let mut rich = if carried {
            self.decoration.take().map(Arc::unwrap_or_clone).unwrap_or_default()
        } else {
            RichText { base, ..RichText::default() }
        };
        let mut marks = Vec::new();
        let mut last = base;
        let mut mark = |at: u64, resume: &paint::Resume| {
            if at >= last + RESUME_EVERY {
                marks.push((at, resume.clone()));
                last = at;
            }
        };
        let cancelled = || self.source.is_cancelled();
        paint::paint(bytes, &mut resume, &mut rich, &mut mark, &cancelled)?;
        marks.push((rich.end(), resume));
        self.keep_resumes(base, rich.end(), marks);
        self.decoration = Some(Arc::new(rich));
        Ok(())
    }

    fn resumes(&self) -> &[(u64, paint::Resume)] {
        self.syntax.as_ref().map_or(&[], |syntax| &syntax.resumes)
    }

    fn resume_at(&self, offset: u64) -> Option<&paint::Resume> {
        let resumes = self.resumes();
        let i = resumes.binary_search_by_key(&offset, |(at, _)| *at).ok()?;
        Some(&resumes[i].1)
    }

    /// Keeps the places a colouring of `start..end` found in place of those it went past, as far
    /// apart as `RESUME_EVERY` but for the end of the colours, and no more than `MAX_RESUMES`.
    fn keep_resumes(&mut self, start: u64, end: u64, found: Vec<(u64, paint::Resume)>) {
        let Some(syntax) = self.syntax.as_mut() else { return };
        let resumes = std::mem::take(&mut syntax.resumes);
        let (before, rest): (Vec<_>, Vec<_>) = resumes.into_iter().partition(|(at, _)| *at <= start);
        let after = rest.into_iter().filter(|(at, _)| *at > end);
        let mut kept: Vec<(u64, paint::Resume)> = Vec::new();
        for (at, resume) in before.into_iter().chain(found).chain(after) {
            if at == end || kept.last().is_none_or(|(last, _)| at >= last + RESUME_EVERY) {
                kept.push((at, resume));
            }
        }
        if kept.len() > MAX_RESUMES {
            kept = kept
                .into_iter()
                .enumerate()
                .filter(|(i, (at, _))| i % 2 == 0 || *at == end)
                .map(|(_, kept)| kept)
                .collect();
        }
        syntax.resumes = kept;
    }

    fn rich_layout(&self, line: Line) -> io::Result<Vec<Vec<Run>>> {
        let bytes = self.read(line.start, line.end)?;
        let text = String::from_utf8_lossy(&bytes);
        let text = text.strip_suffix('\r').unwrap_or(&text);
        let mut cells = Vec::new();
        let mut column = 0;
        for (index, c) in text.char_indices() {
            let offset = line.start + index as u64;
            let paint = self
                .decoration
                .as_ref()
                .filter(|rich| offset >= rich.base)
                .map_or(Paint::default(), |rich| rich.at(offset - rich.base));
            if c == '\t' {
                let spaces = TAB_WIDTH - column % TAB_WIDTH;
                cells.extend(std::iter::repeat_n((' ', 1, paint), spaces));
                column += spaces;
            } else {
                let c = if c.is_control() { '·' } else { c };
                let width = c.width().unwrap_or(0);
                cells.push((c, width, paint));
                column += width;
            }
        }
        let mut rows = vec![Vec::<Run>::new()];
        let mut used = 0;
        let mut column = 0;
        for (c, width, paint) in cells {
            let start = column;
            column += width;
            if !self.wrap && (start < self.column || column > self.column.saturating_add(self.width)) {
                continue;
            }
            if self.wrap && used + width > self.width.max(1) && used > 0 {
                rows.push(Vec::new());
                used = 0;
            }
            let row = rows.last_mut().unwrap();
            if let Some(last) = row.last_mut().filter(|last| last.paint == paint) {
                last.text.push(c);
            } else {
                row.push(Run { text: c.to_string(), paint });
            }
            used += width;
        }
        Ok(rows)
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
        paint::check_cancelled(&|| self.source.is_cancelled())?;
        let mut byte = [0];
        Ok((self.source.read_at(offset, &mut byte)? == 1).then_some(byte[0]))
    }

    fn read(&self, start: u64, end: u64) -> io::Result<Vec<u8>> {
        paint::check_cancelled(&|| self.source.is_cancelled())?;
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
    fn resizing_keeps_the_wrapped_position_and_the_bottom_of_a_document() {
        let mut view = pager("abcdefghij\nklmnopqrst\nuvwxyz\n", 2, 2);
        ok(view.scroll_down(3));
        assert_eq!(view.rows()[0], "gh");
        ok(view.resize(4, 2));
        assert_eq!(view.rows()[0], "efgh");
        ok(view.resize(2, 3));
        assert_eq!(view.rows()[0], "ef");
        ok(view.end());
        ok(view.resize(4, 2));
        assert_eq!(view.rows().last().unwrap(), "yz");
        assert_eq!(view.percent(), 100);
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

    #[test]
    fn colours_reached_by_paging_match_colouring_the_whole_file() {
        // A comment and a string that span lines, so colours depend on the text above the page.
        let chunk = "/* a comment\n   over lines */\nfn main() {\n    let text = \"two\n lines\";\n}\n";
        let text = chunk.repeat(600);
        let source: Arc<dyn Source> = Arc::new(text.clone().into_bytes());
        let mut view = Pager::for_path(source.clone(), Path::new("main.rs"), 40, 12).unwrap_or_else(|e| panic!("{e}"));
        let mut whole = Pager::for_path(source, Path::new("main.rs"), 40, 12).unwrap_or_else(|e| panic!("{e}"));
        whole.decoration =
            Some(Arc::new(paint::highlight(&text, "Rust", 0, &|| false).unwrap_or_else(|e| panic!("{e}"))));
        let mut same = |view: &Pager| {
            whole.top = view.top;
            ok(whole.refresh());
            assert_eq!(view.rich_rows(), whole.rich_rows(), "at {:?}", view.top);
        };
        while !view.at_end() {
            ok(view.page_down());
            same(&view);
        }
        assert!(view.resumes().len() <= text.len() / RESUME_EVERY as usize + 2, "{} places", view.resumes().len());
        ok(view.home());
        ok(view.end());
        same(&view);
        for _ in 0..40 {
            ok(view.page_up());
            same(&view);
        }
    }

    #[test]
    fn paging_down_far_into_a_file_carries_colouring_on_instead_of_starting_again_above() {
        let text = numbered(40_000);
        let at = text.find("\nline 30000\n").unwrap() as u64 + 1;
        let source: Arc<dyn Source> = Arc::new(text.into_bytes());
        let mut view = Pager::for_path(source, Path::new("notes.txt"), 40, 12).unwrap_or_else(|e| panic!("{e}"));
        assert!(view.language().is_some());
        view.top = Position { line: at, row: 0 };
        ok(view.refresh());
        let base = view.decoration.as_ref().unwrap().base;
        assert!(at - base <= SYNTAX_CONTEXT, "a jump colours some way above the page");
        for _ in 0..50 {
            ok(view.page_down());
        }
        assert_eq!(view.decoration.as_ref().unwrap().base, base, "moving down colours only the new text");
    }
}
