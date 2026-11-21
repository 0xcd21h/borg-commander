//! The ciphers Borg opens TLS records with, the AEAD suites of TLS 1.2 and 1.3, and the key
//! schedules that turn a key log's secrets into their keys.

use aes_gcm::{
    Aes128Gcm, Aes256Gcm,
    aead::{Aead, KeyInit, Payload},
};
use chacha20poly1305::ChaCha20Poly1305;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::{Sha256, Sha384};

/// Bytes of a nonce, and of the tag that ends every sealed record.
pub(super) const NONCE: usize = 12;
pub(super) const TAG: usize = 16;
/// The bytes of a TLS 1.2 GCM nonce that each record carries in front of its ciphertext.
pub(super) const EXPLICIT_NONCE: usize = 8;
const GCM_FIXED_IV: usize = 4;
const SEQUENCE: usize = 8;
const LABEL_PREFIX: &str = "tls13 ";
const KEY_LABEL: &str = "key";
const IV_LABEL: &str = "iv";
const UPDATE_LABEL: &str = "traffic upd";
const KEY_EXPANSION: &[u8] = b"key expansion";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Cipher {
    Aes128Gcm,
    Aes256Gcm,
    ChaCha20Poly1305,
}

impl Cipher {
    fn key_len(self) -> usize {
        match self {
            Self::Aes128Gcm => 16,
            Self::Aes256Gcm | Self::ChaCha20Poly1305 => 32,
        }
    }

    /// Bytes of the IV a TLS 1.2 key block gives each side.
    fn fixed_iv(self) -> usize {
        match self {
            Self::Aes128Gcm | Self::Aes256Gcm => GCM_FIXED_IV,
            Self::ChaCha20Poly1305 => NONCE,
        }
    }

    /// Whether TLS 1.2 records carry part of their nonce, as GCM's do.
    pub(super) fn explicit_nonce(self) -> bool {
        self != Self::ChaCha20Poly1305
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Hash {
    Sha256,
    Sha384,
}

/// A cipher suite Borg can open records of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Suite {
    pub cipher: Cipher,
    pub hash: Hash,
}

/// The AEAD suites, by their code: TLS 1.3's, then TLS 1.2's with RSA, DHE and ECDHE.
const SUITES: [(u16, Cipher, Hash); 17] = [
    (0x1301, Cipher::Aes128Gcm, Hash::Sha256),
    (0x1302, Cipher::Aes256Gcm, Hash::Sha384),
    (0x1303, Cipher::ChaCha20Poly1305, Hash::Sha256),
    (0x009c, Cipher::Aes128Gcm, Hash::Sha256),
    (0x009d, Cipher::Aes256Gcm, Hash::Sha384),
    (0x009e, Cipher::Aes128Gcm, Hash::Sha256),
    (0x009f, Cipher::Aes256Gcm, Hash::Sha384),
    (0x00a2, Cipher::Aes128Gcm, Hash::Sha256),
    (0x00a3, Cipher::Aes256Gcm, Hash::Sha384),
    (0xc02b, Cipher::Aes128Gcm, Hash::Sha256),
    (0xc02c, Cipher::Aes256Gcm, Hash::Sha384),
    (0xc02f, Cipher::Aes128Gcm, Hash::Sha256),
    (0xc030, Cipher::Aes256Gcm, Hash::Sha384),
    (0xcca8, Cipher::ChaCha20Poly1305, Hash::Sha256),
    (0xcca9, Cipher::ChaCha20Poly1305, Hash::Sha256),
    (0xccaa, Cipher::ChaCha20Poly1305, Hash::Sha256),
    (0xccab, Cipher::ChaCha20Poly1305, Hash::Sha256),
];

pub(super) fn suite(code: u16) -> Option<Suite> {
    SUITES.iter().find(|(known, ..)| *known == code).map(|&(_, cipher, hash)| Suite { cipher, hash })
}

/// One direction's key, and the IV its nonces start from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Key {
    pub(super) cipher: Cipher,
    pub(super) key: Vec<u8>,
    pub(super) iv: Vec<u8>,
}

impl Key {
    /// The nonce of the record `sequence` records into its direction, for the ciphers whose
    /// whole nonce comes from the IV.
    pub(super) fn nonce(&self, sequence: u64) -> [u8; NONCE] {
        let mut nonce = [0; NONCE];
        nonce.copy_from_slice(&self.iv);
        for (byte, count) in nonce[NONCE - SEQUENCE..].iter_mut().zip(sequence.to_be_bytes()) {
            *byte ^= count;
        }
        nonce
    }

    /// The nonce of a TLS 1.2 GCM record: the IV, then the part the record carries.
    pub(super) fn explicit(&self, carried: &[u8]) -> Option<[u8; NONCE]> {
        [&self.iv[..], carried].concat().try_into().ok()
    }

    /// The plaintext of `sealed`, when it opens with this key.
    pub(super) fn open(&self, nonce: &[u8; NONCE], aad: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
        let payload = Payload { msg: sealed, aad };
        let nonce = nonce.into();
        match self.cipher {
            Cipher::Aes128Gcm => Aes128Gcm::new_from_slice(&self.key).ok()?.decrypt(nonce, payload).ok(),
            Cipher::Aes256Gcm => Aes256Gcm::new_from_slice(&self.key).ok()?.decrypt(nonce, payload).ok(),
            Cipher::ChaCha20Poly1305 => ChaCha20Poly1305::new_from_slice(&self.key).ok()?.decrypt(nonce, payload).ok(),
        }
    }
}

/// The key a TLS 1.3 traffic secret gives.
pub(super) fn traffic_key(suite: Suite, secret: &[u8]) -> Option<Key> {
    Some(Key {
        cipher: suite.cipher,
        key: expand_label(suite.hash, secret, KEY_LABEL, suite.cipher.key_len())?,
        iv: expand_label(suite.hash, secret, IV_LABEL, NONCE)?,
    })
}

/// The traffic secret that follows `secret` after a TLS 1.3 key update.
pub(super) fn next_secret(hash: Hash, secret: &[u8]) -> Option<Vec<u8>> {
    expand_label(hash, secret, UPDATE_LABEL, secret.len())
}

/// TLS 1.3's HKDF-Expand-Label with an empty context.
fn expand_label(hash: Hash, secret: &[u8], label: &str, length: usize) -> Option<Vec<u8>> {
    let full = format!("{LABEL_PREFIX}{label}");
    let mut info = u16::try_from(length).ok()?.to_be_bytes().to_vec();
    info.push(u8::try_from(full.len()).ok()?);
    info.extend(full.as_bytes());
    info.push(0);
    let mut out = vec![0; length];
    match hash {
        Hash::Sha256 => Hkdf::<Sha256>::from_prk(secret).ok()?.expand(&info, &mut out).ok()?,
        Hash::Sha384 => Hkdf::<Sha384>::from_prk(secret).ok()?.expand(&info, &mut out).ok()?,
    }
    Some(out)
}

/// The client's and the server's keys a TLS 1.2 master secret gives.
pub(super) fn session_keys(suite: Suite, master: &[u8], client: &[u8], server: &[u8]) -> Option<(Key, Key)> {
    let (key_len, iv_len) = (suite.cipher.key_len(), suite.cipher.fixed_iv());
    let block = prf(suite.hash, master, &[KEY_EXPANSION, server, client].concat(), 2 * (key_len + iv_len))?;
    let (keys, ivs) = block.split_at(2 * key_len);
    let key = |key: &[u8], iv: &[u8]| Key { cipher: suite.cipher, key: key.to_vec(), iv: iv.to_vec() };
    Some((key(&keys[..key_len], &ivs[..iv_len]), key(&keys[key_len..], &ivs[iv_len..])))
}

/// TLS 1.2's pseudorandom function: `length` bytes of P_hash over the label and seed in `seed`.
fn prf(hash: Hash, secret: &[u8], seed: &[u8], length: usize) -> Option<Vec<u8>> {
    match hash {
        Hash::Sha256 => p_hash::<Hmac<Sha256>>(secret, seed, length),
        Hash::Sha384 => p_hash::<Hmac<Sha384>>(secret, seed, length),
    }
}

fn p_hash<M: Mac + KeyInit + Clone>(secret: &[u8], seed: &[u8], length: usize) -> Option<Vec<u8>> {
    let keyed = <M as Mac>::new_from_slice(secret).ok()?;
    let mut out = Vec::with_capacity(length);
    let mut chained = seed.to_vec();
    while out.len() < length {
        chained = keyed.clone().chain_update(&chained).finalize().into_bytes().to_vec();
        out.extend(keyed.clone().chain_update(&chained).chain_update(seed).finalize().into_bytes());
    }
    out.truncate(length);
    Some(out)
}
