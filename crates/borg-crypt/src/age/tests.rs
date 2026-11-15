use std::io::{BufReader, Read, Write};
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use proptest::prelude::*;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::header::{self, Stanza};
use super::stream::CHUNK_LEN;
use super::*;

const PASSPHRASE: &[u8] = b"correct horse battery staple";
const FAST: WorkFactor = WorkFactor::MIN;
const ARMOR_COLUMNS: usize = 64;
const TESTKIT_VARIABLE: &str = "BORG_AGE_TESTKIT";
const TESTKIT_SKIPPED: [&str; 2] = ["x25519", "hybrid"];
const TESTKIT_MINIMUM: usize = 100;

fn fresh(seed: u8) -> Fresh {
    Fresh {
        file_key: Zeroizing::new([seed; FILE_KEY_LEN]),
        salt: [seed.wrapping_add(1); SALT_LEN],
        nonce: [seed.wrapping_add(2); NONCE_LEN],
    }
}

fn seal(plain: &[u8], work_factor: WorkFactor) -> Vec<u8> {
    let mut encryptor = encrypt(Vec::new(), PASSPHRASE, work_factor, &fresh(7)).expect("encrypt");
    encryptor.write_all(plain).expect("write");
    encryptor.finish().expect("finish")
}

fn open(file: &[u8], passphrase: &[u8]) -> Result<Vec<u8>, Error> {
    let mut plain = Vec::new();
    decrypt(file, passphrase, WorkFactor::MAX)?.read_to_end(&mut plain)?;
    Ok(plain)
}

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|index| (index * 31 % 251) as u8).collect()
}

fn armored(file: &[u8], line_end: &str) -> Vec<u8> {
    let encoded = STANDARD.encode(file);
    let mut out = format!("-----BEGIN AGE ENCRYPTED FILE-----{line_end}");
    for line in encoded.as_bytes().chunks(ARMOR_COLUMNS) {
        out.push_str(std::str::from_utf8(line).expect("base64 is ASCII"));
        out.push_str(line_end);
    }
    out.push_str(&format!("-----END AGE ENCRYPTED FILE-----{line_end}"));
    out.into_bytes()
}

#[test]
fn round_trips_across_chunk_edges() {
    for len in [0, 1, CHUNK_LEN - 1, CHUNK_LEN, CHUNK_LEN + 1, 2 * CHUNK_LEN, 2 * CHUNK_LEN + 5] {
        let plain = pattern(len);
        assert_eq!(open(&seal(&plain, FAST), PASSPHRASE).expect("open"), plain, "{len} bytes");
    }
}

#[test]
fn writes_the_age_passphrase_header() {
    let file = seal(b"hello", FAST);
    let text = String::from_utf8_lossy(&file);
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("age-encryption.org/v1"));
    assert_eq!(lines.next(), Some("-> scrypt CAgICAgICAgICAgICAgICA 1"));
    assert_eq!(lines.next().map(str::len), Some(43));
    assert!(lines.next().is_some_and(|line| line.starts_with("--- ")));
}

#[test]
fn reads_its_own_files_through_armor_with_either_line_end() {
    let plain = pattern(CHUNK_LEN + 100);
    let file = seal(&plain, FAST);
    for line_end in ["\n", "\r\n"] {
        assert_eq!(open(&armored(&file, line_end), PASSPHRASE).expect("open"), plain);
    }
}

#[test]
fn a_wrong_passphrase_is_reported() {
    let file = seal(b"secret", FAST);
    assert!(matches!(open(&file, b"wrong"), Err(Error::WrongPassphrase)));
}

#[test]
fn a_work_factor_above_the_limit_is_refused_before_deriving() {
    let file = seal(b"secret", WorkFactor(5));
    let limit = WorkFactor(4);
    let refused = decrypt(file.as_slice(), PASSPHRASE, limit).err();
    assert!(matches!(refused, Some(Error::WorkFactorTooHigh { found: 5, max }) if max == limit));
}

#[test]
fn work_factors_stay_in_range() {
    assert_eq!(WorkFactor::new(0), None);
    assert_eq!(WorkFactor::new(23), None);
    assert_eq!(WorkFactor::new(18).map(WorkFactor::log_n), Some(18));
}

#[test]
fn text_round_trips_up_to_the_limit() {
    let plain = pattern(TEXT_LIMIT);
    let file = encrypt_text(&plain, PASSPHRASE, FAST, &fresh(3)).expect("encrypt");
    assert_eq!(*decrypt_text(&file, PASSPHRASE, WorkFactor::MAX).expect("decrypt"), plain);
}

#[test]
fn text_over_the_limit_is_refused_both_ways() {
    let plain = pattern(TEXT_LIMIT + 1);
    assert!(matches!(encrypt_text(&plain, PASSPHRASE, FAST, &fresh(3)), Err(Error::TooLong { .. })));
    let file = seal(&plain, FAST);
    assert!(matches!(decrypt_text(&file, PASSPHRASE, WorkFactor::MAX), Err(Error::TooLong { .. })));
}

#[test]
fn files_for_other_recipients_are_not_for_a_passphrase() {
    let stanza = Stanza { kind: "X25519".into(), args: vec!["share".into()], body: vec![1; 32] };
    let file = header::encode(&[stanza], &[0; FILE_KEY_LEN]);
    assert!(matches!(open(&file, PASSPHRASE), Err(Error::NotForPassphrase)));
}

#[test]
fn a_passphrase_stanza_must_be_alone() {
    let key = [0; FILE_KEY_LEN];
    let scrypt = passphrase::wrap(PASSPHRASE, FAST, &[1; SALT_LEN], &key).expect("wrap");
    let other = Stanza { kind: "X25519".into(), args: vec!["share".into()], body: vec![1; 32] };
    let file = header::encode(&[scrypt, other], &key);
    assert!(matches!(open(&file, PASSPHRASE), Err(Error::Header(HeaderError::PassphraseNotAlone))));
}

#[test]
fn headers_round_trip_through_encoding() {
    let key = [9; FILE_KEY_LEN];
    let stanzas = vec![
        Stanza { kind: "a".into(), args: vec![], body: vec![] },
        Stanza { kind: "b".into(), args: vec!["1".into(), "two".into()], body: pattern(48) },
        Stanza { kind: "c".into(), args: vec![], body: pattern(100) },
    ];
    let encoded = header::encode(&stanzas, &key);
    let parsed = header::read(&mut BufReader::new(encoded.as_slice())).expect("parse");
    assert_eq!(parsed.stanzas, stanzas);
    parsed.verify(&key).expect("mac");
    assert!(matches!(parsed.verify(&[8; FILE_KEY_LEN]), Err(Error::HeaderMac)));
}

#[test]
fn plain_text_and_empty_files_are_not_age_files() {
    assert!(matches!(open(b"", PASSPHRASE), Err(Error::Header(HeaderError::Truncated))));
    assert!(matches!(open(b"just some notes\n", PASSPHRASE), Err(Error::Armor(ArmorError::FirstLine))));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn any_payload_round_trips(len in 0..3 * CHUNK_LEN, seed in any::<u8>()) {
        let plain = pattern(len);
        let mut encryptor = encrypt(Vec::new(), PASSPHRASE, FAST, &fresh(seed)).expect("encrypt");
        for piece in plain.chunks(1 + usize::from(seed) * 97) {
            encryptor.write_all(piece).expect("write");
        }
        let file = encryptor.finish().expect("finish");
        prop_assert_eq!(open(&file, PASSPHRASE).expect("open"), plain);
    }
}

proptest! {
    #[test]
    fn any_changed_byte_is_caught(index in any::<prop::sample::Index>(), flip in 1..=u8::MAX) {
        let mut file = seal(&pattern(300), FAST);
        let at = index.index(file.len());
        file[at] ^= flip;
        prop_assert!(open(&file, PASSPHRASE).is_err());
    }

    #[test]
    fn any_cut_is_caught(index in any::<prop::sample::Index>()) {
        let file = seal(&pattern(CHUNK_LEN + 300), FAST);
        let cut = index.index(file.len());
        prop_assert!(open(&file[..cut], PASSPHRASE).is_err());
    }

    #[test]
    fn appended_bytes_are_caught(extra in prop::collection::vec(any::<u8>(), 1..64)) {
        let mut file = seal(&pattern(100), FAST);
        file.extend_from_slice(&extra);
        prop_assert!(open(&file, PASSPHRASE).is_err());
    }
}

struct Vector {
    expect: String,
    payload: Option<String>,
    file_key: Option<[u8; FILE_KEY_LEN]>,
    passphrase: Option<String>,
    armored: bool,
    file: Vec<u8>,
}

impl Vector {
    fn parse(bytes: &[u8]) -> Self {
        let split = bytes.windows(2).position(|pair| pair == b"\n\n").expect("a blank line after the fields");
        let fields = std::str::from_utf8(&bytes[..split]).expect("UTF-8 fields");
        let mut vector = Self {
            expect: String::new(),
            payload: None,
            file_key: None,
            passphrase: None,
            armored: false,
            file: bytes[split + 2..].to_vec(),
        };
        let mut compressed = false;
        for line in fields.lines() {
            let (key, value) = line.split_once(": ").expect("key: value");
            match key {
                "expect" => vector.expect = value.to_owned(),
                "payload" => vector.payload = Some(value.to_owned()),
                "file key" => vector.file_key = unhex(value).try_into().ok(),
                "passphrase" => vector.passphrase = vector.passphrase.take().or(Some(value.to_owned())),
                "armored" => vector.armored = value == "yes",
                "compressed" => compressed = value == "zlib",
                _ => {}
            }
        }
        if compressed {
            let mut inflated = Vec::new();
            flate2::read::ZlibDecoder::new(vector.file.as_slice()).read_to_end(&mut inflated).expect("zlib");
            vector.file = inflated;
        }
        vector
    }

    fn run(&self) -> (String, Vec<u8>) {
        let opened = match (&self.passphrase, &self.file_key) {
            (Some(passphrase), _) => decrypt(self.file.as_slice(), passphrase.as_bytes(), WorkFactor::MAX),
            (None, Some(file_key)) => decrypt_with_file_key(self.file.as_slice(), file_key),
            (None, None) => panic!("a vector without a passphrase or file key"),
        };
        let mut decryptor = match opened {
            Ok(decryptor) => decryptor,
            Err(error) => return (outcome(&error).to_owned(), Vec::new()),
        };
        let mut plain = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            match decryptor.read(&mut buffer) {
                Ok(0) => return ("success".to_owned(), plain),
                Ok(read) => plain.extend_from_slice(&buffer[..read]),
                Err(error) => return (outcome(&Error::from(error)).to_owned(), plain),
            }
        }
    }
}

fn outcome(error: &Error) -> &'static str {
    match error {
        Error::Header(_) | Error::WorkFactorTooHigh { .. } => "header failure",
        Error::Armor(_) => "armor failure",
        Error::NotForPassphrase | Error::WrongPassphrase => "no match",
        Error::HeaderMac => "HMAC failure",
        Error::Payload(_) => "payload failure",
        Error::TooLong { .. } | Error::Io(_) => panic!("unexpected error: {error}"),
    }
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex")).collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn reencodes(vector: &Vector, plain: &[u8]) -> bool {
    let Some(file_key) = &vector.file_key else { return true };
    let parsed = header::read(&mut BufReader::new(vector.file.as_slice())).expect("parse");
    let head = header::encode(&parsed.stanzas, file_key);
    let Some(rest) = vector.file.strip_prefix(head.as_slice()) else { return false };
    let (nonce, payload) = rest.split_at(NONCE_LEN);
    let mut encryptor = Encryptor::new(Vec::new(), file_key, nonce.try_into().expect("nonce"));
    encryptor.write_all(plain).expect("write");
    encryptor.finish().expect("finish") == payload
}

#[test]
#[ignore = "needs the age testkit: set BORG_AGE_TESTKIT to its testdata folder"]
fn testkit() {
    let folder = std::env::var_os(TESTKIT_VARIABLE).expect("BORG_AGE_TESTKIT is set");
    let mut entries: Vec<_> = std::fs::read_dir(Path::new(&folder)).expect("testkit folder").flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    let mut checked = 0;
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if TESTKIT_SKIPPED.iter().any(|prefix| name.starts_with(prefix)) {
            continue;
        }
        let vector = Vector::parse(&std::fs::read(entry.path()).expect("vector"));
        if vector.expect == "no match" && vector.passphrase.is_none() {
            continue;
        }
        let (result, plain) = vector.run();
        assert_eq!(result, vector.expect, "{name}");
        if let Some(payload) = &vector.payload {
            assert_eq!(&hex(&Sha256::digest(&plain)), payload, "{name} payload");
        }
        if result == "success" && !vector.armored {
            assert!(reencodes(&vector, &plain), "{name} re-encoding");
        }
        checked += 1;
    }
    assert!(checked >= TESTKIT_MINIMUM, "only {checked} vectors checked");
}
