use std::{os::unix::ffi::OsStrExt, path::Path};

const QUOTE: u8 = b'\'';
const ESCAPED_QUOTE: &[u8] = b"'\\''";
pub(crate) const HISTORY_SKIP: &[u8] = b" ";
const SEPARATOR: u8 = b' ';
const LOCAL_PREFIX: &[u8] = b"./";
const ASSIGN: u8 = b'=';
const COMMAND_END: u8 = b';';
pub(crate) const AND_THEN: &str = "&&";
pub(crate) const SUBMIT: &[u8] = b"\r";
/// Characters no shell expands or splits on inside a word, beside letters and digits.
const PLAIN_PUNCTUATION: &[u8] = b"-_.:/@+,";

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

    /// A word as it reads, quoted only when the shell would read it otherwise.
    pub fn word(mut self, raw: &str) -> Self {
        let plain = !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_alphanumeric() || PLAIN_PUNCTUATION.contains(&b));
        self.words.push(if plain { raw.as_bytes().to_vec() } else { quote(raw.as_bytes()) });
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

    /// `name=value`, with the value quoted, as a shell sets a variable.
    pub fn assigned(mut self, name: &str, value: &[u8]) -> Self {
        self.words.push([name.as_bytes(), &[ASSIGN], &quote(value)].concat());
        self
    }

    /// This command, then `next` once it ends.
    pub fn then(mut self, next: Self) -> Self {
        if let Some(last) = self.words.last_mut() {
            last.push(COMMAND_END);
        }
        self.words.extend(next.words);
        self
    }

    /// This command, then `next` if it succeeds.
    pub fn and_then(self, next: Self) -> Self {
        let mut joined = self.flag(AND_THEN);
        joined.words.extend(next.words);
        joined
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
    fn assigns_quoted_values_and_runs_one_command_after_another() {
        let line = CommandLine::program("export").assigned("A", b"it's").then(CommandLine::program("unset").flag("B"));
        assert_eq!(line.line(), b"export A='it'\\''s'; unset B");
    }

    #[test]
    fn runs_the_next_command_only_once_one_succeeds() {
        let line = CommandLine::program("make").and_then(CommandLine::program("make").flag("install"));
        assert_eq!(line.line(), b"make && make install");
    }

    #[test]
    fn plain_words_stay_bare_and_others_are_quoted() {
        let line = CommandLine::program("docker").word("registry.example:5000/shop-web:1.4").word("it's").word("");
        assert_eq!(line.line(), b"docker registry.example:5000/shop-web:1.4 'it'\\''s' ''");
        assert_eq!(CommandLine::program("rm").word("~/x").word("a b").word("$HOME").line(), b"rm '~/x' 'a b' '$HOME'");
    }

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
