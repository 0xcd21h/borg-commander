use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use zeroize::Zeroizing;

use super::*;
use crate::TEXT_LIMIT;

const PASSWORD: &str = "correct horse";
const FAST: u8 = 1;
const RANDOM: [u8; RANDOM_LEN] = [0x42; RANDOM_LEN];
const ARMOR_COLUMNS: usize = 64;

fn fast() -> age::WorkFactor {
    age::WorkFactor::new(FAST).expect("a valid work factor")
}

fn age_file(text: &[u8], work_factor: age::WorkFactor) -> Vec<u8> {
    let fresh = age::Fresh {
        file_key: Zeroizing::new([1; age::FILE_KEY_LEN]),
        salt: [2; age::SALT_LEN],
        nonce: [3; age::NONCE_LEN],
    };
    age::encrypt_text(text, PASSWORD.as_bytes(), work_factor, &fresh).expect("encrypt")
}

fn pte_file(text: &[u8]) -> Vec<u8> {
    pte::seal(text, &pte::MasterKey::from_password(PASSWORD), pte::Cipher::Aes256, &RANDOM).expect("seal").into_bytes()
}

fn armored(binary: &[u8]) -> Vec<u8> {
    let encoded = STANDARD.encode(binary);
    let mut out = String::from("\n  \n-----BEGIN AGE ENCRYPTED FILE-----\n");
    for line in encoded.as_bytes().chunks(ARMOR_COLUMNS) {
        out.push_str(std::str::from_utf8(line).expect("base64 is ASCII"));
        out.push('\n');
    }
    out.push_str("-----END AGE ENCRYPTED FILE-----\n");
    out.into_bytes()
}

#[test]
fn detects_both_formats_and_nothing_else() {
    assert_eq!(detect(&age_file(b"notes", fast())), Some(Format::Age));
    assert_eq!(detect(&armored(&age_file(b"notes", fast()))), Some(Format::Age));
    assert_eq!(detect(&pte_file(b"notes")), Some(Format::Pte));
    assert_eq!(detect(b"plain notes!\n"), None);
    assert_eq!(detect(b""), None);
}

#[test]
fn opens_and_reseals_both_formats() {
    for file in [age_file(b"first", fast()), armored(&age_file(b"first", fast())), pte_file(b"first")] {
        let format = detect(&file).expect("a sealed file");
        let (text, seal) = unseal(&file, PASSWORD, age::WorkFactor::MAX).expect("open");
        assert_eq!(*text, b"first");
        assert_eq!(seal.format(), format);
        let resealed = seal.seal(b"second", &RANDOM).expect("reseal");
        assert_eq!(detect(&resealed), Some(format));
        let (text, again) = unseal(&resealed, PASSWORD, age::WorkFactor::MAX).expect("reopen");
        assert_eq!(*text, b"second");
        assert_eq!(again.format(), format);
    }
}

#[test]
fn errors_are_named_for_the_user() {
    let age = age_file(b"notes", fast());
    let pte = pte_file(b"notes");
    assert_eq!(unseal(&age, "wrong", age::WorkFactor::MAX).err(), Some(Error::WrongPassword));
    assert_eq!(unseal(&pte, "wrong", age::WorkFactor::MAX).err(), Some(Error::WrongPassword));
    let costly = age_file(b"notes", age::WorkFactor::new(FAST + 1).expect("a valid work factor"));
    assert_eq!(unseal(&costly, PASSWORD, fast()).err(), Some(Error::NeedsMemory));
    let mut cut = age.clone();
    cut.truncate(age.len() - 1);
    assert_eq!(unseal(&cut, PASSWORD, age::WorkFactor::MAX).err(), Some(Error::Damaged));
    assert_eq!(unseal(&vec![b' '; FILE_LIMIT + 1], PASSWORD, age::WorkFactor::MAX).err(), Some(Error::TooLong));
}

#[test]
fn text_over_the_limit_is_not_resealed() {
    for file in [age_file(b"notes", fast()), pte_file(b"notes")] {
        let (_, seal) = unseal(&file, PASSWORD, age::WorkFactor::MAX).expect("open");
        assert_eq!(seal.seal(&vec![b'a'; TEXT_LIMIT + 1], &RANDOM).err(), Some(Error::TooLong));
    }
}

#[test]
fn debug_output_names_only_the_format() {
    let (_, seal) = unseal(&pte_file(b"notes"), PASSWORD, age::WorkFactor::MAX).expect("open");
    assert_eq!(format!("{seal:?}"), "Seal(Pte)");
}
