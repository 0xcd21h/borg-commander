use std::path::PathBuf;

use aes::Aes128;
use proptest::prelude::*;

use super::*;

const FIXTURES: &str = "tests/fixtures/pte";
const PASSWORD: &str = "correct horse";
const RANDOM: [u8; RANDOM_LEN] = [0x42; RANDOM_LEN];

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURES).join(name)).expect("fixture")
}

fn table(name: &str) -> Vec<Vec<String>> {
    String::from_utf8(fixture(name))
        .expect("UTF-8 table")
        .lines()
        .map(|line| line.split('\t').map(str::to_owned).collect())
        .collect()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex")).collect()
}

fn kdf_value(name: &str) -> Vec<u8> {
    let row = table("kdf.tsv").into_iter().find(|row| row[0] == name).expect("kdf row");
    unhex(&row[1])
}

fn open_text(file: &[u8], password: &str) -> Result<Vec<u8>, Error> {
    Ok(Envelope::parse(file)?.open(&MasterKey::from_password(password))?.to_vec())
}

#[test]
fn passwords_are_converted_like_the_original_app() {
    assert_eq!(*kdf::convert_password("contraseña"), "contrase241a");
    assert_eq!(*kdf::convert_password("Tab\there"), "Tab9here");
    assert_eq!(*kdf::convert_password("pw😀x"), "pw12851256832x");
    assert_eq!(*kdf::convert_password("~ !"), "~ !");
}

#[test]
fn key_derivation_matches_the_original_app() {
    assert_eq!(kdf_value("ikm"), b"contrase241a");
    let master = MasterKey::from_password("contraseña");
    assert_eq!(master.bytes(), kdf_value("master"));
    let stretched = kdf::stretch(&master, &kdf_value("salt"));
    assert_eq!(stretched.to_vec(), kdf_value("argon"));
    let aes = kdf::expand(&stretched, Cipher::Aes256);
    assert_eq!(aes.key.to_vec(), kdf_value("key32"));
    assert_eq!(aes.nonce.to_vec(), kdf_value("nonce16"));
    let blowfish = kdf::expand(&stretched, Cipher::Blowfish448);
    assert_eq!(blowfish.key.to_vec(), kdf_value("key56"));
    assert_eq!(blowfish.nonce.to_vec(), kdf_value("nonce8"));
}

#[test]
fn eax_matches_the_original_app() {
    let rows = table("eax.tsv");
    assert!(!rows.is_empty());
    for row in rows {
        let [name, key, nonce, aad, message, expected] = row.as_slice() else { panic!("six columns") };
        let (key, nonce, aad, expected) = (unhex(key), unhex(nonce), unhex(aad), unhex(expected));
        let mut buffer = unhex(message);
        let tag = match (name.as_str(), key.len()) {
            ("aes", 16) => eax::seal::<Aes128>(&key, &nonce, &aad, &mut buffer),
            ("aes", 32) => eax::seal::<Aes256>(&key, &nonce, &aad, &mut buffer),
            ("blowfish", _) => eax::seal::<Blowfish>(&key, &nonce, &aad, &mut buffer),
            other => panic!("unknown cipher {other:?}"),
        };
        assert_eq!([buffer, tag].concat(), expected, "{name} {message}");
    }
}

#[test]
fn opens_files_made_by_the_original_app() {
    let rows = table("manifest.tsv");
    assert!(!rows.is_empty());
    for row in rows {
        let [name, cipher, _, _, password, text] = row.as_slice() else { panic!("six columns") };
        let file = fixture(name);
        let password = String::from_utf8(unhex(password)).expect("UTF-8 password");
        match cipher.parse::<u8>().expect("cipher code") {
            code @ (Cipher::AES_256 | Cipher::BLOWFISH_448) => {
                let envelope = Envelope::parse(&file).expect(name);
                assert_eq!(envelope.cipher().code(), code, "{name}");
                let opened = envelope.open(&MasterKey::from_password(&password)).expect(name);
                assert_eq!(*opened, unhex(text), "{name}");
            }
            code => assert_eq!(Envelope::parse(&file).err(), Some(Error::UnsupportedCipher(code)), "{name}"),
        }
    }
}

#[test]
fn reads_compressed_files_from_the_original_app() {
    let compressed = table("manifest.tsv")
        .into_iter()
        .filter(|row| Envelope::parse(&fixture(&row[0])).is_ok_and(|envelope| envelope.compressed()))
        .count();
    assert!(compressed >= 2);
}

#[test]
fn sealed_text_round_trips_exactly() {
    let text = "  leading and trailing spaces stay  \r\n\n".as_bytes();
    for cipher in [Cipher::Aes256, Cipher::Blowfish448] {
        let file = seal(text, &MasterKey::from_password(PASSWORD), cipher, &RANDOM).expect("seal");
        assert!(file.ends_with('!'));
        let envelope = Envelope::parse(file.as_bytes()).expect("parse");
        assert_eq!(envelope.cipher(), cipher);
        assert!(!envelope.compressed());
        assert_eq!(open_text(file.as_bytes(), PASSWORD).expect("open"), text);
    }
}

#[test]
fn a_wrong_password_is_reported() {
    let file = seal(b"secret", &MasterKey::from_password(PASSWORD), Cipher::Blowfish448, &RANDOM).expect("seal");
    assert_eq!(open_text(file.as_bytes(), "wrong"), Err(Error::WrongPasswordOrDamaged));
}

#[test]
fn spaces_line_breaks_and_a_byte_order_mark_are_ignored() {
    let file = seal(b"spaced", &MasterKey::from_password(PASSWORD), Cipher::Aes256, &RANDOM).expect("seal");
    let spaced: String =
        file.chars().enumerate().flat_map(|(at, ch)| [ch].into_iter().chain((at % 7 == 6).then_some(' '))).collect();
    let marked = [BYTE_ORDER_MARK, spaced.as_bytes(), b"\r\n"].concat();
    assert_eq!(open_text(&marked, PASSWORD).expect("open"), b"spaced");
}

#[test]
fn other_text_is_not_pte() {
    assert_eq!(Envelope::parse(b"just some notes\n").err(), Some(Error::NotPte));
    assert_eq!(Envelope::parse(b"!").err(), Some(Error::NotPte));
    assert_eq!(Envelope::parse(b"plain text, with commas!").err(), Some(Error::NotPte));
}

#[test]
fn limits_are_enforced() {
    let master = MasterKey::from_password(PASSWORD);
    assert_eq!(
        seal(&vec![b'a'; TEXT_LIMIT + 1], &master, Cipher::Aes256, &RANDOM),
        Err(Error::TooLong { limit: TEXT_LIMIT })
    );
    assert_eq!(Envelope::parse(&vec![b'A'; FILE_LIMIT + 1]).err(), Some(Error::FileTooLarge { limit: FILE_LIMIT }));
}

#[test]
fn other_versions_are_named() {
    let file = seal(b"x", &MasterKey::from_password(PASSWORD), Cipher::Aes256, &RANDOM).expect("seal");
    let mut bytes = PTE_BASE64.decode(file.trim_end_matches('!')).expect("base64");
    let config = bytes.last_mut().expect("config byte");
    *config = (2 << VERSION_SHIFT) | Cipher::AES_256;
    let older = format!("{}!", PTE_BASE64.encode(&bytes));
    assert_eq!(Envelope::parse(older.as_bytes()).err(), Some(Error::UnsupportedVersion(2)));
}

#[test]
fn compressed_payloads_are_checked_before_decoding() {
    let master = MasterKey::from_password(PASSWORD);
    let flags = (VERSION << VERSION_SHIFT) | COMPRESSED;
    let mut too_large = vec![LZMA_PROPERTIES, 0, 0, 0x10, 0];
    too_large.extend_from_slice(&(TEXT_LIMIT as u64 + 1).to_le_bytes());
    let cases = [
        (too_large, Error::TooLong { limit: TEXT_LIMIT }),
        (vec![0x01, 0x02, 0x03], Error::UnsupportedCompression),
        (vec![LZMA_PROPERTIES, 0, 0], Error::Damaged),
        ([&[LZMA_PROPERTIES, 0, 0, 0x10, 0][..], &5u64.to_le_bytes(), &[0xff; 4]].concat(), Error::Damaged),
    ];
    for (payload, expected) in cases {
        let file = seal_payload(&payload, &master, Cipher::Aes256, &RANDOM, flags);
        assert_eq!(open_text(file.as_bytes(), PASSWORD), Err(expected));
    }
}

proptest! {
    #[test]
    fn eax_catches_any_changed_byte(
        message in prop::collection::vec(any::<u8>(), 0..64),
        index in any::<prop::sample::Index>(),
        flip in 1..=u8::MAX,
    ) {
        let (key, nonce, aad) = ([7; 56], [9; 8], [1, 2, 3]);
        let mut sealed = message.clone();
        let tag = eax::seal::<Blowfish>(&key, &nonce, &aad, &mut sealed);
        let mut whole = [sealed, tag].concat();
        let at = index.index(whole.len());
        whole[at] ^= flip;
        let (body, tag) = whole.split_at_mut(message.len());
        prop_assert!(eax::open::<Blowfish>(&key, &nonce, &aad, body, tag).is_err());
    }

    #[test]
    fn eax_round_trips(message in prop::collection::vec(any::<u8>(), 0..200)) {
        let (key, nonce, aad) = ([3; 32], [5; 16], [8; 9]);
        let mut buffer = message.clone();
        let tag = eax::seal::<Aes256>(&key, &nonce, &aad, &mut buffer);
        eax::open::<Aes256>(&key, &nonce, &aad, &mut buffer, &tag).expect("open");
        prop_assert_eq!(buffer, message);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]

    #[test]
    fn files_catch_any_changed_byte(index in any::<prop::sample::Index>(), flip in 1..=u8::MAX) {
        let file = seal(b"a short note", &MasterKey::from_password(PASSWORD), Cipher::Aes256, &RANDOM).expect("seal");
        let mut bytes = PTE_BASE64.decode(file.trim_end_matches('!')).expect("base64");
        let at = index.index(bytes.len());
        bytes[at] ^= flip;
        let changed = format!("{}!", PTE_BASE64.encode(&bytes));
        prop_assert!(open_text(changed.as_bytes(), PASSWORD).is_err());
    }
}
