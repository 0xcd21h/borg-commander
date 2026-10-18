use std::{os::unix::ffi::OsStrExt, path::Path};

const QUOTE: u8 = b'\'';
const ESCAPED_QUOTE: &[u8] = b"'\\''";
const HISTORY_SKIP: &[u8] = b" ";
const SEPARATOR: u8 = b' ';
const LOCAL_PREFIX: &[u8] = b"./";
const SUBMIT: &[u8] = b"\r";

pub fn quote(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + 2);
    out.push(QUOTE);
    raw.iter().for_each(|&b| if b == QUOTE { out.extend_from_slice(ESCAPED_QUOTE) } else { out.push(b) });
    out.push(QUOTE);
    out
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandLine {
    words: Vec<Vec<u8>>,
}

impl CommandLine {
    pub fn program(name: &str) -> Self {
        Self { words: vec![name.as_bytes().to_vec()] }
    }

    pub fn flag(mut self, flag: &str) -> Self {
        self.words.push(flag.as_bytes().to_vec());
        self
    }

    pub fn arg(mut self, raw: &[u8]) -> Self {
        self.words.push(quote(raw));
        self
    }

    pub fn path(self, path: &Path) -> Self {
        self.arg(path.as_os_str().as_bytes())
    }

    pub fn local(self, name: &str) -> Self {
        self.arg(&[LOCAL_PREFIX, name.as_bytes()].concat())
    }

    pub fn locals<'a>(self, names: impl IntoIterator<Item = &'a str>) -> Self {
        names.into_iter().fold(self, Self::local)
    }

    pub fn glued(mut self, flag: &str, path: &Path) -> Self {
        self.words.push([flag.as_bytes(), &quote(path.as_os_str().as_bytes())].concat());
        self
    }

    pub fn line(&self) -> Vec<u8> {
        self.words.join(&SEPARATOR)
    }

    pub fn submit(&self) -> Vec<u8> {
        [&self.line()[..], SUBMIT].concat()
    }

    pub fn submit_hidden(&self) -> Vec<u8> {
        [HISTORY_SKIP, &self.line(), SUBMIT].concat()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_single_quotes() {
        assert_eq!(quote(b"it's"), b"'it'\\''s'");
    }

    #[test]
    fn builds_words_with_quoting_and_local_prefix() {
        let line = CommandLine::program("cp").flag("-R").locals(["a b", "-x"]).path(Path::new("/dst")).submit();
        assert_eq!(line, b"cp -R './a b' './-x' '/dst'\r");
    }

    #[test]
    fn hidden_commands_skip_history() {
        assert_eq!(CommandLine::program("cd").path(Path::new("/tmp")).submit_hidden(), b" cd '/tmp'\r");
    }

    #[test]
    fn glues_flag_and_path() {
        assert_eq!(CommandLine::program("7z").glued("-o", Path::new("/out dir")).line(), b"7z -o'/out dir'");
    }
}
