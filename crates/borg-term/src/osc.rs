use std::{ffi::OsString, os::unix::ffi::OsStringExt, path::PathBuf};

use crate::ShellSignal;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const OSC_INTRODUCER: u8 = b']';
const STRING_TERMINATOR: u8 = b'\\';
const MAX_PAYLOAD: usize = 4096;

const SEMANTIC_PROMPT: &[u8] = b"133;";
const CURRENT_DIR: &[u8] = b"7;";
/// Borg's own announcement, sent by its shell integration (`OSC 6633;silent-cd`).
const SILENT_CD: &[u8] = b"6633;silent-cd";
const FILE_SCHEME: &[u8] = b"file://";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    Escape,
    Payload,
    PayloadEscape,
    Overflow,
    OverflowEscape,
}

#[derive(Debug, Default)]
pub struct OscScanner {
    state: State,
    payload: Vec<u8>,
}

impl OscScanner {
    pub fn scan(&mut self, bytes: &[u8]) -> Vec<(usize, ShellSignal)> {
        bytes
            .iter()
            .enumerate()
            .filter_map(|(offset, &byte)| self.step(byte).map(|signal| (offset + 1, signal)))
            .collect()
    }

    fn step(&mut self, byte: u8) -> Option<ShellSignal> {
        let (next, finished) = match (self.state, byte) {
            (State::Ground | State::Escape, ESC) => (State::Escape, false),
            (State::Escape, OSC_INTRODUCER) => {
                self.payload.clear();
                (State::Payload, false)
            }
            (State::Ground | State::Escape, _) => (State::Ground, false),
            (State::Payload, BEL) => (State::Ground, true),
            (State::Payload, ESC) => (State::PayloadEscape, false),
            (State::PayloadEscape, STRING_TERMINATOR) => (State::Ground, true),
            (State::PayloadEscape, OSC_INTRODUCER) => {
                self.payload.clear();
                (State::Payload, false)
            }
            (State::PayloadEscape, _) => (State::Ground, false),
            (State::Payload, _) if self.payload.len() >= MAX_PAYLOAD => (State::Overflow, false),
            (State::Payload, _) => {
                self.payload.push(byte);
                (State::Payload, false)
            }
            (State::Overflow | State::OverflowEscape, BEL) => (State::Ground, false),
            (State::Overflow | State::OverflowEscape, ESC) => (State::OverflowEscape, false),
            (State::OverflowEscape, STRING_TERMINATOR) => (State::Ground, false),
            (State::Overflow | State::OverflowEscape, _) => (State::Overflow, false),
        };

        self.state = next;
        finished.then(|| parse_payload(&self.payload)).flatten()
    }
}

fn parse_payload(payload: &[u8]) -> Option<ShellSignal> {
    if let Some(rest) = payload.strip_prefix(SEMANTIC_PROMPT) {
        parse_semantic_prompt(rest)
    } else if let Some(rest) = payload.strip_prefix(CURRENT_DIR) {
        parse_current_dir(rest)
    } else if payload == SILENT_CD {
        Some(ShellSignal::SilentCd)
    } else {
        None
    }
}

fn parse_semantic_prompt(rest: &[u8]) -> Option<ShellSignal> {
    let mut parts = rest.split(|&b| b == b';');
    match parts.next()? {
        b"A" => Some(ShellSignal::PromptStart),
        b"B" => Some(ShellSignal::InputStart),
        b"C" => Some(ShellSignal::CommandStart),
        b"D" => Some(ShellSignal::CommandEnd {
            exit: parts.next().and_then(|code| std::str::from_utf8(code).ok()?.parse().ok()),
        }),
        _ => None,
    }
}

fn parse_current_dir(rest: &[u8]) -> Option<ShellSignal> {
    let location = rest.strip_prefix(FILE_SCHEME)?;
    let split = location.iter().position(|&b| b == b'/')?;
    let (host, path) = location.split_at(split);

    Some(ShellSignal::Cwd {
        host: String::from_utf8_lossy(host).into_owned(),
        path: PathBuf::from(OsString::from_vec(percent_decode(path))),
    })
}

fn percent_decode(bytes: &[u8]) -> Vec<u8> {
    let hex = |b: u8| char::from(b).to_digit(16).and_then(|d| u8::try_from(d).ok());
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let decoded =
            (bytes[i] == b'%').then(|| Some((hex(*bytes.get(i + 1)?)? << 4) | hex(*bytes.get(i + 2)?)?)).flatten();
        match decoded {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_all(chunks: &[&[u8]]) -> Vec<ShellSignal> {
        let mut scanner = OscScanner::default();
        chunks.iter().flat_map(|chunk| scanner.scan(chunk)).map(|(_, signal)| signal).collect()
    }

    #[test]
    fn silent_cd_is_announced_by_borg_integration_only() {
        assert_eq!(scan_all(&[b"\x1b]6633;silent-cd\x07"]), vec![ShellSignal::SilentCd]);
        assert!(scan_all(&[b"\x1b]6633;silent-cd-later\x07", b"\x1b]6634;silent-cd\x07"]).is_empty());
    }

    #[test]
    fn recognizes_semantic_prompt_marks() {
        let signals = scan_all(&[b"\x1b]133;A\x07$ \x1b]133;B\x1b\\ls\r\n\x1b]133;C\x07out\x1b]133;D;2\x07"]);
        assert_eq!(
            signals,
            vec![
                ShellSignal::PromptStart,
                ShellSignal::InputStart,
                ShellSignal::CommandStart,
                ShellSignal::CommandEnd { exit: Some(2) },
            ]
        );
    }

    #[test]
    fn decodes_current_directory() {
        let signals = scan_all(&[b"\x1b]7;file://box/home/me/My%20Docs/100%25\x07"]);
        assert_eq!(
            signals,
            vec![ShellSignal::Cwd { host: "box".into(), path: PathBuf::from("/home/me/My Docs/100%") }]
        );
    }

    #[test]
    fn survives_sequences_split_across_chunks() {
        let signals = scan_all(&[b"text\x1b", b"]13", b"3;D\x1b", b"\\"]);
        assert_eq!(signals, vec![ShellSignal::CommandEnd { exit: None }]);
    }

    #[test]
    fn reports_offset_after_terminator() {
        let mut scanner = OscScanner::default();
        let bytes = b"ab\x1b]133;B\x07cd";
        let found = scanner.scan(bytes);
        assert_eq!(found, vec![(bytes.len() - 2, ShellSignal::InputStart)]);
    }

    #[test]
    fn ignores_unrelated_and_oversized_sequences() {
        let mut oversized = b"\x1b]133;".to_vec();
        oversized.extend(std::iter::repeat_n(b'x', MAX_PAYLOAD * 2));
        oversized.extend(b"\x07\x1b]0;title\x07\x1b]133;A\x07");
        assert_eq!(scan_all(&[&oversized]), vec![ShellSignal::PromptStart]);
    }
}
