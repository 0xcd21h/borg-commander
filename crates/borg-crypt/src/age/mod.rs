mod armor;
mod header;
mod passphrase;
mod stream;

#[cfg(test)]
mod tests;

use std::io::{self, BufReader, Chain, Cursor, Read, Write};

use base64::alphabet;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::{DecodePaddingMode, Engine as _};
use zeroize::Zeroizing;

pub use armor::ArmorError;
pub use header::HeaderError;
pub use stream::{Decryptor, Encryptor, PayloadError};

use crate::TEXT_LIMIT;

pub const FILE_KEY_LEN: usize = 16;
pub const SALT_LEN: usize = 16;
pub const NONCE_LEN: usize = 16;

const BINARY_INTRO: &[u8] = b"age-encryption.org/";

const RAW_BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_encode_padding(false)
        .with_decode_padding_mode(DecodePaddingMode::RequireNone)
        .with_decode_allow_trailing_bits(false),
);

fn decode_raw(text: &[u8]) -> Option<Vec<u8>> {
    RAW_BASE64.decode(text).ok()
}

fn encode_raw(bytes: &[u8]) -> String {
    RAW_BASE64.encode(bytes)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkFactor(u8);

impl WorkFactor {
    pub const DEFAULT: Self = Self(20);
    pub const MAX: Self = Self(22);
    const MIN: Self = Self(1);

    pub fn new(log_n: u8) -> Option<Self> {
        (Self::MIN.0..=Self::MAX.0).contains(&log_n).then_some(Self(log_n))
    }

    pub fn log_n(self) -> u8 {
        self.0
    }
}

pub struct Fresh {
    pub file_key: Zeroizing<[u8; FILE_KEY_LEN]>,
    pub salt: [u8; SALT_LEN],
    pub nonce: [u8; NONCE_LEN],
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a valid age file: {0}")]
    Header(#[from] HeaderError),
    #[error("not a valid armored age file: {0}")]
    Armor(#[from] ArmorError),
    #[error("the file is not encrypted with a passphrase")]
    NotForPassphrase,
    #[error("wrong passphrase")]
    WrongPassphrase,
    #[error("the passphrase work factor {found} is above the limit {}", max.log_n())]
    WorkFactorTooHigh { found: u32, max: WorkFactor },
    #[error("the file header is damaged")]
    HeaderMac,
    #[error("the encrypted content is damaged: {0}")]
    Payload(#[from] PayloadError),
    #[error("the text is longer than {limit} bytes")]
    TooLong { limit: usize },
    #[error(transparent)]
    Io(io::Error),
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        if let Some(armor) = error.get_ref().and_then(|inner| inner.downcast_ref::<ArmorError>()) {
            return Self::Armor(*armor);
        }
        if let Some(payload) = error.get_ref().and_then(|inner| inner.downcast_ref::<PayloadError>()) {
            return Self::Payload(*payload);
        }
        Self::Io(error)
    }
}

pub fn encrypt<W: Write>(
    mut out: W,
    passphrase: &[u8],
    work_factor: WorkFactor,
    fresh: &Fresh,
) -> Result<Encryptor<W>, Error> {
    let stanza = passphrase::wrap(passphrase, work_factor, &fresh.salt, &fresh.file_key)?;
    out.write_all(&header::encode(&[stanza], &fresh.file_key))?;
    out.write_all(&fresh.nonce)?;
    Ok(Encryptor::new(out, &fresh.file_key, &fresh.nonce))
}

pub fn decrypt<R: Read>(input: R, passphrase: &[u8], max: WorkFactor) -> Result<Decryptor<R>, Error> {
    let mut input = BufReader::new(Input::detect(input)?);
    let header = header::read(&mut input)?;
    let file_key = passphrase::unwrap(&header.stanzas, passphrase, max)?;
    open_payload(input, &header, &file_key)
}

pub fn encrypt_text(text: &[u8], passphrase: &[u8], work_factor: WorkFactor, fresh: &Fresh) -> Result<Vec<u8>, Error> {
    if text.len() > TEXT_LIMIT {
        return Err(Error::TooLong { limit: TEXT_LIMIT });
    }
    let mut encryptor = encrypt(Vec::new(), passphrase, work_factor, fresh)?;
    encryptor.write_all(text)?;
    Ok(encryptor.finish()?)
}

pub fn decrypt_text(file: &[u8], passphrase: &[u8], max: WorkFactor) -> Result<Zeroizing<Vec<u8>>, Error> {
    let mut decryptor = decrypt(file, passphrase, max)?;
    let mut text = Zeroizing::new(vec![0; TEXT_LIMIT + 1]);
    let mut filled = 0;
    while filled < text.len() {
        match decryptor.read(&mut text[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    if filled > TEXT_LIMIT {
        return Err(Error::TooLong { limit: TEXT_LIMIT });
    }
    text.truncate(filled);
    Ok(text)
}

fn open_payload<R: Read>(
    mut input: BufReader<Input<R>>,
    header: &header::Header,
    file_key: &[u8; FILE_KEY_LEN],
) -> Result<Decryptor<R>, Error> {
    header.verify(file_key)?;
    let mut nonce = [0; NONCE_LEN];
    if read_full(&mut input, &mut nonce)? < NONCE_LEN {
        return Err(HeaderError::MissingNonce.into());
    }
    Ok(Decryptor::new(input, file_key, &nonce))
}

#[cfg(test)]
fn decrypt_with_file_key<R: Read>(input: R, file_key: &[u8; FILE_KEY_LEN]) -> Result<Decryptor<R>, Error> {
    let mut input = BufReader::new(Input::detect(input)?);
    let header = header::read(&mut input)?;
    open_payload(input, &header, file_key)
}

fn read_full(input: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match input.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

type Rejoined<R> = Chain<Cursor<Vec<u8>>, R>;

enum Input<R: Read> {
    Binary(Rejoined<R>),
    Armored(armor::ArmorReader<Rejoined<R>>),
}

impl<R: Read> Input<R> {
    fn detect(mut input: R) -> io::Result<Self> {
        let mut start = vec![0; BINARY_INTRO.len()];
        let read = read_full(&mut input, &mut start)?;
        start.truncate(read);
        let binary = BINARY_INTRO.starts_with(&start);
        let rejoined = Cursor::new(start).chain(input);
        Ok(if binary { Self::Binary(rejoined) } else { Self::Armored(armor::ArmorReader::new(rejoined)) })
    }
}

impl<R: Read> Read for Input<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Binary(input) => input.read(buffer),
            Self::Armored(input) => input.read(buffer),
        }
    }
}
