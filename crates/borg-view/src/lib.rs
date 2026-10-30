mod markdown;
mod pager;
mod paint;
mod source;
mod text;

pub use pager::{MAX_LINE, Pager};
pub use paint::{Paint, Run};
pub use source::{FileSource, Source};

/// The language Borg highlights a file as, from its name or else its first line.
pub fn language_of(path: &std::path::Path, first_line: &str) -> Option<String> {
    paint::syntax_name(path, first_line)
}

/// Colours for `text` in `language`: the runs of each line, line breaks left out.
pub fn highlight_lines(text: &str, language: &str) -> Vec<Vec<Run>> {
    let Ok(rich) = paint::highlight(text, language, 0, &|| false) else { return Vec::new() };
    let mut lines = vec![Vec::new()];
    for (range, paint) in &rich.regions {
        let piece = &rich.text[range.start as usize..range.end as usize];
        for (i, part) in piece.split('\n').enumerate() {
            if i > 0 {
                lines.push(Vec::new());
            }
            if let (false, Some(line)) = (part.is_empty(), lines.last_mut()) {
                line.push(Run { text: part.into(), paint: *paint });
            }
        }
    }
    lines
}
