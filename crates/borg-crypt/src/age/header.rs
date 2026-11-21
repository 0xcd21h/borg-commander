use std::io::{BufRead, Read};
use std::ops::RangeInclusive;

use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

use super::{Error, FILE_KEY_LEN, decode_raw, encode_raw};

const INTRO: &[u8] = b"age-encryption.org/v1\n";
const STANZA_PREFIX: &[u8] = b"->";
const CLOSING_PREFIX: &[u8] = b"---";
const SEPARATOR: u8 = b' ';
const LINE_END: u8 = b'\n';
const COLUMNS_PER_LINE: usize = 64;
const BYTES_PER_LINE: usize = COLUMNS_PER_LINE / 4 * 3;
const MAC_LEN: usize = 32;
const MAC_KEY_LEN: usize = 32;
const MAC_LABEL: &[u8] = b"header";
const HEADER_LIMIT: usize = 1 << 20;
const ARGUMENT_BYTES: RangeInclusive<u8> = 33..=126;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum HeaderError {
    #[error("the file does not start with the age version line")]
    Intro,
    #[error("the header ends early")]
    Truncated,
    #[error("the header is too large")]
    TooLarge,
    #[error("a recipient line is malformed")]
    StanzaLine,
    #[error("a recipient body line is malformed")]
    StanzaBody,
    #[error("the closing line is malformed")]
    ClosingLine,
    #[error("the payload nonce is missing")]
    MissingNonce,
    #[error("a passphrase recipient must be the only one")]
    PassphraseNotAlone,
    #[error("the passphrase recipient is malformed")]
    PassphraseStanza,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Stanza {
    pub kind: String,
    pub args: Vec<String>,
    pub body: Vec<u8>,
}

pub(super) struct Header {
    pub stanzas: Vec<Stanza>,
    mac: [u8; MAC_LEN],
    authenticated: Vec<u8>,
}

impl Header {
    pub fn verify(&self, file_key: &[u8; FILE_KEY_LEN]) -> Result<(), Error> {
        let mut mac = header_mac(file_key);
        mac.update(&self.authenticated);
        mac.verify_slice(&self.mac).map_err(|_| Error::HeaderMac)
    }

    pub fn encoded(&self) -> Vec<u8> {
        let mut out = self.authenticated.clone();
        out.push(SEPARATOR);
        out.extend_from_slice(encode_raw(&self.mac).as_bytes());
        out.push(LINE_END);
        out
    }
}

pub(super) fn read(input: &mut impl BufRead) -> Result<Header, Error> {
    let mut used = 0;
    let intro = read_line(input, &mut used)?;
    if intro != INTRO {
        return Err(HeaderError::Intro.into());
    }
    let mut authenticated = intro;
    let mut stanzas = Vec::new();
    loop {
        let line = read_line(input, &mut used)?;
        if line.starts_with(CLOSING_PREFIX) {
            let mac = parse_closing(&line)?;
            authenticated.extend_from_slice(CLOSING_PREFIX);
            return Ok(Header { stanzas, mac, authenticated });
        }
        let (kind, args) = parse_stanza_line(&line)?;
        authenticated.extend_from_slice(&line);
        let mut body = Vec::new();
        loop {
            let line = read_line(input, &mut used)?;
            let part = parse_body_line(&line)?;
            authenticated.extend_from_slice(&line);
            body.extend_from_slice(&part);
            if part.len() < BYTES_PER_LINE {
                break;
            }
        }
        stanzas.push(Stanza { kind, args, body });
    }
}

pub(super) fn encode(stanzas: &[Stanza], file_key: &[u8; FILE_KEY_LEN]) -> Vec<u8> {
    let mut out = INTRO.to_vec();
    for stanza in stanzas {
        out.extend_from_slice(STANZA_PREFIX);
        for argument in std::iter::once(&stanza.kind).chain(&stanza.args) {
            out.push(SEPARATOR);
            out.extend_from_slice(argument.as_bytes());
        }
        out.push(LINE_END);
        for part in stanza.body.chunks(BYTES_PER_LINE) {
            out.extend_from_slice(encode_raw(part).as_bytes());
            out.push(LINE_END);
        }
        if stanza.body.len() % BYTES_PER_LINE == 0 {
            out.push(LINE_END);
        }
    }
    out.extend_from_slice(CLOSING_PREFIX);
    let mut mac = header_mac(file_key);
    mac.update(&out);
    out.push(SEPARATOR);
    out.extend_from_slice(encode_raw(&mac.finalize().into_bytes()).as_bytes());
    out.push(LINE_END);
    out
}

fn header_mac(file_key: &[u8; FILE_KEY_LEN]) -> Hmac<Sha256> {
    let mut key = Zeroizing::new([0; MAC_KEY_LEN]);
    Hkdf::<Sha256>::new(None, file_key).expand(MAC_LABEL, key.as_mut()).expect("32 bytes fit HKDF-SHA256");
    <Hmac<Sha256> as Mac>::new_from_slice(key.as_ref()).expect("HMAC takes any key length")
}

fn read_line(input: &mut impl BufRead, used: &mut usize) -> Result<Vec<u8>, Error> {
    let mut line = Vec::new();
    let room = HEADER_LIMIT - *used;
    *used += input.take(room as u64).read_until(LINE_END, &mut line)?;
    if line.last() == Some(&LINE_END) {
        return Ok(line);
    }
    Err(if *used == HEADER_LIMIT { HeaderError::TooLarge } else { HeaderError::Truncated }.into())
}

fn split_line(line: &[u8]) -> impl Iterator<Item = &[u8]> {
    line.strip_suffix(&[LINE_END]).unwrap_or(line).split(|byte| *byte == SEPARATOR)
}

fn parse_stanza_line(line: &[u8]) -> Result<(String, Vec<String>), HeaderError> {
    let mut parts = split_line(line);
    if parts.next() != Some(STANZA_PREFIX) {
        return Err(HeaderError::StanzaLine);
    }
    let mut arguments = parts.map(parse_argument).collect::<Option<Vec<_>>>().ok_or(HeaderError::StanzaLine)?;
    if arguments.is_empty() {
        return Err(HeaderError::StanzaLine);
    }
    let kind = arguments.remove(0);
    Ok((kind, arguments))
}

fn parse_argument(part: &[u8]) -> Option<String> {
    let valid = !part.is_empty() && part.iter().all(|byte| ARGUMENT_BYTES.contains(byte));
    valid.then(|| String::from_utf8_lossy(part).into_owned())
}

fn parse_body_line(line: &[u8]) -> Result<Vec<u8>, HeaderError> {
    let text = line.strip_suffix(&[LINE_END]).unwrap_or(line);
    if text.len() > COLUMNS_PER_LINE {
        return Err(HeaderError::StanzaBody);
    }
    decode_raw(text).ok_or(HeaderError::StanzaBody)
}

fn parse_closing(line: &[u8]) -> Result<[u8; MAC_LEN], HeaderError> {
    let mut parts = split_line(line);
    let (Some(CLOSING_PREFIX), Some(encoded), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(HeaderError::ClosingLine);
    };
    decode_raw(encoded).and_then(|mac| mac.try_into().ok()).ok_or(HeaderError::ClosingLine)
}
