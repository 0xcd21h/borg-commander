use anyhow::{Context, Result, anyhow};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::Zeroizing;

pub(super) const SEALED_FILE: &str = "computer-key.sealed";
pub(super) const KEY_LEN: usize = 32;
const FORMAT: u32 = 1;
const NONCE_LEN: usize = 24;
const SALT_LEN: usize = 16;
const KEYS_LABEL: &[u8] = b"borg computer keys 1";
const PASSWORD_LABEL: &[u8] = b"borg password slot 1";
const AGENT_LABEL: &[u8] = b"borg ssh-agent slot 1";
const AGENT_CHALLENGE: &[u8] = b"borg-commander unlocks this computer's key 1 ";
const FULL_MEMORY_KIB: u64 = 1024 * 1024;
const LEAST_MEMORY_KIB: u64 = 64 * 1024;
const MEMORY_SHARE: u64 = 8;
const PASSES: u32 = 3;
const LANES: u32 = 4;

pub(super) type DataKey = Zeroizing<[u8; KEY_LEN]>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Cost {
    pub memory_kib: u32,
    pub passes: u32,
    pub lanes: u32,
}

impl Cost {
    pub fn for_memory(total_bytes: u64) -> Self {
        let memory_kib = (total_bytes / 1024 / MEMORY_SHARE).clamp(LEAST_MEMORY_KIB, FULL_MEMORY_KIB);
        Self { memory_kib: u32::try_from(memory_kib).unwrap_or(u32::MAX), passes: PASSES, lanes: LANES }
    }

    pub fn for_this_machine() -> Self {
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        Self::for_memory(system.total_memory())
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Boxed {
    nonce: String,
    sealed: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct PasswordSlot {
    cost: Cost,
    salt: String,
    key: Boxed,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct AgentSlot {
    pub fingerprint: String,
    salt: String,
    key: Boxed,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Sealed {
    format: u32,
    password: PasswordSlot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentSlot>,
    keys: Boxed,
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|error| anyhow!("{error}"))?;
    Ok(bytes)
}

fn decode(text: &str) -> Option<Vec<u8>> {
    STANDARD.decode(text).ok()
}

fn lock_box(key: &[u8; KEY_LEN], label: &[u8], plain: &[u8]) -> Result<Boxed> {
    let nonce: [u8; NONCE_LEN] = random()?;
    let sealed = XChaCha20Poly1305::new(key.into())
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: plain, aad: label })
        .map_err(|_| anyhow!("sealing failed"))?;
    Ok(Boxed { nonce: STANDARD.encode(nonce), sealed: STANDARD.encode(sealed) })
}

fn open_box(key: &[u8; KEY_LEN], label: &[u8], boxed: &Boxed) -> Option<Zeroizing<Vec<u8>>> {
    let nonce = decode(&boxed.nonce).filter(|nonce| nonce.len() == NONCE_LEN)?;
    let sealed = decode(&boxed.sealed)?;
    XChaCha20Poly1305::new(key.into())
        .decrypt(XNonce::from_slice(&nonce), Payload { msg: &sealed, aad: label })
        .ok()
        .map(Zeroizing::new)
}

fn data_key(bytes: &[u8]) -> Option<DataKey> {
    <[u8; KEY_LEN]>::try_from(bytes).ok().map(Zeroizing::new)
}

fn stretch(password: &str, salt: &[u8], cost: Cost) -> Result<DataKey> {
    let params =
        Params::new(cost.memory_kib, cost.passes, cost.lanes, Some(KEY_LEN)).map_err(|error| anyhow!("{error}"))?;
    let mut out = Zeroizing::new([0; KEY_LEN]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, out.as_mut())
        .map_err(|error| anyhow!("{error}"))?;
    Ok(out)
}

fn agent_key(signature: &[u8], salt: &[u8]) -> DataKey {
    let mut out = Zeroizing::new([0; KEY_LEN]);
    Hkdf::<Sha256>::new(Some(salt), signature).expand(AGENT_LABEL, out.as_mut()).expect("32 bytes fit HKDF-SHA256");
    out
}

pub(super) fn agent_challenge(salt: &[u8]) -> Vec<u8> {
    [AGENT_CHALLENGE, salt].concat()
}

impl Sealed {
    pub fn seal(password: &str, cost: Cost, keys: &str) -> Result<(Self, DataKey)> {
        let data = Zeroizing::new(random::<KEY_LEN>()?);
        let salt: [u8; SALT_LEN] = random()?;
        let wrapping = stretch(password, &salt, cost)?;
        let password = PasswordSlot {
            cost,
            salt: STANDARD.encode(salt),
            key: lock_box(&wrapping, PASSWORD_LABEL, data.as_ref())?,
        };
        let keys = lock_box(&data, KEYS_LABEL, keys.as_bytes())?;
        Ok((Self { format: FORMAT, password, agent: None, keys }, data))
    }

    pub fn from_text(text: &str) -> Result<Self> {
        let sealed: Self = serde_json::from_str(text).context("this computer's sealed key file is damaged")?;
        if sealed.format != FORMAT {
            anyhow::bail!("this computer's sealed key file comes from a newer Borg");
        }
        Ok(sealed)
    }

    pub fn to_text(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn open_with_password(&self, password: &str) -> Result<Option<DataKey>> {
        let salt = decode(&self.password.salt).context("this computer's sealed key file is damaged")?;
        let wrapping = stretch(password, &salt, self.password.cost)?;
        Ok(open_box(&wrapping, PASSWORD_LABEL, &self.password.key).and_then(|bytes| data_key(&bytes)))
    }

    pub fn agent_salt(&self) -> Option<Vec<u8>> {
        self.agent.as_ref().and_then(|slot| decode(&slot.salt))
    }

    pub fn open_with_signature(&self, signature: &[u8]) -> Option<DataKey> {
        let slot = self.agent.as_ref()?;
        let wrapping = agent_key(signature, &decode(&slot.salt)?);
        open_box(&wrapping, AGENT_LABEL, &slot.key).and_then(|bytes| data_key(&bytes))
    }

    pub fn new_agent_salt() -> Result<[u8; SALT_LEN]> {
        random()
    }

    pub fn allow_agent(&mut self, data: &DataKey, fingerprint: &str, salt: &[u8], signature: &[u8]) -> Result<()> {
        let wrapping = agent_key(signature, salt);
        let key = lock_box(&wrapping, AGENT_LABEL, data.as_ref())?;
        self.agent = Some(AgentSlot { fingerprint: fingerprint.into(), salt: STANDARD.encode(salt), key });
        Ok(())
    }

    pub fn open(&self, data: &DataKey) -> Result<Zeroizing<String>> {
        let bytes =
            open_box(data, KEYS_LABEL, &self.keys).context("the unlocked key does not open this computer's keys")?;
        String::from_utf8(bytes.to_vec()).map(Zeroizing::new).context("this computer's sealed key file is damaged")
    }

    pub fn reseal(&mut self, data: &DataKey, keys: &str) -> Result<()> {
        self.keys = lock_box(data, KEYS_LABEL, keys.as_bytes())?;
        Ok(())
    }
}
