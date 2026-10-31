use std::{io, ops::Range, path::Path, sync::OnceLock};

use two_face::{
    re_exports::syntect::{
        highlighting::{FontStyle, HighlightIterator, HighlightState, Highlighter, Style},
        parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet},
    },
    theme::{EmbeddedLazyThemeSet, EmbeddedThemeName},
};

const MAX_HIGHLIGHT_LINE: usize = 4096;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Paint {
    pub dark: Option<[u8; 3]>,
    pub light: Option<[u8; 3]>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub dim: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub paint: Paint,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct RichText {
    pub text: String,
    pub regions: Vec<(Range<u64>, Paint)>,
    pub base: u64,
}

impl RichText {
    pub fn push(&mut self, text: &str, paint: Paint) {
        let start = self.text.len() as u64;
        self.text.push_str(text);
        if let Some((range, _)) = self.regions.last_mut().filter(|(_, previous)| *previous == paint) {
            range.end = self.text.len() as u64;
        } else {
            self.regions.push((start..self.text.len() as u64, paint));
        }
    }

    /// The offset just past the text.
    pub fn end(&self) -> u64 {
        self.base + self.text.len() as u64
    }

    pub fn at(&self, offset: u64) -> Paint {
        let i = self.regions.partition_point(|(range, _)| range.end <= offset);
        self.regions.get(i).filter(|(range, _)| range.contains(&offset)).map_or(Paint::default(), |(_, paint)| *paint)
    }
}

pub(crate) fn syntaxes() -> &'static SyntaxSet {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAXES.get_or_init(two_face::syntax::extra_newlines)
}

fn themes() -> &'static EmbeddedLazyThemeSet {
    static THEMES: OnceLock<EmbeddedLazyThemeSet> = OnceLock::new();
    THEMES.get_or_init(two_face::theme::extra)
}

/// The dark and the light theme, ready to colour parsed text.
fn highlighters() -> &'static [Highlighter<'static>; 2] {
    static HIGHLIGHTERS: OnceLock<[Highlighter<'static>; 2]> = OnceLock::new();
    HIGHLIGHTERS.get_or_init(|| {
        [EmbeddedThemeName::Nord, EmbeddedThemeName::InspiredGithub].map(|name| Highlighter::new(&themes()[name]))
    })
}

fn syntax(language: &str) -> &'static SyntaxReference {
    let set = syntaxes();
    set.find_syntax_by_name(language)
        .or_else(|| set.find_syntax_by_token(language))
        .unwrap_or_else(|| set.find_syntax_plain_text())
}

/// Where colouring stands at the start of a line: what the parser is inside and the styles both
/// themes have in force. Colouring carries on from it without reading the text above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resume {
    parse: ParseState,
    dark: HighlightState,
    light: HighlightState,
}

impl Resume {
    pub(crate) fn new(language: &str) -> Self {
        let [dark, light] = highlighters();
        Self {
            parse: ParseState::new(syntax(language)),
            dark: HighlightState::new(dark, ScopeStack::new()),
            light: HighlightState::new(light, ScopeStack::new()),
        }
    }
}

pub(crate) fn syntax_name(path: &Path, first_line: &str) -> Option<String> {
    let set = syntaxes();
    let name = path.file_name()?.to_str()?;
    set.find_syntax_by_extension(name)
        .or_else(|| path.extension().and_then(|ext| ext.to_str()).and_then(|ext| set.find_syntax_by_extension(ext)))
        .or_else(|| set.find_syntax_by_first_line(first_line))
        .map(|syntax| syntax.name.clone())
}

pub(crate) fn check_cancelled(cancelled: &impl Fn() -> bool) -> io::Result<()> {
    if cancelled() { Err(io::Error::new(io::ErrorKind::Interrupted, "document loading cancelled")) } else { Ok(()) }
}

pub(crate) fn highlight(text: &str, language: &str, base: u64, cancelled: &impl Fn() -> bool) -> io::Result<RichText> {
    check_cancelled(cancelled)?;
    let mut rich = RichText { base, ..RichText::default() };
    paint(text.as_bytes(), &mut Resume::new(language), &mut rich, &mut |_, _| {}, cancelled)?;
    Ok(rich)
}

/// Colours `text`, which starts a line, onto the end of `rich`, carrying on from `resume` and
/// leaving it at the end of `text`. `mark` hears the offset and the state at the start of each line.
/// Lines that are too long, hold a NUL or are not UTF-8 keep their length and no colour.
pub(crate) fn paint(
    text: &[u8],
    resume: &mut Resume,
    rich: &mut RichText,
    mark: &mut impl FnMut(u64, &Resume),
    cancelled: &impl Fn() -> bool,
) -> io::Result<()> {
    let set = syntaxes();
    let [dark, light] = highlighters();
    for line in text.split_inclusive(|byte| *byte == b'\n') {
        check_cancelled(cancelled)?;
        mark(rich.end(), resume);
        let Ok(line) = std::str::from_utf8(line) else {
            // Only its length matters, so that the offsets after it stay where they are.
            rich.push(&" ".repeat(line.len()), Paint::default());
            continue;
        };
        if line.len() > MAX_HIGHLIGHT_LINE || line.contains('\0') {
            rich.push(line, Paint::default());
            continue;
        }
        let Ok(ops) = resume.parse.parse_line(line, set) else {
            rich.push(line, Paint::default());
            continue;
        };
        let dark: Vec<_> = HighlightIterator::new(&mut resume.dark, &ops, line, dark).collect();
        let light: Vec<_> = HighlightIterator::new(&mut resume.light, &ops, line, light).collect();
        push_line(rich, line, &dark, &light);
    }
    Ok(())
}

/// Adds a line coloured by both themes, split wherever either colour changes.
fn push_line(rich: &mut RichText, line: &str, dark: &[(Style, &str)], light: &[(Style, &str)]) {
    let mut lights = light
        .iter()
        .scan(0, |end, (style, text)| {
            *end += text.len();
            Some((*end, *style))
        })
        .peekable();
    let rgb = |c: two_face::re_exports::syntect::highlighting::Color| [c.r, c.g, c.b];
    let mut at = 0;
    for (style, text) in dark {
        let end = at + text.len();
        while at < end {
            while lights.next_if(|(light_end, _)| *light_end <= at).is_some() {}
            let (cut, light) =
                lights.peek().map_or((end, None), |(light_end, light)| ((*light_end).min(end), Some(light)));
            let paint = Paint {
                dark: Some(rgb(style.foreground)),
                light: light.map(|s| rgb(s.foreground)),
                bold: style.font_style.contains(FontStyle::BOLD),
                italic: style.font_style.contains(FontStyle::ITALIC),
                underline: style.font_style.contains(FontStyle::UNDERLINE),
                ..Paint::default()
            };
            rich.push(&line[at..cut], paint);
            at = cut;
        }
    }
    if at == 0 {
        rich.push(line, Paint::default());
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn cancelled_highlighting_stops_between_lines() {
        let polls = Cell::new(0);
        let cancelled = || {
            polls.set(polls.get() + 1);
            polls.get() > 3
        };
        let result = highlight(&"let value = 42;\n".repeat(100), "Rust", 0, &cancelled);
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert_eq!(polls.get(), 4);
    }
}
