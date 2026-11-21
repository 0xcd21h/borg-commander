use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::Duration,
};

#[cfg(target_os = "linux")]
const SOCKET_VARIABLE: &str = "SSH_AUTH_SOCK";
const FAILURE: u8 = 5;
const REQUEST_IDENTITIES: u8 = 11;
const IDENTITIES_ANSWER: u8 = 12;
const SIGN_REQUEST: u8 = 13;
const SIGN_RESPONSE: u8 = 14;
const RSA_SHA2_256: u32 = 2;
const ED25519: &str = "ssh-ed25519";
const RSA: &str = "ssh-rsa";
const MESSAGE_LIMIT: usize = 256 * 1024;
const WAIT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AgentKey {
    pub blob: Vec<u8>,
    pub comment: String,
    pub kind: String,
}

impl AgentKey {
    pub fn fingerprint(&self) -> String {
        format!("SHA256:{}", STANDARD_NO_PAD.encode(Sha256::digest(&self.blob)))
    }

    fn flags(&self) -> u32 {
        if self.kind == RSA { RSA_SHA2_256 } else { 0 }
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        (self.0.len() >= len).then(|| {
            let (head, rest) = self.0.split_at(len);
            self.0 = rest;
            head
        })
    }

    fn byte(&mut self) -> Option<u8> {
        self.take(1).map(|bytes| bytes[0])
    }

    fn number(&mut self) -> Option<u32> {
        self.take(4).map(|bytes| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn string(&mut self) -> Option<&'a [u8]> {
        let len = usize::try_from(self.number()?).ok()?;
        self.take(len)
    }
}

fn put_string(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn key_kind(blob: &[u8]) -> Option<String> {
    String::from_utf8(Reader(blob).string()?.to_vec()).ok()
}

pub(super) fn parse_identities(payload: &[u8]) -> Option<Vec<AgentKey>> {
    let mut reader = Reader(payload);
    if reader.byte()? != IDENTITIES_ANSWER {
        return None;
    }
    let count = reader.number()?;
    let mut keys = Vec::new();
    for _ in 0..count {
        let blob = reader.string()?.to_vec();
        let comment = String::from_utf8_lossy(reader.string()?).into_owned();
        let kind = key_kind(&blob)?;
        if kind == ED25519 || kind == RSA {
            keys.push(AgentKey { blob, comment, kind });
        }
    }
    Some(keys)
}

pub(super) fn sign_request(key: &AgentKey, data: &[u8]) -> Vec<u8> {
    let mut payload = vec![SIGN_REQUEST];
    put_string(&mut payload, &key.blob);
    put_string(&mut payload, data);
    payload.extend_from_slice(&key.flags().to_be_bytes());
    payload
}

pub(super) fn parse_signature(payload: &[u8]) -> Option<Vec<u8>> {
    let mut reader = Reader(payload);
    (reader.byte()? == SIGN_RESPONSE).then(|| reader.string().map(<[u8]>::to_vec)).flatten()
}

pub(super) struct Agent(UnixStream);

impl Agent {
    #[cfg(target_os = "linux")]
    pub fn connect() -> Option<Self> {
        Self::at(Path::new(&std::env::var_os(SOCKET_VARIABLE)?))
    }

    pub fn at(socket: &Path) -> Option<Self> {
        let stream = UnixStream::connect(socket).ok()?;
        stream.set_read_timeout(Some(WAIT)).ok()?;
        stream.set_write_timeout(Some(WAIT)).ok()?;
        Some(Self(stream))
    }

    fn exchange(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let len = u32::try_from(payload.len()).context("the request is too long for ssh-agent")?;
        self.0.write_all(&len.to_be_bytes())?;
        self.0.write_all(payload)?;
        let mut len = [0; 4];
        self.0.read_exact(&mut len)?;
        let len = usize::try_from(u32::from_be_bytes(len))?;
        if len == 0 || len > MESSAGE_LIMIT {
            bail!("ssh-agent sent an answer of {len} bytes");
        }
        let mut answer = vec![0; len];
        self.0.read_exact(&mut answer)?;
        Ok(answer)
    }

    pub fn keys(&mut self) -> Result<Vec<AgentKey>> {
        let answer = self.exchange(&[REQUEST_IDENTITIES])?;
        parse_identities(&answer).context("ssh-agent did not list its keys")
    }

    pub fn sign(&mut self, key: &AgentKey, data: &[u8]) -> Result<Vec<u8>> {
        let answer = self.exchange(&sign_request(key, data))?;
        if answer.first() == Some(&FAILURE) {
            bail!("ssh-agent refused to sign with {}", key.fingerprint());
        }
        parse_signature(&answer).context("ssh-agent did not send a signature")
    }
}
