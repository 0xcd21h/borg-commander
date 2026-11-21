use ring::signature::{ED25519, UnparsedPublicKey};

use crate::UpdateError;

/// The Ed25519 public keys that release checksum files may be signed with, each as 64 hex
/// digits. A release is trusted when any of them made its signature, so a release that adds a
/// key here lets the copies that install it follow a change of signing key.
/// `scripts/sign-release.sh` refuses to sign a release when the signing key is not in this list.
/// While the list is empty, every update is refused.
pub const RELEASE_PUBLIC_KEYS: &[&str] = &["41045941fe23335a32add1dcf7fc2c8aedfd6d8e47b82a09e184f8c55ad77cca"];

/// A signature file holds the 64 raw bytes of an Ed25519 signature over the checksum file.
pub(crate) const SIGNATURE_SUFFIX: &str = ".sig";
pub(crate) const SIGNATURE_LIMIT: u64 = 1024;

const KEY_BYTES: usize = 32;

pub(crate) type PublicKey = [u8; KEY_BYTES];

const fn nibble(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

pub(crate) const fn parse_key(hex: &str) -> Option<PublicKey> {
    let digits = hex.as_bytes();
    if digits.len() != KEY_BYTES * 2 {
        return None;
    }
    let mut key = [0; KEY_BYTES];
    let mut index = 0;
    while index < KEY_BYTES {
        let (Some(high), Some(low)) = (nibble(digits[index * 2]), nibble(digits[index * 2 + 1])) else {
            return None;
        };
        key[index] = high << 4 | low;
        index += 1;
    }
    Some(key)
}

const fn all_keys_parse(keys: &[&str]) -> bool {
    let mut index = 0;
    while index < keys.len() {
        if parse_key(keys[index]).is_none() {
            return false;
        }
        index += 1;
    }
    true
}

const _: () = assert!(all_keys_parse(RELEASE_PUBLIC_KEYS), "every key in RELEASE_PUBLIC_KEYS must be 64 hex digits");

pub(crate) fn release_keys() -> Result<Vec<PublicKey>, UpdateError> {
    let keys: Vec<PublicKey> = RELEASE_PUBLIC_KEYS.iter().filter_map(|key| parse_key(key)).collect();
    if keys.is_empty() { Err(UpdateError::NoReleaseKey) } else { Ok(keys) }
}

pub(crate) fn verify_signature(
    keys: &[PublicKey],
    signed: &[u8],
    signature: &[u8],
    name: &str,
) -> Result<(), UpdateError> {
    let trusted = keys.iter().any(|key| UnparsedPublicKey::new(&ED25519, key).verify(signed, signature).is_ok());
    if trusted { Ok(()) } else { Err(UpdateError::Signature(name.to_owned())) }
}
