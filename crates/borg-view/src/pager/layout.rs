//! Laying the file out in screen rows: wrapping, painting syntax as it goes, and finding where lines start in either direction.

use super::{
    BACK_CHUNK, Cell, FIRST_READ, Line, MAX_RESUMES, NEWLINE, Pager, Position, RESUME_EVERY, SYNTAX_AHEAD,
    SYNTAX_CONTEXT, SYNTAX_KEEP, TAB_WIDTH,
};
use crate::{
    Mark, Paint, Run,
    paint::{self, RichText},
    text,
};
use std::{io, sync::Arc};
use unicode_width::UnicodeWidthChar;

impl Pager {
    pub(super) fn decorate(&mut self) -> io::Result<()> {
        if let Some(rendered) = self.document.as_ref().and_then(|document| document.rendered.clone())
            && self.formatted
        {
            self.decoration = Some(rendered);
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

    pub(super) fn resumes(&self) -> &[(u64, paint::Resume)] {
        self.syntax.as_ref().map_or(&[], |syntax| &syntax.resumes)
    }

    pub(super) fn resume_at(&self, offset: u64) -> Option<&paint::Resume> {
        let resumes = self.resumes();
        let i = resumes.binary_search_by_key(&offset, |(at, _)| *at).ok()?;
        Some(&resumes[i].1)
    }

    /// Keeps the places a colouring of `start..end` found in place of those it went past, as far
    /// apart as `RESUME_EVERY` but for the end of the colours, and no more than `MAX_RESUMES`.
    pub(super) fn keep_resumes(&mut self, start: u64, end: u64, found: Vec<(u64, paint::Resume)>) {
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

    /// The cells `line` shows as, with their colours and whether they are part of a match.
    pub(super) fn cells(&self, line: Line) -> io::Result<Vec<Cell>> {
        let bytes = self.read(line.start, line.end)?;
        let matches = self.matches.as_ref().map(|search| search.in_line(&bytes)).unwrap_or_default();
        let current = self.current.clone();
        let text = String::from_utf8_lossy(&bytes);
        let text = text.strip_suffix('\r').unwrap_or(&text);
        let mut cells = Vec::new();
        let mut column = 0;
        let mut next = 0;
        for (index, c) in text.char_indices() {
            let offset = line.start + index as u64;
            let paint = self
                .decoration
                .as_ref()
                .filter(|rich| offset >= rich.base)
                .map_or(Paint::default(), |rich| rich.at(offset - rich.base));
            while matches.get(next).is_some_and(|found| found.end <= index) {
                next += 1;
            }
            let mark = if current.as_ref().is_some_and(|current| current.contains(&offset)) {
                Mark::Current
            } else if matches.get(next).is_some_and(|found| found.contains(&index)) {
                Mark::Match
            } else {
                Mark::None
            };
            if c == '\t' {
                let spaces = TAB_WIDTH - column % TAB_WIDTH;
                cells.extend(std::iter::repeat_n(Cell { c: ' ', width: 1, offset, paint, mark }, spaces));
                column += spaces;
            } else {
                let c = if c.is_control() { '·' } else { c };
                let width = c.width().unwrap_or(0);
                cells.push(Cell { c, width, offset, paint, mark });
                column += width;
            }
        }
        Ok(cells)
    }

    pub(super) fn rich_layout(&self, line: Line) -> io::Result<Vec<Vec<Run>>> {
        let mut rows = vec![Vec::<Run>::new()];
        let mut used = 0;
        let mut column = 0;
        for Cell { c, width, paint, mark, .. } in self.cells(line)? {
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
            if let Some(last) = row.last_mut().filter(|last| last.paint == paint && last.mark == mark) {
                last.text.push(c);
            } else {
                row.push(Run { text: c.to_string(), paint, mark });
            }
            used += width;
        }
        Ok(rows)
    }

    pub(super) fn layout(&self, line: Line) -> io::Result<Vec<String>> {
        let cells = text::display(&self.read(line.start, line.end)?);
        Ok(if self.wrap { text::wrap(&cells, self.width) } else { vec![text::slice(&cells, self.column, self.width)] })
    }

    pub(super) fn row_count(&self, line: Line) -> io::Result<usize> {
        if self.wrap { self.layout(line).map(|rows| rows.len()) } else { Ok(1) }
    }

    pub(super) fn next_position(&self, position: Position) -> io::Result<Option<Position>> {
        let line = self.line_at(position.line)?;
        if position.row + 1 < self.row_count(line)? {
            return Ok(Some(Position { row: position.row + 1, ..position }));
        }
        Ok((line.next < self.len()).then_some(Position { line: line.next, row: 0 }))
    }

    pub(super) fn previous_position(&self, position: Position) -> io::Result<Option<Position>> {
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

    pub(super) fn line_at(&self, start: u64) -> io::Result<Line> {
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

    pub(super) fn previous_line_start(&self, offset: u64) -> io::Result<u64> {
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

    pub(super) fn scan_back_for_newline(&self, before: u64) -> io::Result<u64> {
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

    pub(super) fn byte_at(&self, offset: u64) -> io::Result<Option<u8>> {
        paint::check_cancelled(&|| self.source.is_cancelled())?;
        let mut byte = [0];
        Ok((self.source.read_at(offset, &mut byte)? == 1).then_some(byte[0]))
    }

    pub(super) fn read(&self, start: u64, end: u64) -> io::Result<Vec<u8>> {
        paint::check_cancelled(&|| self.source.is_cancelled())?;
        let mut buf = vec![0; usize::try_from(end.saturating_sub(start)).unwrap_or(usize::MAX)];
        let read = self.source.read_at(start, &mut buf)?;
        buf.truncate(read);
        Ok(buf)
    }
}
