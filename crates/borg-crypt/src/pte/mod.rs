mod eax;
mod kdf;

#[cfg(test)]
mod tests;

use std::io::Read;

use aes::Aes256;
use aes::cipher::{BlockSizeUser, KeySizeUser};
use base64::alphabet::Alphabet;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::{DecodePaddingMode, Engine as _};
use blowfish::Blowfish;
use lzma_rust2::LzmaReader;
use zeroize::Zeroizing;

pub use kdf::MasterKey;

use crate::TEXT_LIMIT;

pub const FILE_LIMIT: usize = 512 * 1024;
pub const RANDOM_LEN: usize = 16;

const BYTE_ORDER_MARK: &[u8] = b"\xef\xbb\xbf";
const END_MARK: u8 = b'!';
const PADDING: u8 = b'.';
const VERSION: u8 = 3;
const VERSION_SHIFT: u32 = 4;
const VERSION_MASK: u8 = 0b0111;
const CIPHER_MASK: u8 = 0b1111;
const COMPRESSED: u8 = 0b1000_0000;
const LZMA_PROPERTIES: u8 = 0x5d;
const LZMA_DICTIONARY: std::ops::Range<usize> = 1..5;
const LZMA_SIZE: std::ops::Range<usize> = 5..13;

const ALPHABET: Alphabet = match Alphabet::new("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789)]") {
    Ok(alphabet) => alphabet,
    Err(_) => panic!("the PTE alphabet has 64 distinct characters"),
};

const PTE_BASE64: GeneralPurpose = GeneralPurpose::new(
    &ALPHABET,
    GeneralPurposeConfig::new()
        .with_encode_padding(false)
        .with_decode_padding_mode(DecodePaddingMode::RequireNone)
        .with_decode_allow_trailing_bits(true),
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cipher {
    Aes256,
    Blowfish448,
}

impl Cipher {
    const AES_256: u8 = 0;
    const BLOWFISH_448: u8 = 6;

    fn from_code(code: u8) -> Result<Self, Error> {
        match code {
            Self::AES_256 => Ok(Self::Aes256),
            Self::BLOWFISH_448 => Ok(Self::Blowfish448),
            other => Err(Error::UnsupportedCipher(other)),
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::Aes256 => Self::AES_256,
            Self::Blowfish448 => Self::BLOWFISH_448,
        }
    }

    fn block_len(self) -> usize {
        match self {
            Self::Aes256 => Aes256::block_size(),
            Self::Blowfish448 => <Blowfish>::block_size(),
        }
    }

    fn key_len(self) -> usize {
        match self {
            Self::Aes256 => Aes256::key_size(),
            Self::Blowfish448 => <Blowfish>::key_size(),
        }
    }

    fn seal(self, key: &[u8], nonce: &[u8], aad: &[u8], buffer: &mut [u8]) -> Vec<u8> {
        match self {
            Self::Aes256 => eax::seal::<Aes256>(key, nonce, aad, buffer),
            Self::Blowfish448 => eax::seal::<Blowfish>(key, nonce, aad, buffer),
        }
    }

    fn open(self, key: &[u8], nonce: &[u8], aad: &[u8], buffer: &mut [u8], tag: &[u8]) -> Result<(), eax::TagMismatch> {
        match self {
            Self::Aes256 => eax::open::<Aes256>(key, nonce, aad, buffer, tag),
            Self::Blowfish448 => eax::open::<Blowfish>(key, nonce, aad, buffer, tag),
        }
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("not a Paranoia Text Encryption file")]
    NotPte,
    #[error("the file is larger than {limit} bytes")]
    FileTooLarge { limit: usize },
    #[error("format version {0} is not supported")]
    UnsupportedVersion(u8),
    #[error("cipher {0} is not supported")]
    UnsupportedCipher(u8),
    #[error("wrong password, or the file is damaged")]
    WrongPasswordOrDamaged,
    #[error("the text is compressed with a method that is not supported")]
    UnsupportedCompression,
    #[error("the compressed text is damaged")]
    Damaged,
    #[error("the text is longer than {limit} bytes")]
    TooLong { limit: usize },
}

pub struct Envelope {
    cipher: Cipher,
    config: u8,
    salt: Vec<u8>,
    ciphertext: Vec<u8>,
    tag: Vec<u8>,
}

impl Envelope {
    pub fn parse(file: &[u8]) -> Result<Self, Error> {
        if file.len() > FILE_LIMIT {
            return Err(Error::FileTooLarge { limit: FILE_LIMIT });
        }
        let file = file.strip_prefix(BYTE_ORDER_MARK).unwrap_or(file);
        let mut text: Vec<u8> = file.iter().copied().filter(|byte| !is_space(*byte)).collect();
        if text.pop() != Some(END_MARK) {
            return Err(Error::NotPte);
        }
        while text.last() == Some(&PADDING) {
            text.pop();
        }
        let mut bytes = PTE_BASE64.decode(&text).map_err(|_| Error::NotPte)?;
        let config = bytes.pop().ok_or(Error::NotPte)?;
        let version = (config >> VERSION_SHIFT) & VERSION_MASK;
        if version != VERSION {
            return Err(Error::UnsupportedVersion(version));
        }
        let cipher = Cipher::from_code(config & CIPHER_MASK)?;
        let block = cipher.block_len();
        if bytes.len() < 2 * block {
            return Err(Error::NotPte);
        }
        let tag = bytes.split_off(bytes.len() - block);
        let ciphertext = bytes.split_off(block);
        Ok(Self { cipher, config, salt: bytes, ciphertext, tag })
    }

    pub fn cipher(&self) -> Cipher {
        self.cipher
    }

    pub fn compressed(&self) -> bool {
        self.config & COMPRESSED != 0
    }

    pub fn open(&self, master: &MasterKey) -> Result<Zeroizing<Vec<u8>>, Error> {
        let keys = kdf::derive(master, &self.salt, self.cipher);
        let mut plain = Zeroizing::new(self.ciphertext.clone());
        self.cipher
            .open(&keys.key, &keys.nonce, &aad(&self.salt, self.config), &mut plain, &self.tag)
            .map_err(|_| Error::WrongPasswordOrDamaged)?;
        if self.compressed() {
            return decompress(&plain);
        }
        if plain.len() > TEXT_LIMIT {
            return Err(Error::TooLong { limit: TEXT_LIMIT });
        }
        Ok(plain)
    }
}

pub fn seal(text: &[u8], master: &MasterKey, cipher: Cipher, random: &[u8; RANDOM_LEN]) -> Result<String, Error> {
    if text.len() > TEXT_LIMIT {
        return Err(Error::TooLong { limit: TEXT_LIMIT });
    }
    Ok(seal_payload(text, master, cipher, random, VERSION << VERSION_SHIFT))
}

fn seal_payload(payload: &[u8], master: &MasterKey, cipher: Cipher, random: &[u8; RANDOM_LEN], flags: u8) -> String {
    let block = cipher.block_len();
    let salt = &random[..block];
    let config = flags | cipher.code();
    let keys = kdf::derive(master, salt, cipher);
    let mut sealed = Zeroizing::new(Vec::with_capacity(block + payload.len() + block + 1));
    sealed.extend_from_slice(salt);
    sealed.extend_from_slice(payload);
    let tag = cipher.seal(&keys.key, &keys.nonce, &aad(salt, config), &mut sealed[block..]);
    sealed.extend_from_slice(&tag);
    sealed.push(config);
    let mut text = PTE_BASE64.encode(sealed.as_slice());
    text.push(char::from(END_MARK));
    text
}

fn aad(salt: &[u8], config: u8) -> Vec<u8> {
    [salt, &[config]].concat()
}

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

fn decompress(packed: &[u8]) -> Result<Zeroizing<Vec<u8>>, Error> {
    if packed.first() != Some(&LZMA_PROPERTIES) {
        return Err(Error::UnsupportedCompression);
    }
    let (Some(dictionary), Some(declared)) = (packed.get(LZMA_DICTIONARY), packed.get(LZMA_SIZE)) else {
        return Err(Error::Damaged);
    };
    let dictionary = u32::from_le_bytes(dictionary.try_into().expect("four bytes"));
    let declared = u64::from_le_bytes(declared.try_into().expect("eight bytes"));
    let size = usize::try_from(declared)
        .ok()
        .filter(|size| *size <= TEXT_LIMIT)
        .ok_or(Error::TooLong { limit: TEXT_LIMIT })?;
    let stream = &packed[LZMA_SIZE.end..];
    let mut reader =
        LzmaReader::new_with_props(stream, declared, LZMA_PROPERTIES, dictionary, None).map_err(|_| Error::Damaged)?;
    let mut text = Zeroizing::new(vec![0; size]);
    reader.read_exact(&mut text).map_err(|_| Error::Damaged)?;
    Ok(text)
}
