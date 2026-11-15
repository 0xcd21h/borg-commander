use std::fmt::Write as _;
use std::ops::RangeInclusive;

use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use sha3::Sha3_512;
use zeroize::Zeroizing;

use super::Cipher;

const MASTER_LEN: usize = 256;
const MASTER_SALT: &[u8] = b"memorySaltTE";
const MASTER_INFO: &[u8] = b"memoryInfoTE";
const KEY_SALT: &[u8] = b"encKeySalt";
const KEY_INFO: &[u8] = b"encKeyInfo";
const NONCE_SALT: &[u8] = b"nonceSalt";
const NONCE_INFO: &[u8] = b"nonceInfo";
const ARGON_MEMORY_KIB: u32 = 30 * 1024;
const ARGON_PASSES: u32 = 3;
const ARGON_LANES: u32 = 4;
const STRETCHED_LEN: usize = 256;
const KEPT_AS_TYPED: RangeInclusive<u32> = 32..=126;
const LONGEST_EXPANSION: usize = 12;

pub struct MasterKey(Zeroizing<[u8; MASTER_LEN]>);

impl MasterKey {
    pub fn from_password(password: &str) -> Self {
        let converted = convert_password(password);
        let mut master = Zeroizing::new([0; MASTER_LEN]);
        hkdf_sha3(converted.as_bytes(), MASTER_SALT, MASTER_INFO, master.as_mut());
        Self(master)
    }

    #[cfg(test)]
    pub(super) fn bytes(&self) -> &[u8] {
        self.0.as_ref()
    }
}

pub(super) struct FileKeys {
    pub key: Zeroizing<Vec<u8>>,
    pub nonce: Zeroizing<Vec<u8>>,
}

pub(super) fn derive(master: &MasterKey, salt: &[u8], cipher: Cipher) -> FileKeys {
    expand(&stretch(master, salt), cipher)
}

pub(super) fn convert_password(password: &str) -> Zeroizing<String> {
    let mut converted = Zeroizing::new(String::with_capacity(password.chars().count() * LONGEST_EXPANSION));
    for character in password.chars() {
        let code = u32::from(character);
        let mut units = [0; 2];
        if let [_, low] = character.encode_utf16(&mut units) {
            let low = u32::from(*low);
            write!(converted, "{code}{low}").expect("writing to a string succeeds");
        } else if KEPT_AS_TYPED.contains(&code) {
            converted.push(character);
        } else {
            write!(converted, "{code}").expect("writing to a string succeeds");
        }
    }
    converted
}

pub(super) fn stretch(master: &MasterKey, salt: &[u8]) -> Zeroizing<[u8; STRETCHED_LEN]> {
    let params = Params::new(ARGON_MEMORY_KIB, ARGON_PASSES, ARGON_LANES, Some(STRETCHED_LEN))
        .expect("the PTE Argon2 parameters are valid");
    let mut stretched = Zeroizing::new([0; STRETCHED_LEN]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(master.0.as_ref(), salt, stretched.as_mut())
        .expect("PTE salts are long enough for Argon2");
    stretched
}

pub(super) fn expand(stretched: &[u8; STRETCHED_LEN], cipher: Cipher) -> FileKeys {
    let mut key = Zeroizing::new(vec![0; cipher.key_len()]);
    hkdf_sha3(stretched, KEY_SALT, KEY_INFO, &mut key);
    let mut nonce = Zeroizing::new(vec![0; cipher.block_len()]);
    hkdf_sha3(stretched, NONCE_SALT, NONCE_INFO, &mut nonce);
    FileKeys { key, nonce }
}

fn hkdf_sha3(input: &[u8], salt: &[u8], info: &[u8], out: &mut [u8]) {
    Hkdf::<Sha3_512>::new(Some(salt), input).expand(info, out).expect("PTE lengths fit HKDF-SHA3-512");
}
