//! A borg's identity: a static X25519 key pair, kept on disk readable only by its owner.

use std::{
    fmt, fs,
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::Path,
};

pub const KEY_LEN: usize = 32;
pub type PublicKey = [u8; KEY_LEN];

const PRIVATE_FILE: u32 = 0o600;
const PRIVATE_DIR: u32 = 0o700;
const OPEN_TO_OTHERS: u32 = 0o077;
/// Bytes of a key a fingerprint shows.
const FINGERPRINT_LEN: usize = 8;

#[derive(Clone)]
pub struct Identity {
    private: [u8; KEY_LEN],
    public: PublicKey,
}

impl Drop for Identity {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.private);
    }
}

impl PartialEq for Identity {
    fn eq(&self, other: &Self) -> bool {
        self.public == other.public
    }
}

impl Eq for Identity {}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Identity({})", fingerprint(&self.public))
    }
}

impl Identity {
    pub fn generate() -> Self {
        let params = crate::channel::NOISE.parse().expect("the Noise pattern is valid");
        let mut pair = snow::Builder::new(params).generate_keypair().expect("the system gives random bytes");
        let mut identity = Self { private: [0; KEY_LEN], public: [0; KEY_LEN] };
        identity.private.copy_from_slice(&pair.private);
        identity.public.copy_from_slice(&pair.public);
        zeroize::Zeroize::zeroize(&mut pair.private);
        identity
    }

    pub fn from_private(private: &[u8; KEY_LEN]) -> Self {
        let mut dh = curve25519();
        dh.set(private);
        let mut identity = Self { private: *private, public: [0; KEY_LEN] };
        identity.public.copy_from_slice(dh.pubkey());
        identity
    }

    pub(crate) fn shared_secret(&self, peer: &PublicKey) -> Option<[u8; KEY_LEN]> {
        let mut dh = curve25519();
        dh.set(&self.private);
        let mut secret = [0; KEY_LEN];
        dh.dh(peer, &mut secret).ok()?;
        (secret != [0; KEY_LEN]).then_some(secret)
    }

    pub fn to_text(&self) -> String {
        format!("{}{}", hex(&self.private), hex(&self.public))
    }

    pub fn from_text(text: &str) -> Option<Self> {
        Self::decode(text.trim()).filter(|identity| Self::from_private(&identity.private).public == identity.public)
    }

    /// The identity kept at `path`, made there on first use. The file holds the private key,
    /// so it is created readable only by its owner, and one others can read is refused.
    pub fn load_or_create(path: &Path) -> io::Result<Self> {
        match fs::metadata(path) {
            Ok(meta) => {
                if meta.mode() & OPEN_TO_OTHERS != 0 || meta.uid() != rustix::process::getuid().as_raw() {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!("{} must belong to you and be closed to others", path.display()),
                    ));
                }
                let text = fs::read_to_string(path)?;
                Self::decode(text.trim()).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, format!("{} does not hold a borg key", path.display()))
                })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if let Some(dir) = path.parent() {
                    fs::DirBuilder::new().recursive(true).mode(PRIVATE_DIR).create(dir)?;
                }
                let identity = Self::generate();
                let mut file = fs::OpenOptions::new().write(true).create_new(true).mode(PRIVATE_FILE).open(path)?;
                writeln!(file, "{}", identity.to_text())?;
                Ok(identity)
            }
            Err(error) => Err(error),
        }
    }

    fn decode(text: &str) -> Option<Self> {
        let both: [u8; KEY_LEN * 2] = unhex(text)?;
        let mut identity = Self { private: [0; KEY_LEN], public: [0; KEY_LEN] };
        identity.private.copy_from_slice(&both[..KEY_LEN]);
        identity.public.copy_from_slice(&both[KEY_LEN..]);
        Some(identity)
    }

    pub fn public(&self) -> &PublicKey {
        &self.public
    }

    pub(crate) fn private(&self) -> &[u8] {
        &self.private
    }
}

fn curve25519() -> Box<dyn snow::types::Dh> {
    use snow::resolvers::CryptoResolver;
    snow::resolvers::DefaultResolver
        .resolve_dh(&snow::params::DHChoice::Curve25519)
        .expect("the default resolver has Curve25519")
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn unhex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != N * 2 || !text.is_ascii() {
        return None;
    }
    let mut out = [0; N];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// A short form of a key for people to compare: its first bytes, in groups of four digits.
pub fn fingerprint(key: &PublicKey) -> String {
    let digits = hex(&key[..FINGERPRINT_LEN]);
    digits.as_bytes().chunks(4).map(|group| String::from_utf8_lossy(group).into_owned()).collect::<Vec<_>>().join("-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn an_identity_is_kept_owner_only_and_one_others_can_read_is_refused() {
        let dir = std::env::temp_dir().join(format!("borg-link-key-{}", std::process::id()));
        let path = dir.join("link").join("key");
        let made = Identity::load_or_create(&path).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, PRIVATE_FILE);
        let again = Identity::load_or_create(&path).unwrap();
        assert_eq!(made.public(), again.public());
        assert_eq!(made.private(), again.private());

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Identity::load_or_create(&path).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_identity_reads_back_from_its_text_and_both_ends_share_a_secret() {
        let one = Identity::generate();
        let other = Identity::generate();
        assert_eq!(Identity::from_text(&one.to_text()), Some(one.clone()));
        assert_eq!(Identity::from_private(&one.private).public(), one.public());
        let mut damaged = one.to_text();
        damaged.replace_range(..2, if damaged.starts_with("00") { "01" } else { "00" });
        assert_eq!(Identity::from_text(&damaged), None);
        assert_eq!(one.shared_secret(other.public()), other.shared_secret(one.public()));
        assert_eq!(one.shared_secret(&[0; KEY_LEN]), None);
    }

    #[test]
    fn keys_read_back_from_hex_and_fingerprints_are_short() {
        let key: PublicKey = std::array::from_fn(|i| i as u8);
        assert_eq!(unhex::<KEY_LEN>(&hex(&key)), Some(key));
        assert_eq!(unhex::<KEY_LEN>("zz"), None);
        assert_eq!(fingerprint(&key), "0001-0203-0405-0607");
    }
}
