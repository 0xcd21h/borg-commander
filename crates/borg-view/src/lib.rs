mod markdown;
mod pager;
mod paint;
pub mod pretty;
mod search;
mod source;
mod text;

pub use pager::{Format, MAX_LINE, Pager};
pub use paint::{Mark, Paint, Run};
pub use search::{Count, MAX_COUNT, Query, Search};
pub use source::{FileSource, Source};

/// The language Borg highlights a file as, from its name or else its first line.
pub fn language_of(path: &std::path::Path, first_line: &str) -> Option<String> {
    paint::syntax_name(path, first_line)
}

/// Where colouring stands at the start of a line. Colouring a line carries it to the next, so
/// a line coloured from the same state as before colours the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineState(paint::Resume);

impl LineState {
    /// The state at the start of a text in `language`.
    pub fn new(language: &str) -> Self {
        Self(paint::Resume::new(language))
    }
}

/// The runs of `line`, given without its break, coloured on from `state`, which it leaves at the
/// start of the next line.
pub fn highlight_line(line: &str, state: &mut LineState) -> Vec<Run> {
    let mut rich = paint::RichText::default();
    if paint::paint(format!("{line}\n").as_bytes(), &mut state.0, &mut rich, &mut |_, _| {}, &|| false).is_err() {
        return Vec::new();
    }
    rich.regions
        .iter()
        .map(|(range, paint)| Run {
            text: rich.text[range.start as usize..range.end as usize].replace('\n', ""),
            paint: *paint,
            mark: Mark::None,
        })
        .filter(|run| !run.text.is_empty())
        .collect()
}
