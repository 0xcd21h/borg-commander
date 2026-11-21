//! Times, Markdown and CSV as the reports write them.

use borg_i18n::tr;
use jiff::Timestamp;

const DAY: &str = "%Y-%m-%d";
const READABLE: &str = "%Y-%m-%d %H:%M UTC";
const CSV_QUOTE: char = '"';
const CSV_SPECIAL: [char; 4] = [',', '"', '\n', '\r'];
const CSV_SEPARATOR: &str = ",";
const LINE_END: &str = "\n";
const BLOCK_GAP: &str = "\n\n";
const CELL_PIPE: &str = "|";
const ESCAPED_PIPE: &str = "\\|";
const ROW_START: &str = "| ";
const ROW_END: &str = " |";
const CELL_SEPARATOR: &str = " | ";
const RULE: &str = "---";
pub(super) const LIST_SEPARATOR: &str = ", ";
pub(super) const VALUE_SEPARATOR: &str = " · ";
pub(super) const CSV_LIST_SEPARATOR: &str = " ";

fn timestamp(at: u64) -> Timestamp {
    i64::try_from(at).ok().and_then(|seconds| Timestamp::from_second(seconds).ok()).unwrap_or(Timestamp::UNIX_EPOCH)
}

/// A time as tools read it: RFC 3339 in UTC.
pub(super) fn utc(at: u64) -> String {
    timestamp(at).to_string()
}

/// A time as people read it, in UTC.
pub(super) fn readable(at: u64) -> String {
    timestamp(at).strftime(READABLE).to_string()
}

/// The day of a time, for file names.
pub(super) fn day(at: u64) -> String {
    timestamp(at).strftime(DAY).to_string()
}

/// A line of `label: value` facts in Markdown.
pub(super) fn fact(label: &str, value: impl std::fmt::Display) -> String {
    tr!(report_fact, label = label, value = value)
}

fn cell(text: &str) -> String {
    text.replace(['\n', '\r'], " ").replace(CELL_PIPE, ESCAPED_PIPE)
}

pub(super) const LINES: &str = LINE_END;

/// A Markdown table with `heads` over `rows`.
pub(super) fn table(heads: &[&str], rows: &[Vec<String>]) -> String {
    let line = |cells: Vec<String>| format!("{ROW_START}{}{ROW_END}", cells.join(CELL_SEPARATOR));
    let head = line(heads.iter().map(|head| cell(head)).collect());
    let rule = line(heads.iter().map(|_| RULE.to_owned()).collect());
    let body = rows.iter().map(|row| line(row.iter().map(|value| cell(value)).collect()));
    [head, rule].into_iter().chain(body).collect::<Vec<_>>().join(LINE_END)
}

fn csv_field(text: &str) -> String {
    if text.contains(CSV_SPECIAL) {
        let quote = CSV_QUOTE.to_string();
        format!("{CSV_QUOTE}{}{CSV_QUOTE}", text.replace(CSV_QUOTE, &quote.repeat(2)))
    } else {
        text.to_owned()
    }
}

/// CSV with a header line, each line ended with a newline.
pub(super) fn csv(heads: &[&str], rows: &[Vec<String>]) -> String {
    let line = |cells: Vec<String>| format!("{}{LINE_END}", cells.join(CSV_SEPARATOR));
    let head = line(heads.iter().map(|head| csv_field(head)).collect());
    std::iter::once(head)
        .chain(rows.iter().map(|row| line(row.iter().map(|value| csv_field(value)).collect())))
        .collect()
}

/// Paragraphs of Markdown, one blank line apart, ending with a newline.
pub(super) fn document(blocks: Vec<String>) -> String {
    let mut text = blocks.into_iter().filter(|block| !block.is_empty()).collect::<Vec<_>>().join(BLOCK_GAP);
    text.push_str(LINE_END);
    text
}

/// A value the way JSON spells it, without quotes: `fast` for Fast mode.
pub(super) fn plain(value: impl serde::Serialize) -> String {
    serde_json::to_value(value).ok().and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_default()
}

/// A JSON document, pretty and ending with a newline.
pub(super) fn json(value: &impl serde::Serialize) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    text.push_str(LINE_END);
    text
}
