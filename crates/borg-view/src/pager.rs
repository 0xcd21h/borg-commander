use std::{fmt, io, ops::Range, path::Path, sync::Arc};

use crate::{
    Mark, Paint, Run, Search, Source,
    paint::{self, RichText},
};

mod layout;
mod scroll;

const MARKDOWN_LIMIT: u64 = 2 * 1024 * 1024;
/// The largest JSON laid out to read, held whole in memory with its layout.
const JSON_LIMIT: u64 = 16 * 1024 * 1024;
/// What a level of JSON is indented by, as jq and browsers lay it out.
const JSON_INDENT: &str = "  ";
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

/// A row on screen: the offset its line starts at, and which of the line's rows it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Position {
    line: u64,
    row: usize,
}

/// One cell of a laid out line, and the offset of the character it shows.
#[derive(Debug, Clone, Copy)]
struct Cell {
    c: char,
    width: usize,
    offset: u64,
    paint: Paint,
    mark: Mark,
}

/// Documents shown formatted, which `toggle_source` switches to their source and back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Markdown,
    /// JSON, or JSON Lines, laid out one value to a line.
    Json,
}

#[derive(Clone)]
struct Formatted {
    format: Format,
    original: Arc<dyn Source>,
    text: Arc<Vec<u8>>,
    /// The colours Markdown is rendered in; JSON is coloured as its language.
    rendered: Option<Arc<RichText>>,
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
    document: Option<Box<Formatted>>,
    formatted: bool,
    after_page: Option<Position>,
    /// The search whose matches the page marks, and the match stepped to, marked on its own.
    matches: Option<Search>,
    current: Option<Range<u64>>,
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
            document: None,
            formatted: false,
            after_page: None,
            matches: None,
            current: None,
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
        let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or_default().to_ascii_lowercase();
        let (format, limit) = match extension.as_str() {
            "md" | "markdown" | "mdown" | "mkd" => (Some(Format::Markdown), MARKDOWN_LIMIT),
            "json" | "jsonl" | "ndjson" | "geojson" | "har" | "webmanifest" => (Some(Format::Json), JSON_LIMIT),
            _ => (None, 0),
        };
        if let Some(format) = format.filter(|_| !binary && source.len() <= limit) {
            let bytes = pager.read(0, source.len())?;
            let text = String::from_utf8_lossy(&bytes);
            let cancelled = || source.is_cancelled();
            let (text, rendered) = match format {
                Format::Markdown => {
                    let rendered = crate::markdown::render(&text, &cancelled)?;
                    (rendered.text.clone(), Some(Arc::new(rendered)))
                }
                Format::Json => {
                    let lines = matches!(extension.as_str(), "jsonl" | "ndjson");
                    let go_on = || !source.is_cancelled();
                    let laid_out = if lines {
                        crate::pretty::json_lines(&text, JSON_INDENT, &go_on)
                    } else {
                        crate::pretty::json(&text, JSON_INDENT, &go_on)
                    };
                    match laid_out {
                        None => return Err(io::Error::new(io::ErrorKind::Interrupted, "document loading cancelled")),
                        // Not JSON after all: it shows as it is.
                        Some(Err(_)) => (String::new(), None),
                        Some(Ok(text)) => (text, None),
                    }
                }
            };
            if rendered.is_some() || !text.is_empty() {
                let text = Arc::new(text.into_bytes());
                pager.source = text.clone();
                let top = pager.top;
                pager.document = Some(Box::new(Formatted {
                    format,
                    original: source,
                    text,
                    rendered,
                    raw_top: top,
                    formatted_top: top,
                }));
                pager.formatted = true;
            }
        }
        pager.refresh()?;
        Ok(pager)
    }

    pub fn rich_rows(&self) -> &[Vec<Run>] {
        &self.rich_rows
    }

    /// How the document shows formatted, when it can.
    pub fn format(&self) -> Option<Format> {
        self.document.as_ref().map(|document| document.format)
    }

    pub fn formatted(&self) -> bool {
        self.formatted
    }

    pub fn language(&self) -> Option<&str> {
        self.syntax.as_ref().map(|syntax| syntax.language.as_str())
    }

    pub fn toggle_source(&mut self) -> io::Result<()> {
        let Some(document) = self.document.as_mut() else { return Ok(()) };
        if self.formatted {
            document.formatted_top = self.top;
            self.top = document.raw_top;
            self.source = document.original.clone();
        } else {
            document.raw_top = self.top;
            self.top = document.formatted_top;
            self.source = document.text.clone();
        }
        self.formatted = !self.formatted;
        self.decoration = None;
        self.current = None;
        if let Some(syntax) = self.syntax.as_mut() {
            syntax.resumes.clear();
        }
        self.refresh()
    }

    pub fn rows(&self) -> &[String] {
        &self.rows
    }

    /// What the pager reads, for a search through all of it.
    pub fn source(&self) -> Arc<dyn Source> {
        self.source.clone()
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
}

#[cfg(test)]
mod tests;
