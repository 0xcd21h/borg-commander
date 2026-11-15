//! The DNS messages mDNS, LLMNR and NetBIOS carry: their records with the names packed as
//! RFC 1035 packs them, read as far as the listener needs.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

const HEADER: usize = 12;
const FLAGS: usize = 2;
/// Where the header counts the questions, answers, authority and additional records.
const COUNTS: [usize; 4] = [4, 6, 8, 10];
const RESPONSE: u16 = 0x8000;
const OPCODE_SHIFT: u16 = 11;
const OPCODE_MASK: u16 = 0x0f;
const POINTER: u8 = 0xc0;
const POINTER_MASK: u16 = 0x3fff;
/// Jumps a packed name may take before Borg takes it for a loop.
const JUMPS: usize = 32;
const LONGEST_NAME: usize = 255;
/// Records Borg reads of one section; a real announcement has a few dozen at most.
const MOST_RECORDS: u16 = 64;
const QUESTION_TAIL: usize = 4;
const RECORD_HEAD: usize = 10;
/// Where a record's data length sits, after its type, class and time to live.
const DATA_LENGTH: usize = 8;
const TYPE_A: u16 = 1;
const TYPE_PTR: u16 = 12;
const TYPE_TXT: u16 = 16;
const TYPE_AAAA: u16 = 28;
const TYPE_SRV: u16 = 33;
/// Where a SRV record's target starts, after its priority, weight and port.
const SRV_TARGET: usize = 6;
const IPV4_BYTES: usize = 4;
const IPV6_BYTES: usize = 16;
const LABEL_SEPARATOR: char = '.';
const PAIR_SEPARATOR: char = '=';

/// What a record says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Data {
    Address(IpAddr),
    Pointer(String),
    Service { target: String },
    Text(Vec<(String, String)>),
    Other(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Record {
    pub(super) name: String,
    pub(super) kind: u16,
    pub(super) data: Data,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Message {
    pub(super) response: bool,
    pub(super) opcode: u8,
    pub(super) questions: Vec<String>,
    pub(super) answers: Vec<Record>,
    pub(super) authority: Vec<Record>,
    pub(super) additional: Vec<Record>,
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

/// The name packed at `at`, and where the bytes after it start.
fn name(bytes: &[u8], at: usize) -> Option<(String, usize)> {
    let mut labels: Vec<String> = Vec::new();
    let mut at = at;
    let mut after = None;
    let mut length = 0;
    for _ in 0..JUMPS {
        let size = *bytes.get(at)?;
        if size & POINTER == POINTER {
            after.get_or_insert(at + 2);
            at = usize::from(u16_at(bytes, at)? & POINTER_MASK);
            continue;
        }
        if size == 0 {
            return Some((labels.join("."), after.unwrap_or(at + 1)));
        }
        let label = bytes.get(at + 1..at + 1 + usize::from(size))?;
        length += label.len() + 1;
        if length > LONGEST_NAME {
            return None;
        }
        labels.push(String::from_utf8_lossy(label).into_owned());
        at += 1 + usize::from(size);
    }
    None
}

/// The pairs of a TXT record's strings, each `key=value` or a bare key.
fn text(data: &[u8]) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut at = 0;
    while let Some(&size) = data.get(at) {
        let Some(string) = data.get(at + 1..at + 1 + usize::from(size)) else { break };
        let string = String::from_utf8_lossy(string);
        let (key, value) = string.split_once(PAIR_SEPARATOR).unwrap_or((&string, ""));
        if !key.is_empty() {
            pairs.push((key.to_owned(), value.to_owned()));
        }
        at += 1 + usize::from(size);
    }
    pairs
}

fn address(data: &[u8]) -> Option<IpAddr> {
    match data.len() {
        IPV4_BYTES => Some(IpAddr::V4(Ipv4Addr::from(<[u8; IPV4_BYTES]>::try_from(data).ok()?))),
        IPV6_BYTES => Some(IpAddr::V6(Ipv6Addr::from(<[u8; IPV6_BYTES]>::try_from(data).ok()?))),
        _ => None,
    }
}

/// The record at `at`, and where the next one starts.
fn record(bytes: &[u8], at: usize) -> Option<(Record, usize)> {
    let (owner, at) = name(bytes, at)?;
    let kind = u16_at(bytes, at)?;
    let length = usize::from(u16_at(bytes, at + DATA_LENGTH)?);
    let start = at + RECORD_HEAD;
    let data = bytes.get(start..start + length)?;
    let other = || Data::Other(data.to_vec());
    let found = match kind {
        TYPE_A | TYPE_AAAA => address(data).map(Data::Address),
        TYPE_PTR => name(bytes, start).map(|(target, _)| Data::Pointer(target)),
        TYPE_SRV => name(bytes, start + SRV_TARGET).map(|(target, _)| Data::Service { target }),
        TYPE_TXT => Some(Data::Text(text(data))),
        _ => None,
    };
    let record =
        Record { name: owner.trim_end_matches(LABEL_SEPARATOR).to_owned(), kind, data: found.unwrap_or_else(other) };
    Some((record, start + length))
}

fn records(bytes: &[u8], at: &mut usize, count: u16) -> Vec<Record> {
    let mut found = Vec::new();
    for _ in 0..count.min(MOST_RECORDS) {
        let Some((record, next)) = record(bytes, *at) else { break };
        found.push(record);
        *at = next;
    }
    found
}

/// The message in `bytes`, with as many of its records as are whole.
pub(super) fn read(bytes: &[u8]) -> Option<Message> {
    if bytes.len() < HEADER {
        return None;
    }
    let flags = u16_at(bytes, FLAGS)?;
    let [questions, answers, authority, additional] = COUNTS.map(|at| u16_at(bytes, at).unwrap_or_default());
    let mut at = HEADER;
    let mut asked = Vec::new();
    for _ in 0..questions.min(MOST_RECORDS) {
        let (question, next) = name(bytes, at)?;
        asked.push(question);
        at = next + QUESTION_TAIL;
    }
    let answers = records(bytes, &mut at, answers);
    let authority = records(bytes, &mut at, authority);
    let additional = records(bytes, &mut at, additional);
    let opcode = u8::try_from((flags >> OPCODE_SHIFT) & OPCODE_MASK).unwrap_or_default();
    Some(Message { response: flags & RESPONSE != 0, opcode, questions: asked, answers, authority, additional })
}

#[cfg(test)]
pub(super) mod build {
    //! Packs made-up DNS messages for the tests.

    pub(crate) fn name(name: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for label in name.split('.').filter(|label| !label.is_empty()) {
            out.push(u8::try_from(label.len()).unwrap());
            out.extend(label.as_bytes());
        }
        out.push(0);
        out
    }

    /// A record owned by `owner` of DNS type `kind` holding `data`.
    pub(crate) fn record(owner: &str, kind: u16, data: &[u8]) -> Vec<u8> {
        let mut out = name(owner);
        out.extend(kind.to_be_bytes());
        out.extend(0x8001_u16.to_be_bytes());
        out.extend(120_u32.to_be_bytes());
        out.extend(u16::try_from(data.len()).unwrap().to_be_bytes());
        out.extend(data);
        out
    }

    pub(crate) fn text(pairs: &[&str]) -> Vec<u8> {
        pairs.iter().flat_map(|pair| std::iter::once(u8::try_from(pair.len()).unwrap()).chain(pair.bytes())).collect()
    }

    pub(crate) fn service(target: &str) -> Vec<u8> {
        let mut out = vec![0, 0, 0, 0, 0x1f, 0x49];
        out.extend(name(target));
        out
    }

    /// A message with `flags`, one question when `asked` names one, and its sections.
    pub(crate) fn message(flags: u16, asked: Option<&str>, sections: [&[Vec<u8>]; 3]) -> Vec<u8> {
        let mut out = vec![0, 0];
        out.extend(flags.to_be_bytes());
        out.extend(u16::from(asked.is_some()).to_be_bytes());
        for section in sections {
            out.extend(u16::try_from(section.len()).unwrap().to_be_bytes());
        }
        if let Some(asked) = asked {
            out.extend(name(asked));
            out.extend([0, 1, 0, 1]);
        }
        for section in sections {
            for record in section {
                out.extend(record);
            }
        }
        out
    }
}
