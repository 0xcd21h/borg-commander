use std::{os::unix::ffi::OsStrExt, path::Path};

const QUOTE: u8 = b'\'';
const ESCAPED_QUOTE: &[u8] = b"'\\''";
const HISTORY_SKIP: &[u8] = b" ";
const WORD_SEPARATOR: &[u8] = b" ";
const CD: &[u8] = b"cd ";
const RUN_LOCAL: &[u8] = b"./";
const SUBMIT: &[u8] = b"\r";

pub fn quote(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + 2);
    out.push(QUOTE);
    raw.iter().for_each(|&b| if b == QUOTE { out.extend_from_slice(ESCAPED_QUOTE) } else { out.push(b) });
    out.push(QUOTE);
    out
}

pub fn cd_command(dir: &Path) -> Vec<u8> {
    [HISTORY_SKIP, CD, &quote(dir.as_os_str().as_bytes()), SUBMIT].concat()
}

pub fn run_command(name: &str) -> Vec<u8> {
    [RUN_LOCAL, &quote(name.as_bytes()), SUBMIT].concat()
}

pub fn insert_word(word: &str) -> Vec<u8> {
    [&quote(word.as_bytes())[..], WORD_SEPARATOR].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_single_quotes() {
        assert_eq!(quote(b"it's"), b"'it'\\''s'");
    }

    #[test]
    fn builds_hidden_cd() {
        assert_eq!(cd_command(Path::new("/tmp/a b")), b" cd '/tmp/a b'\r");
    }
}
