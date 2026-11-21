//! Prose in an approval dialog, wrapped between words. Scripts written without spaces, such as
//! Chinese, break between characters, and a word wider than the row is cut where the row ends.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The cells a character of a script written without spaces takes.
const WIDE: usize = 2;
/// Marks that stay with the character before them, so no row starts with one.
const CLOSING: &str = "，。、；：？！）」』】》〉〕”’…";
/// Marks that stay with the character after them, so no row ends with one.
const OPENING: &str = "（「『【《〈〔“‘";

/// What a line is made of, for wrapping.
#[derive(Debug, PartialEq, Eq)]
enum Piece {
    /// Spaces between words, dropped where a row breaks.
    Gap(String),
    /// A word, or one character of a script written without spaces with the marks it keeps.
    Word(String),
}

/// `line` in rows at most `width` cells wide. Spaces at its start stay at the start of every
/// row it takes, so an indented line stays indented.
pub(super) fn wrap(line: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let text = line.trim_start_matches(' ');
    let indent = &line[..line.len() - text.len()];
    let indent = if indent.width() * 2 <= width { indent } else { "" };
    let mut rows = Vec::new();
    let mut row = indent.to_owned();
    let mut gap = String::new();
    for piece in pieces(text) {
        let word = match piece {
            Piece::Gap(spaces) => {
                gap = spaces;
                continue;
            }
            Piece::Word(word) => word,
        };
        let fresh = row.len() == indent.len();
        let lead = if fresh { "" } else { gap.as_str() };
        if row.width() + lead.width() + word.width() <= width {
            row.push_str(lead);
            row.push_str(&word);
        } else {
            if !fresh {
                rows.push(std::mem::replace(&mut row, indent.to_owned()));
            }
            for (index, part) in super::wrap(&word, width - indent.width()).into_iter().enumerate() {
                if index > 0 {
                    rows.push(std::mem::replace(&mut row, indent.to_owned()));
                }
                row.push_str(&part);
            }
        }
        gap.clear();
    }
    rows.push(row);
    rows
}

/// Splits text into words and the gaps between them; each character of a script written
/// without spaces is a word of its own, with the punctuation that keeps to it.
fn pieces(text: &str) -> Vec<Piece> {
    let mut pieces: Vec<Piece> = Vec::new();
    let mut opening = String::new();
    for c in text.chars() {
        if c == ' ' {
            match pieces.last_mut() {
                Some(Piece::Gap(spaces)) => spaces.push(c),
                _ => pieces.push(Piece::Gap(c.to_string())),
            }
            continue;
        }
        if OPENING.contains(c) {
            opening.push(c);
            continue;
        }
        let wide = c.width() == Some(WIDE);
        let joins = match pieces.last_mut() {
            Some(Piece::Word(word)) if CLOSING.contains(c) || (!wide && !ends_wide(word) && opening.is_empty()) => {
                word.push(c);
                true
            }
            _ => false,
        };
        if !joins {
            pieces.push(Piece::Word(std::mem::take(&mut opening) + &c.to_string()));
        }
    }
    if !opening.is_empty() {
        pieces.push(Piece::Word(opening));
    }
    pieces
}

/// Whether a word ends in a character of a script written without spaces, after which a row
/// can break.
fn ends_wide(word: &str) -> bool {
    word.chars().rev().find(|c| !CLOSING.contains(*c)).is_some_and(|c| c.width() == Some(WIDE))
}

#[cfg(test)]
mod tests {
    use super::wrap;

    #[test]
    fn prose_breaks_between_words_and_keeps_each_word_whole() {
        let sentence = "Allow dev@harbor to run commands and change files without individual prompts?";
        assert_eq!(
            wrap(sentence, 30),
            ["Allow dev@harbor to run", "commands and change files", "without individual prompts?"]
        );
        assert_eq!(wrap("one two", 7), ["one two"], "a row as wide as the text holds all of it");
    }

    #[test]
    fn a_word_wider_than_the_row_is_cut_where_the_row_ends() {
        assert_eq!(
            wrap("see /etc/nginx/sites-available/menu now", 12),
            ["see", "/etc/nginx/s", "ites-availab", "le/menu now"]
        );
    }

    #[test]
    fn chinese_breaks_between_characters_by_their_width_and_keeps_punctuation_with_them() {
        let rows = wrap("允许其运行命令，并更改文件。", 9);
        assert_eq!(rows, ["允许其运", "行命令，", "并更改文", "件。"]);
        for row in &rows {
            assert!(unicode_width::UnicodeWidthStr::width(row.as_str()) <= 9, "{row}");
        }
        assert_eq!(wrap("允许 dev@harbor 运行", 12), ["允许", "dev@harbor", "运行"]);
        assert_eq!(wrap("（备注）在此", 6), ["（备", "注）在", "此"]);
    }

    #[test]
    fn an_indented_line_keeps_its_indent_on_every_row() {
        assert_eq!(wrap("  🐱 Cat · 🦁 Lion · 🐎 Horse", 16), ["  🐱 Cat · 🦁", "  Lion · 🐎", "  Horse"]);
        assert_eq!(wrap("", 10), [""]);
        assert_eq!(wrap("      x", 4), ["x"], "an indent half the row or wider is dropped");
    }
}
