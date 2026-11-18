use std::fmt;

use zeroize::Zeroizing;

use crate::{age, pte};

#[cfg(test)]
mod tests;

pub const FILE_LIMIT: usize = 1024 * 1024;
pub const RANDOM_LEN: usize = 16;

const AGE_INTRO: &[u8] = b"age-encryption.org/v1\n";
const AGE_ARMOR: &[u8] = b"-----BEGIN AGE ENCRYPTED FILE-----";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Age,
    Pte,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("wrong password")]
    WrongPassword,
    #[error("the file is damaged")]
    Damaged,
    #[error("the file is encrypted in a way Borg cannot open")]
    Unsupported,
    #[error("the file is encrypted for a key, not a password")]
    NotForPassword,
    #[error("the text is too long")]
    TooLong,
    #[error("opening the file needs more memory than is free")]
    NeedsMemory,
}

pub fn age_start(start: &[u8]) -> bool {
    let trimmed = start.trim_ascii_start();
    start.starts_with(AGE_INTRO) || trimmed.starts_with(AGE_ARMOR)
}

pub fn detect(file: &[u8]) -> Option<Format> {
    if age_start(file) {
        return Some(Format::Age);
    }
    match pte::Envelope::parse(file) {
        Ok(_) | Err(pte::Error::UnsupportedCipher(_)) => Some(Format::Pte),
        Err(_) => None,
    }
}

pub fn unseal(file: &[u8], password: &str, max: age::WorkFactor) -> Result<(Zeroizing<Vec<u8>>, Seal), Error> {
    if file.len() > FILE_LIMIT {
        return Err(Error::TooLong);
    }
    if age_start(file) {
        let (text, rekey) = age::open_text(file, password.as_bytes(), max).map_err(age_error)?;
        return Ok((text, Seal::Age(rekey)));
    }
    let (text, rekey) = pte::open_text(file, password).map_err(pte_error)?;
    Ok((text, Seal::Pte(Box::new(rekey))))
}

#[derive(Clone, PartialEq, Eq)]
pub enum Seal {
    Age(age::Rekey),
    Pte(Box<pte::Rekey>),
}

impl Seal {
    pub fn format(&self) -> Format {
        match self {
            Self::Age(_) => Format::Age,
            Self::Pte(_) => Format::Pte,
        }
    }

    pub fn seal(&self, text: &[u8], random: &[u8; RANDOM_LEN]) -> Result<Vec<u8>, Error> {
        match self {
            Self::Age(rekey) => rekey.seal_text(text, random).map_err(age_error),
            Self::Pte(rekey) => rekey.seal_text(text, random).map(String::into_bytes).map_err(pte_error),
        }
    }
}

impl fmt::Debug for Seal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Seal({:?})", self.format())
    }
}

fn age_error(error: age::Error) -> Error {
    match error {
        age::Error::WrongPassphrase => Error::WrongPassword,
        age::Error::NotForPassphrase => Error::NotForPassword,
        age::Error::WorkFactorTooHigh { .. } => Error::NeedsMemory,
        age::Error::TooLong { .. } => Error::TooLong,
        age::Error::Header(_)
        | age::Error::Armor(_)
        | age::Error::HeaderMac
        | age::Error::Payload(_)
        | age::Error::Io(_) => Error::Damaged,
    }
}

fn pte_error(error: pte::Error) -> Error {
    match error {
        pte::Error::WrongPasswordOrDamaged => Error::WrongPassword,
        pte::Error::UnsupportedVersion(_) | pte::Error::UnsupportedCipher(_) | pte::Error::UnsupportedCompression => {
            Error::Unsupported
        }
        pte::Error::FileTooLarge { .. } | pte::Error::TooLong { .. } => Error::TooLong,
        pte::Error::NotPte | pte::Error::Damaged => Error::Damaged,
    }
}
