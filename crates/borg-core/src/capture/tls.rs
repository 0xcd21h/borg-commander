//! TLS inside a connection's stream: its records, read from each side's bytes, and what the
//! sealed ones carry once a key log's secrets open them. TLS 1.3 and TLS 1.2 with AES-GCM or
//! ChaCha20-Poly1305.

use super::{
    keylog::{KeyLog, RANDOM, Random, Secrets},
    stream::{Stream, Turn},
};
use std::ops::Range;

mod cipher;

use cipher::{Hash, Key, Suite, TAG};

const HEADER: usize = 5;
const CHANGE_CIPHER_SPEC: u8 = 20;
const HANDSHAKE: u8 = 22;
const APPLICATION_DATA: u8 = 23;
const RECORD_TYPES: Range<u8> = CHANGE_CIPHER_SPEC..APPLICATION_DATA + 1;
const MAJOR_VERSION: u8 = 3;
/// The longest record either version allows: 2^14 bytes of plaintext and what sealing adds.
const LONGEST_RECORD: usize = (1 << 14) + 2048;
const CLIENT_HELLO: u8 = 1;
const SERVER_HELLO: u8 = 2;
const MESSAGE_HEADER: usize = 4;
const VERSION: usize = 2;
const SUPPORTED_VERSIONS: u16 = 0x002b;
const TLS13: u16 = 0x0304;
/// The random of a ServerHello that asks the client to say hello again (RFC 8446, 4.1.3).
const RETRY: Random = [
    0xcf, 0x21, 0xad, 0x74, 0xe5, 0x9a, 0x61, 0x11, 0xbe, 0x1d, 0x8c, 0x02, 0x1e, 0x65, 0xb8, 0x91, 0xc2, 0xa2, 0x11,
    0x16, 0x7a, 0xbb, 0x8c, 0x5e, 0x07, 0x9e, 0x09, 0xe2, 0xc8, 0xa8, 0x33, 0x9c,
];

/// A record's type, and where it lies in its side's bytes, header and all.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    kind: u8,
    range: Range<usize>,
}

impl Record {
    fn header<'a>(&self, bytes: &'a [u8]) -> &'a [u8] {
        &bytes[self.range.start..self.range.start + HEADER]
    }

    fn body<'a>(&self, bytes: &'a [u8]) -> &'a [u8] {
        &bytes[self.range.start + HEADER..self.range.end]
    }
}

/// The records at the start of `bytes`, and where the bytes stop being whole records.
fn records(bytes: &[u8]) -> (Vec<Record>, usize) {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(header) = bytes.get(at..at + HEADER) {
        let length = usize::from(u16::from_be_bytes([header[3], header[4]]));
        let end = at + HEADER + length;
        if !RECORD_TYPES.contains(&header[0])
            || header[1] != MAJOR_VERSION
            || length > LONGEST_RECORD
            || end > bytes.len()
        {
            break;
        }
        found.push(Record { kind: header[0], range: at..end });
        at = end;
    }
    (found, at)
}

/// The handshake messages a side sent in the clear, as their type and body: those of its
/// handshake records before its first ChangeCipherSpec, which TLS 1.2 seals the rest after.
fn clear_messages(bytes: &[u8], records: &[Record]) -> Vec<(u8, Vec<u8>)> {
    let joined: Vec<u8> = records
        .iter()
        .take_while(|record| record.kind != CHANGE_CIPHER_SPEC)
        .filter(|record| record.kind == HANDSHAKE)
        .flat_map(|record| record.body(bytes).iter().copied())
        .collect();
    let mut messages = Vec::new();
    let mut at = 0;
    while let Some(header) = joined.get(at..at + MESSAGE_HEADER) {
        let length = u32::from_be_bytes([0, header[1], header[2], header[3]]) as usize;
        let Some(body) = joined.get(at + MESSAGE_HEADER..at + MESSAGE_HEADER + length) else { break };
        messages.push((header[0], body.to_vec()));
        at += MESSAGE_HEADER + length;
    }
    messages
}

/// The client random of the ClientHello that `bytes`, what a client sent, begin with. Only the
/// start of the hello is needed, so the first packet of a long one is enough.
pub(super) fn client_random(bytes: &[u8]) -> Option<Random> {
    let hello = bytes.first() == Some(&HANDSHAKE)
        && bytes.get(1) == Some(&MAJOR_VERSION)
        && bytes.get(HEADER) == Some(&CLIENT_HELLO);
    let at = HEADER + MESSAGE_HEADER + VERSION;
    hello.then(|| bytes.get(at..at + RANDOM)?.try_into().ok()).flatten()
}

/// What a server's hello settles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ServerHello {
    random: Random,
    suite: u16,
    tls13: bool,
}

fn server_hello(messages: &[(u8, Vec<u8>)]) -> Option<ServerHello> {
    messages.iter().filter(|(kind, _)| *kind == SERVER_HELLO).find_map(|(_, body)| {
        let random: Random = body.get(VERSION..VERSION + RANDOM)?.try_into().ok()?;
        let session = VERSION + RANDOM;
        let suite_at = session + 1 + usize::from(*body.get(session)?);
        let suite = u16::from_be_bytes(body.get(suite_at..suite_at + 2)?.try_into().ok()?);
        let extensions = body.get(suite_at + 2 + 1 + 2..).unwrap_or_default();
        let tls13 = extension(extensions, SUPPORTED_VERSIONS).is_some_and(|value| value == TLS13.to_be_bytes());
        (random != RETRY).then_some(ServerHello { random, suite, tls13 })
    })
}

/// The value of the extension of type `wanted` among `extensions`.
fn extension(extensions: &[u8], wanted: u16) -> Option<&[u8]> {
    let mut at = 0;
    while let Some(header) = extensions.get(at..at + 4) {
        let (kind, length) =
            (u16::from_be_bytes([header[0], header[1]]), usize::from(u16::from_be_bytes([header[2], header[3]])));
        let value = extensions.get(at + 4..at + 4 + length)?;
        if kind == wanted {
            return Some(value);
        }
        at += 4 + length;
    }
    None
}

/// What one side of a connection sent: its turns' bytes one after another, where each of its
/// turns begins in them, and how far they run before the first bytes the capture lacks.
struct Side {
    bytes: Vec<u8>,
    starts: Vec<(usize, usize)>,
    whole: usize,
}

impl Side {
    fn of(stream: &Stream, from_client: bool) -> Self {
        let mut side = Self { bytes: Vec::new(), starts: Vec::new(), whole: usize::MAX };
        for (index, turn) in stream.turns.iter().enumerate().filter(|(_, turn)| turn.from_client == from_client) {
            if turn.missing > 0 {
                side.whole = side.whole.min(side.bytes.len());
            }
            side.starts.push((side.bytes.len(), index));
            side.bytes.extend_from_slice(&stream.bytes[turn.range.clone()]);
        }
        side.whole = side.whole.min(side.bytes.len());
        side
    }

    /// The stream's turn that the side's byte at `at` came in.
    fn turn(&self, at: usize) -> usize {
        let place = self.starts.partition_point(|&(start, _)| start <= at);
        self.starts[place.saturating_sub(1)].1
    }

    /// The parts of `range` in each turn they span.
    fn split(&self, range: Range<usize>) -> Vec<(usize, Range<usize>)> {
        let ends = self.starts.iter().skip(1).map(|&(start, _)| start).chain([self.bytes.len()]);
        self.starts
            .iter()
            .zip(ends)
            .map(|(&(start, turn), end)| (turn, start.max(range.start)..end.min(range.end)))
            .filter(|(_, part)| !part.is_empty())
            .collect()
    }
}

/// What a record or the bytes past the last whole one come to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    /// Application data a record carried.
    Opened(Vec<u8>),
    /// Bytes no key opened.
    Sealed(Range<usize>),
}

/// How one side's sealed records open.
enum Opener {
    /// TLS 1.3: each traffic secret the side moves through, with its key, the one in use, and
    /// how many records it has sealed so far.
    Tls13 { hash: Hash, suite: Suite, keys: Vec<(Vec<u8>, Key)>, current: usize, sequence: u64 },
    /// TLS 1.2: the side's key, and its records since its ChangeCipherSpec once one came.
    Tls12 { key: Key, sequence: Option<u64> },
}

impl Opener {
    fn tls13(suite: Suite, secrets: &[&Option<Vec<u8>>]) -> Self {
        let keys = secrets
            .iter()
            .copied()
            .flatten()
            .filter_map(|secret| Some((secret.clone(), cipher::traffic_key(suite, secret)?)))
            .collect();
        Self::Tls13 { hash: suite.hash, suite, keys, current: 0, sequence: 0 }
    }

    /// What the side's record carried, once it opens. Records in the clear and the ones that
    /// carry no application data come to nothing.
    fn open(&mut self, bytes: &[u8], record: &Record) -> Option<Piece> {
        let sealed = || Some(Piece::Sealed(record.range.clone()));
        match self {
            Self::Tls13 { .. } if record.kind != APPLICATION_DATA => None,
            Self::Tls13 { .. } => match self.open_tls13(record.header(bytes), record.body(bytes)) {
                Some((APPLICATION_DATA, data)) => Some(Piece::Opened(data)),
                Some(_) => None,
                None => sealed(),
            },
            Self::Tls12 { sequence, .. } if record.kind == CHANGE_CIPHER_SPEC => {
                *sequence = Some(0);
                None
            }
            Self::Tls12 { sequence: None, .. } => None,
            Self::Tls12 { key, sequence: Some(sequence) } => {
                let opened = open_tls12(key, *sequence, record.header(bytes), record.body(bytes));
                *sequence += 1;
                match opened {
                    Some(data) if record.kind == APPLICATION_DATA => Some(Piece::Opened(data)),
                    None if record.kind == APPLICATION_DATA => sealed(),
                    _ => None,
                }
            }
        }
    }

    /// A TLS 1.3 record's inner type and content. It is tried with the key in use, then with
    /// each the side moves on to, then with the one a key update would bring.
    fn open_tls13(&mut self, header: &[u8], body: &[u8]) -> Option<(u8, Vec<u8>)> {
        let Self::Tls13 { hash, suite, keys, current, sequence } = self else { return None };
        let tried = (*current..keys.len()).find_map(|index| {
            let next = if index == *current { *sequence } else { 0 };
            Some((index, next, keys[index].1.open(&keys[index].1.nonce(next), header, body)?))
        });
        let (index, next, plain) = match tried {
            Some(found) => found,
            None => {
                let secret = cipher::next_secret(*hash, &keys.last()?.0)?;
                let key = cipher::traffic_key(*suite, &secret)?;
                let plain = key.open(&key.nonce(0), header, body)?;
                keys.push((secret, key));
                (keys.len() - 1, 0, plain)
            }
        };
        (*current, *sequence) = (index, next + 1);
        inner(plain)
    }
}

/// A TLS 1.2 record's content, when it opens.
fn open_tls12(key: &Key, sequence: u64, header: &[u8], body: &[u8]) -> Option<Vec<u8>> {
    let (nonce, sealed) = if key.cipher.explicit_nonce() {
        let (carried, sealed) = body.split_at_checked(cipher::EXPLICIT_NONCE)?;
        (key.explicit(carried)?, sealed)
    } else {
        (key.nonce(sequence), body)
    };
    let length = u16::try_from(sealed.len().checked_sub(TAG)?).ok()?;
    let aad = [&sequence.to_be_bytes()[..], &header[..HEADER - 2], &length.to_be_bytes()].concat();
    key.open(&nonce, &aad, sealed)
}

/// A TLS 1.3 plaintext's content type, which ends it before any zeros of padding, and its
/// content.
fn inner(mut plain: Vec<u8>) -> Option<(u8, Vec<u8>)> {
    let end = plain.iter().rposition(|&byte| byte != 0)?;
    let kind = plain[end];
    plain.truncate(end);
    Some((kind, plain))
}

/// The openers of the client's records and the server's, from the connection's secrets.
fn openers(secrets: &Secrets, client: &Random, server: &ServerHello) -> Option<(Opener, Opener)> {
    let suite = cipher::suite(server.suite)?;
    if server.tls13 {
        let client_keys = [&secrets.client_early, &secrets.client_handshake, &secrets.client_traffic];
        let server_keys = [&secrets.server_handshake, &secrets.server_traffic];
        return Some((Opener::tls13(suite, &client_keys), Opener::tls13(suite, &server_keys)));
    }
    let (client_key, server_key) = cipher::session_keys(suite, secrets.master.as_ref()?, client, &server.random)?;
    Some((Opener::Tls12 { key: client_key, sequence: None }, Opener::Tls12 { key: server_key, sequence: None }))
}

/// What a side's records carried, each by the turn it began in, then its bytes past the last
/// whole record as they are.
fn pieces(side: &Side, mut opener: Opener) -> Vec<(usize, Piece)> {
    let (records, stop) = records(&side.bytes[..side.whole]);
    let mut pieces: Vec<(usize, Piece)> = records
        .iter()
        .filter_map(|record| Some((side.turn(record.range.start), opener.open(&side.bytes, record)?)))
        .filter(|(_, piece)| *piece != Piece::Opened(Vec::new()))
        .collect();
    pieces.extend(side.split(stop..side.bytes.len()).into_iter().map(|(turn, part)| (turn, Piece::Sealed(part))));
    pieces
}

/// `stream` with its TLS records opened by `keys`: what the records carried, in the turns they
/// came in, with the records no key opened marked. `None` when the stream is not TLS or the
/// key log has no secrets for it.
pub(super) fn decrypt(stream: &Stream, keys: &KeyLog) -> Option<Stream> {
    let (client, server) = (Side::of(stream, true), Side::of(stream, false));
    let random = client_random(&client.bytes[..client.whole])?;
    let secrets = keys.get(&random)?;
    let (server_records, _) = records(&server.bytes[..server.whole]);
    let hello = server_hello(&clear_messages(&server.bytes, &server_records))?;
    let (client_opener, server_opener) = openers(secrets, &random, &hello)?;
    let mut all: Vec<(usize, Piece, &Side)> = pieces(&client, client_opener)
        .into_iter()
        .map(|(turn, piece)| (turn, piece, &client))
        .chain(pieces(&server, server_opener).into_iter().map(|(turn, piece)| (turn, piece, &server)))
        .collect();
    all.sort_by_key(|(turn, ..)| *turn);
    let mut opened = Stream { decrypted: true, left_out: stream.left_out, ..Stream::default() };
    let mut last = None;
    for (index, piece, side) in all {
        let turn = &stream.turns[index];
        let (data, sealed) = match piece {
            Piece::Opened(data) => (data, false),
            Piece::Sealed(range) => (side.bytes[range].to_vec(), true),
        };
        let start = opened.bytes.len();
        opened.bytes.extend(data);
        let missing = if last == Some(index) { 0 } else { turn.missing };
        match opened.turns.last_mut() {
            Some(previous) if previous.from_client == turn.from_client && previous.sealed == sealed && missing == 0 => {
                previous.range.end = opened.bytes.len();
            }
            _ => opened.turns.push(Turn {
                from_client: turn.from_client,
                stamp: turn.stamp,
                range: start..opened.bytes.len(),
                missing,
                sealed,
            }),
        }
        last = Some(index);
    }
    Some(opened)
}

#[cfg(test)]
mod tests;
