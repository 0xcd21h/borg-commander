use std::{io, ops::Range, path::Path, sync::OnceLock};

use two_face::{
    re_exports::syntect::{easy::HighlightLines, highlighting::FontStyle, parsing::SyntaxSet},
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
    let set = syntaxes();
    let syntax = set
        .find_syntax_by_name(language)
        .or_else(|| set.find_syntax_by_token(language))
        .unwrap_or_else(|| set.find_syntax_plain_text());
    let mut dark = HighlightLines::new(syntax, &themes()[EmbeddedThemeName::Nord]);
    let mut light = HighlightLines::new(syntax, &themes()[EmbeddedThemeName::InspiredGithub]);
    let mut rich = RichText { base, ..RichText::default() };
    for line in text.split_inclusive('\n') {
        check_cancelled(cancelled)?;
        if line.len() > MAX_HIGHLIGHT_LINE || line.as_bytes().contains(&0) {
            rich.push(line, Paint::default());
            continue;
        }
        let dark_runs = dark.highlight_line(line, set).unwrap_or_default();
        let light_runs = light.highlight_line(line, set).unwrap_or_default();
        let mut light_regions = Vec::new();
        let mut end = 0;
        for (style, text) in light_runs {
            let start = end;
            end += text.len();
            light_regions.push((start..end, style));
        }
        let mut offset = 0;
        for (style, text) in dark_runs {
            let rgb = |c: two_face::re_exports::syntect::highlighting::Color| [c.r, c.g, c.b];
            for (i, c) in text.char_indices() {
                let light = light_regions.iter().find(|(range, _)| range.contains(&(offset + i))).map(|(_, s)| s);
                rich.push(
                    &c.to_string(),
                    Paint {
                        dark: Some(rgb(style.foreground)),
                        light: light.map(|s| rgb(s.foreground)),
                        bold: style.font_style.contains(FontStyle::BOLD),
                        italic: style.font_style.contains(FontStyle::ITALIC),
                        underline: style.font_style.contains(FontStyle::UNDERLINE),
                        ..Paint::default()
                    },
                );
            }
            offset += text.len();
        }
        if offset == 0 {
            rich.push(line, Paint::default());
        }
    }
    Ok(rich)
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
