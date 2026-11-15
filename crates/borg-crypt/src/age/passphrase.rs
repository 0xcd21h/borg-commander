use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use zeroize::Zeroizing;

use super::header::{HeaderError, Stanza};
use super::{Error, FILE_KEY_LEN, SALT_LEN, WorkFactor, decode_raw, encode_raw};

const KIND: &str = "scrypt";
const LABEL: &[u8] = b"age-encryption.org/v1/scrypt";
const BLOCK_SIZE: u32 = 8;
const PARALLELISM: u32 = 1;
const WRAP_KEY_LEN: usize = 32;
const TAG_LEN: usize = 16;
const BODY_LEN: usize = FILE_KEY_LEN + TAG_LEN;

pub(super) fn wrap(
    passphrase: &[u8],
    work_factor: WorkFactor,
    salt: &[u8; SALT_LEN],
    file_key: &[u8; FILE_KEY_LEN],
) -> Result<Stanza, Error> {
    let key = wrap_key(passphrase, salt, work_factor.log_n())?;
    let mut body = Vec::with_capacity(BODY_LEN);
    body.extend_from_slice(file_key);
    cipher(&key).encrypt_in_place(&Nonce::default(), &[], &mut body).expect("the body has room for the tag");
    Ok(Stanza { kind: KIND.to_owned(), args: vec![encode_raw(salt), work_factor.log_n().to_string()], body })
}

pub(super) fn unwrap(
    stanzas: &[Stanza],
    passphrase: &[u8],
    max: WorkFactor,
) -> Result<Zeroizing<[u8; FILE_KEY_LEN]>, Error> {
    if stanzas.len() > 1 && stanzas.iter().any(|stanza| stanza.kind == KIND) {
        return Err(HeaderError::PassphraseNotAlone.into());
    }
    let [stanza] = stanzas else {
        return Err(Error::NotForPassphrase);
    };
    if stanza.kind != KIND {
        return Err(Error::NotForPassphrase);
    }
    let [salt, work_factor] = stanza.args.as_slice() else {
        return Err(HeaderError::PassphraseStanza.into());
    };
    let salt =
        decode_raw(salt.as_bytes()).filter(|salt| salt.len() == SALT_LEN).ok_or(HeaderError::PassphraseStanza)?;
    let found = parse_work_factor(work_factor).ok_or(HeaderError::PassphraseStanza)?;
    let log_n = u8::try_from(found)
        .ok()
        .filter(|log_n| *log_n <= max.log_n())
        .ok_or(Error::WorkFactorTooHigh { found, max })?;
    if stanza.body.len() != BODY_LEN {
        return Err(HeaderError::PassphraseStanza.into());
    }
    let key = wrap_key(passphrase, &salt, log_n)?;
    let mut opened = Zeroizing::new(stanza.body.clone());
    cipher(&key).decrypt_in_place(&Nonce::default(), &[], &mut *opened).map_err(|_| Error::WrongPassphrase)?;
    let mut file_key = Zeroizing::new([0; FILE_KEY_LEN]);
    file_key.copy_from_slice(&opened);
    Ok(file_key)
}

fn parse_work_factor(text: &str) -> Option<u32> {
    let canonical = text.bytes().all(|byte| byte.is_ascii_digit()) && !text.starts_with('0');
    canonical.then(|| text.parse().ok()).flatten()
}

fn wrap_key(passphrase: &[u8], salt: &[u8], log_n: u8) -> Result<Zeroizing<[u8; WRAP_KEY_LEN]>, Error> {
    let params = scrypt::Params::new(log_n, BLOCK_SIZE, PARALLELISM).map_err(|_| HeaderError::PassphraseStanza)?;
    let labelled = [LABEL, salt].concat();
    let mut key = Zeroizing::new([0; WRAP_KEY_LEN]);
    scrypt::scrypt(passphrase, &labelled, &params, key.as_mut()).expect("32 bytes are a valid scrypt output");
    Ok(key)
}

fn cipher(key: &[u8; WRAP_KEY_LEN]) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(Key::from_slice(key))
}
