const NEWLINE: u8 = b'\n';

/// An entry as one line of the log file: a JSON string, so its own line breaks stay inside it.
pub fn line(entry: &str) -> String {
    let mut line = serde_json::Value::from(entry).to_string();
    line.push(char::from(NEWLINE));
    line
}

/// The entries in the log file's `text`, oldest first, leaving out lines that do not read as
/// one; `cut` means `text` starts inside the file, so its first line is a piece of an entry.
pub fn entries(text: &[u8], cut: bool) -> Vec<String> {
    text.split(|&byte| byte == NEWLINE)
        .skip(usize::from(cut))
        .filter_map(|line| serde_json::from_slice::<String>(line).ok())
        .collect()
}

/// The end of the log file's `text` that holds the newest whole lines in at most `keep` bytes.
pub fn newest(text: &[u8], keep: usize) -> &[u8] {
    let from = text.len().saturating_sub(keep);
    if from == 0 || text[from - 1] == NEWLINE {
        return &text[from..];
    }
    let start = text[from..].iter().position(|&byte| byte == NEWLINE).map_or(text.len(), |at| from + at + 1);
    &text[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(entries: &[&str]) -> Vec<u8> {
        entries.iter().flat_map(|entry| line(entry).into_bytes()).collect()
    }

    #[test]
    fn entries_with_line_breaks_and_quotes_read_back_as_written() {
        let written = ["14:00:00  ran in ~/srv\n$ echo \"hi\"\n[exit 0]", "14:00:01  read /srv/app"];
        let text = file(&written);
        assert_eq!(text.iter().filter(|&&byte| byte == NEWLINE).count(), written.len());
        assert_eq!(entries(&text, false), written);
    }

    #[test]
    fn a_cut_first_line_and_unreadable_lines_are_left_out() {
        let mut text = b"\"hi\\\"\"\n".to_vec();
        text.extend(file(&["14:00:00  showed /srv"]));
        text.extend(b"{not an entry\n\n");
        text.extend(file(&["14:00:01  read /srv/app"]));
        assert_eq!(entries(&text, true), ["14:00:00  showed /srv", "14:00:01  read /srv/app"]);
        assert_eq!(entries(&text, false)[0], "hi\"", "an uncut first line is an entry");
    }

    #[test]
    fn trimming_keeps_the_newest_whole_lines_that_fit() {
        let text = file(&["one", "two", "three"]);
        let last_two = line("two").len() + line("three").len();
        assert_eq!(entries(newest(&text, last_two), false), ["two", "three"]);
        assert_eq!(entries(newest(&text, last_two + 1), false), ["two", "three"]);
        assert_eq!(entries(newest(&text, last_two - 1), false), ["three"]);
        assert_eq!(newest(&text, text.len()), text.as_slice());
        assert!(newest(&text, line("three").len() - 1).is_empty(), "an entry too big to keep goes");
    }
}
